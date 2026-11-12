//! BPM / key analysis through the `khole-analyze` sidecar (Essentia).
//!
//! The sidecar never decodes audio itself: Rust decodes to mono f32le PCM (symphonia) and
//! streams it to the sidecar's stdin, which answers with one JSON line. The background
//! queue that runs it for a session lives in [`queue`].

#[cfg(test)]
mod bench;
pub mod queue;

use std::io::{BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::audio::decode::MonoDecoder;
use crate::error::{Error, Result};
use crate::store::analysis::Analysis;

/// Sidecar name as registered in `tauri.conf.json` → `bundle.externalBin`.
pub const SIDECAR: &str = "khole-analyze";

/// Only the first this many seconds go to the sidecar. It holds the whole input in memory
/// (a one-hour mix would be ~600 MB); ten minutes is a full club track, and plenty for the
/// tempo and key of a longer recording.
pub const MAX_SECONDS: u64 = 600;

/// Output of `khole-analyze --version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarInfo {
    pub name: String,
    /// Analyzer version; cached analysis results are keyed on it.
    pub version: String,
    pub essentia: String,
}

pub fn parse_version_output(stdout: &[u8]) -> Result<SidecarInfo> {
    serde_json::from_slice(stdout)
        .map_err(|e| Error::Sidecar(format!("unexpected --version output: {e}")))
}

/// The sidecar as a plain `std::process::Command` (resolved next to the app binary by the
/// shell plugin), so its stdin can be streamed from a worker thread.
pub fn sidecar_command(app: &AppHandle) -> Result<Command> {
    let cmd = app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| Error::Sidecar(e.to_string()))?;
    Ok(cmd.into())
}

/// Runs `khole-analyze --version`. Blocking.
pub fn sidecar_info(mut cmd: Command) -> Result<SidecarInfo> {
    let output = cmd
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Sidecar(e.to_string()))?;
    if !output.status.success() {
        return Err(Error::Sidecar(format!(
            "exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_version_output(&output.stdout)
}

/// Decodes `path` and has the sidecar analyse it. Blocking: decoding and analysis overlap
/// (the sidecar reads while we decode), about 0.5 s per minute of audio. `cancelled` stops
/// the decode and kills the sidecar ([`Error::Cancelled`]).
pub fn analyze(mut cmd: Command, path: &Path, cancelled: impl Fn() -> bool) -> Result<Analysis> {
    let decoder = MonoDecoder::open(path)?;
    let rate = decoder.sample_rate();
    let limit = MAX_SECONDS * rate as u64;

    cmd.args(["--sr", &rate.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A spawn failure is ours (an Io error), not the file's: it is not cached.
    let mut child = cmd.spawn()?;
    lower_priority(child.id());

    let mut stdin = BufWriter::with_capacity(1 << 16, child.stdin.take().expect("piped"));
    let mut sent = 0u64;
    let mut bytes = Vec::new();
    let fed = decoder.run(&cancelled, |block| {
        let take = block.len().min((limit - sent) as usize);
        bytes.clear();
        bytes.extend(block[..take].iter().flat_map(|v| v.to_le_bytes()));
        sent += take as u64;
        match stdin.write_all(&bytes) {
            Ok(()) => {}
            // The sidecar quit early; its exit code and stderr tell why.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                return Ok(ControlFlow::Break(()));
            }
            Err(e) => return Err(e.into()),
        }
        Ok(if sent >= limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    });
    let flushed = stdin.flush();
    drop(stdin); // EOF: the sidecar starts analysing
    let flushed = flushed.or_else(|e| match e.kind() {
        std::io::ErrorKind::BrokenPipe => Ok(()),
        _ => Err(e.into()),
    });
    if let Err(e) = fed.and(flushed) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }

    // Poll, so a cancel does not wait for a long analysis to finish.
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout)?;
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    if !status.success() {
        let reason = match status.code() {
            Some(2) => "too short to analyse".to_owned(),
            code => format!("analyzer exited with {code:?}: {}", stderr.trim()),
        };
        return Err(Error::Sidecar(reason));
    }
    parse_result(&stdout)
}

pub fn parse_result(stdout: &[u8]) -> Result<Analysis> {
    serde_json::from_slice(stdout)
        .map_err(|e| Error::Sidecar(format!("unexpected analyzer output: {e}")))
}

/// Analysis is background work: the sidecar runs niced, so playback, waveforms and the UI
/// stay responsive while every core is busy.
fn lower_priority(pid: u32) {
    // SAFETY: plain syscall on our own child; failure only leaves the default priority.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, 10);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::testutil::{write_click_wav, write_wav};
    use std::path::PathBuf;

    #[test]
    fn parses_version_json() {
        let info = parse_version_output(
            br#"{"name":"khole-analyze","version":"0.1.0","essentia":"2.1-beta6-dev"}"#,
        )
        .unwrap();
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.essentia, "2.1-beta6-dev");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_version_output(b"usage: khole-analyze ...").is_err());
        assert!(parse_result(b"").is_err());
    }

    #[test]
    fn parses_result_json() {
        let a = parse_result(
            br#"{"bpm":124.00,"bpm_essentia":123.68,"bpm_confidence":3.76,"key":"A","scale":"minor","key_strength":0.85}"#,
        )
        .unwrap();
        assert_eq!(a.bpm, 124.0);
        assert_eq!((a.key.as_str(), a.scale.as_str()), ("A", "minor"));
    }

    /// The sidecar built by `sidecar/khole-analyze/build.sh` (CI builds it before the tests).
    pub(crate) fn built_sidecar() -> Option<Command> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries/khole-analyze-aarch64-apple-darwin");
        if path.is_file() {
            Some(Command::new(path))
        } else {
            eprintln!("sidecar not built, skipping: {}", path.display());
            None
        }
    }

    #[test]
    fn analyzes_a_click_track() {
        let Some(cmd) = built_sidecar() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("click.wav");
        write_click_wav(&path, 30.0, 124.0);
        let a = analyze(cmd, &path, || false).unwrap();
        assert!((a.bpm - 124.0).abs() < 0.1, "{a:?}");
        assert!(!a.key.is_empty());
    }

    #[test]
    fn too_short_and_cancelled() {
        let Some(cmd) = built_sidecar() else { return };
        let dir = tempfile::tempdir().unwrap();
        let short = dir.path().join("short.wav");
        write_wav(&short, 2.0, None);
        match analyze(cmd, &short, || false) {
            Err(Error::Sidecar(m)) => assert!(m.contains("too short"), "{m}"),
            other => panic!("{other:?}"),
        }
        let long = dir.path().join("long.wav");
        write_click_wav(&long, 30.0, 124.0);
        assert!(matches!(
            analyze(built_sidecar().unwrap(), &long, || true),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn version_of_the_built_sidecar() {
        let Some(cmd) = built_sidecar() else { return };
        assert_eq!(sidecar_info(cmd).unwrap().name, "khole-analyze");
    }
}
