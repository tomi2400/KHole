//! BPM / key analysis through the `khole-analyze` sidecar (Essentia).
//!
//! The sidecar never decodes audio itself: Rust decodes to mono f32le PCM and pipes it to
//! stdin (M7). For now this module only knows how to identify the sidecar.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::error::{Error, Result};

/// Sidecar name as registered in `tauri.conf.json` → `bundle.externalBin`.
pub const SIDECAR: &str = "khole-analyze";

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

pub async fn sidecar_info(app: &AppHandle) -> Result<SidecarInfo> {
    let output = app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| Error::Sidecar(e.to_string()))?
        .args(["--version"])
        .output()
        .await
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
