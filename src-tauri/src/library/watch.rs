//! Watches the open session's source folder (FSEvents through `notify`), so that a file
//! moved, renamed, deleted, added or put back in Finder shows up in the list without a
//! manual rescan.
//!
//! The watcher does not touch the DB: after a quiet moment it emits `folder_changed`, and
//! the UI runs the normal scan, which already reconciles everything (see `scan.rs`).
//! Changes the app makes itself (decisions, undo) are reported to [`own_op`] and ignored,
//! otherwise every keypress would trigger a rescan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::scan;
use crate::error::{Error, Result};
use crate::store::{Store, destinations, sessions};

/// Events are collected until the folder has been quiet this long…
const QUIET: Duration = Duration::from_millis(400);
/// …but a long copy still refreshes the list this often.
const MAX_WAIT: Duration = Duration::from_secs(2);
/// How long a path the app itself changed is ignored.
const OWN_TTL: Duration = Duration::from_secs(5);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderChanged {
    session_id: i64,
}

/// The one active watcher (the open session's). Dropping it stops the FSEvents stream; the
/// debounce thread ends when its channel closes.
#[derive(Default)]
pub struct Watcher(Mutex<Option<(i64, RecommendedWatcher)>>);

impl Watcher {
    /// Watches the source folder of `session_id`, replacing any previous watch.
    pub fn start(&self, app: &AppHandle, session_id: i64, root: PathBuf) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::StorePoisoned)?;
        if matches!(&*slot, Some((id, _)) if *id == session_id) {
            return Ok(());
        }
        *slot = None;
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(watch_error)?;
        // FSEvents is recursive anyway; `is_relevant` drops subfolders when they are off.
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(watch_error)?;
        let app = app.clone();
        std::thread::Builder::new()
            .name("khole-watch".into())
            .spawn(move || {
                debounce(rx, |paths, lost| {
                    flush(&app, session_id, &root, paths, lost)
                })
            })?;
        *slot = Some((session_id, watcher));
        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

fn watch_error(e: notify::Error) -> Error {
    Error::Invalid(format!("cannot watch the folder: {e}"))
}

/// Collects event paths until the stream is quiet (or `MAX_WAIT` passed), then hands them
/// to `flush`, with `true` if FSEvents lost details (anything may have changed). Waits
/// while the app's own file operation runs. Returns when the watcher is dropped.
fn debounce(
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    mut flush: impl FnMut(Vec<PathBuf>, bool),
) {
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut lost = false;
    let mut first: Option<Instant> = None;
    loop {
        let event = if first.is_none() {
            match rx.recv() {
                Ok(e) => Some(e),
                Err(_) => return,
            }
        } else {
            match rx.recv_timeout(QUIET) {
                Ok(e) => Some(e),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match event {
            Some(Ok(e)) => {
                lost |= e.need_rescan();
                pending.extend(e.paths);
                first.get_or_insert_with(Instant::now);
                if first.is_some_and(|t| t.elapsed() < MAX_WAIT) {
                    continue;
                }
            }
            Some(Err(_)) => {
                lost = true;
                first.get_or_insert_with(Instant::now);
                continue;
            }
            None => {}
        }
        if OWN.busy.load(Ordering::SeqCst) > 0 {
            continue; // our own move/copy is still running; its paths are not known yet
        }
        first = None;
        flush(std::mem::take(&mut pending), std::mem::take(&mut lost));
    }
}

fn flush(app: &AppHandle, session_id: i64, root: &Path, paths: Vec<PathBuf>, lost: bool) {
    let Some((include_sub, exclude)) = scope(app, session_id) else {
        return;
    };
    let changed = lost
        || paths
            .iter()
            .filter(|p| !is_own(p))
            .any(|p| is_relevant(p, root, include_sub, &exclude));
    if changed {
        let _ = app.emit("folder_changed", FolderChanged { session_id });
    }
}

/// The session's current scan scope, read at flush time so Setup changes apply at once.
fn scope(app: &AppHandle, session_id: i64) -> Option<(bool, Vec<PathBuf>)> {
    let store = app.state::<Store>();
    let conn = store.conn().ok()?;
    let session = sessions::get(&conn, session_id).ok()?;
    let exclude = destinations::paths(&conn, session_id)
        .ok()?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    Some((session.include_sub, exclude))
}

/// Whether a change at `path` can alter the track list of a scan of `root` (same rules as
/// [`scan::list_audio_files`]).
pub fn is_relevant(path: &Path, root: &Path, include_sub: bool, exclude: &[PathBuf]) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    if rel.as_os_str().is_empty() {
        // The folder itself: only its disappearance matters (renamed, deleted, unmounted).
        return !root.is_dir();
    }
    if rel
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        || exclude.iter().any(|x| path.starts_with(x))
    {
        return false;
    }
    let direct = rel.components().count() == 1;
    if scan::is_audio(path) {
        return include_sub || direct;
    }
    // A folder moved in or out may hold tracks. A path that is gone cannot be checked, so
    // anything without an extension counts as a folder.
    include_sub && (path.is_dir() || path.extension().is_none())
}

// ── The app's own changes ─────────────────────────────────────────────────────────────

struct Own {
    busy: AtomicUsize,
    paths: OnceLock<Mutex<HashMap<PathBuf, Instant>>>,
}

static OWN: Own = Own {
    busy: AtomicUsize::new(0),
    paths: OnceLock::new(),
};

fn own_paths() -> &'static Mutex<HashMap<PathBuf, Instant>> {
    OWN.paths.get_or_init(Default::default)
}

fn is_own(path: &Path) -> bool {
    own_paths()
        .lock()
        .is_ok_and(|m| m.get(path).is_some_and(|t| t.elapsed() < OWN_TTL))
}

/// Marks a file operation of the app in progress; the watcher holds its events back until
/// the guard is dropped. Every path the operation touched goes to [`OwnOp::touched`].
pub fn own_op() -> OwnOp {
    OWN.busy.fetch_add(1, Ordering::SeqCst);
    OwnOp
}

pub struct OwnOp;

impl OwnOp {
    pub fn touched(&self, path: impl AsRef<Path>) {
        if let Ok(mut m) = own_paths().lock() {
            m.retain(|_, t| t.elapsed() < OWN_TTL);
            m.insert(path.as_ref().to_path_buf(), Instant::now());
        }
    }
}

impl Drop for OwnOp {
    fn drop(&mut self) {
        OWN.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_follows_the_scan_rules() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("sub")).unwrap();
        std::fs::create_dir_all(r.join("Keep")).unwrap();
        let keep = vec![r.join("Keep")];
        let rel = |p: &str, sub: bool| is_relevant(&r.join(p), r, sub, &keep);

        assert!(rel("a.mp3", false));
        assert!(rel("A.AIFF", false));
        assert!(!rel("notes.txt", true));
        assert!(!rel(".DS_Store", true));
        assert!(!rel("._a.mp3", true));
        assert!(!rel(".git/a.mp3", true));
        // Subfolders only when they are scanned.
        assert!(!rel("sub/b.flac", false));
        assert!(rel("sub/b.flac", true));
        assert!(!rel("sub", false));
        assert!(rel("sub", true));
        assert!(rel("gone-folder", true));
        // Destinations inside the source are not scanned.
        assert!(!rel("Keep/a.mp3", true));
        assert!(!rel("Keep/a.mp3", false));
        // Outside the folder; the folder itself only when gone.
        assert!(!is_relevant(Path::new("/elsewhere/a.mp3"), r, true, &keep));
        assert!(!is_relevant(r, r, true, &keep));
        assert!(is_relevant(&r.join("x"), &r.join("x"), true, &[]));
    }

    #[test]
    fn own_paths_are_ignored_for_a_while() {
        let p = PathBuf::from("/tmp/khole-own-test/a.mp3");
        assert!(!is_own(&p));
        own_op().touched(&p);
        assert!(is_own(&p));
    }

    /// The real FSEvents stream: a file added in the folder arrives as a flush, a hidden one
    /// does not count.
    #[test]
    fn debounce_delivers_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();
        let (out_tx, out_rx) = mpsc::channel();
        let t = std::thread::spawn(move || {
            debounce(rx, |paths, _| {
                let _ = out_tx.send(paths);
            })
        });
        std::thread::sleep(Duration::from_millis(300)); // let the stream start
        std::fs::write(root.join("new.mp3"), b"x").unwrap();
        let paths = out_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            paths
                .iter()
                .any(|p| is_relevant(p, &root, false, &[]) && p.ends_with("new.mp3")),
            "{paths:?}"
        );
        drop(watcher);
        t.join().unwrap();
    }
}
