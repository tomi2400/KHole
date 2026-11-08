//! Lists the audio files of a source folder and syncs them into the session's `tracks` rows.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use super::{file_key::file_key, tags};
use crate::error::Result;
use crate::store::{self, Store, sessions, tracks};

/// Lower-case extensions that the WebView plays (see docs/spike-results.md).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "aif", "aiff", "flac", "m4a", "aac"];

/// Rows written per DB transaction, so the UI's queries are never blocked for long.
const BATCH: usize = 64;

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Audio files in `root` (and its subfolders if `include_sub`), sorted by path.
/// Hidden entries (including macOS `._` AppleDouble files) and symlinks are skipped;
/// unreadable subfolders are ignored rather than failing the scan. Folders in `exclude`
/// (destinations inside the source) are skipped too: what was sorted there is done.
pub fn list_audio_files(root: &Path, include_sub: bool, exclude: &[PathBuf]) -> Vec<PathBuf> {
    WalkDir::new(root)
        .max_depth(if include_sub { usize::MAX } else { 1 })
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || !(e.file_name().to_string_lossy().starts_with('.')
                    || e.file_type().is_dir() && exclude.iter().any(|x| x == e.path()))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio(e.path()))
        .map(|e| e.into_path())
        .collect()
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    /// Tracks listed for the session after the scan.
    pub total: usize,
    pub added: usize,
    pub removed: usize,
    /// Trashed or moved tracks whose file came back into the folder.
    pub returned: usize,
    /// Known tracks found under a new name or in another subfolder (renamed in Finder).
    pub renamed: usize,
    /// Decided tracks whose file disappeared from the folder (kept for the undo log).
    pub missing: usize,
    /// Audio files that could not be opened (permissions, non-UTF-8 name, …).
    pub unreadable: usize,
}

/// Brings the session's track list in line with the folder: new files are added, tags of
/// known files refreshed, and rows whose file is gone are dropped unless the undo log
/// still references them. Statuses (kept, moved, …) are preserved.
pub fn sync_session(
    store: &Store,
    session_id: i64,
    root: &Path,
    include_sub: bool,
    exclude: &[PathBuf],
    mut progress: impl FnMut(usize, usize),
) -> Result<ScanSummary> {
    let files = list_audio_files(root, include_sub, exclude);
    let total = files.len();
    progress(0, total);

    let known: HashMap<String, i64> = tracks::ids_by_path(&*store.conn()?, session_id)?;
    let mut seen = HashSet::with_capacity(total);
    let mut summary = ScanSummary::default();
    let mut batch = Vec::with_capacity(BATCH);

    for (i, path) in files.iter().enumerate() {
        let scanned = path.to_str().and_then(|p| {
            let key = file_key(path).ok()?;
            Some(tracks::NewTrack {
                path: p.to_owned(),
                file_key: key,
                meta: tags::read(path),
            })
        });
        match scanned {
            Some(t) => {
                if !known.contains_key(&t.path) {
                    summary.added += 1;
                }
                seen.insert(t.path.clone());
                batch.push(t);
            }
            None => summary.unreadable += 1,
        }
        if batch.len() == BATCH {
            tracks::upsert(&mut *store.conn()?, session_id, &batch)?;
            batch.clear();
        }
        progress(i + 1, total);
    }
    tracks::upsert(&mut *store.conn()?, session_id, &batch)?;

    let exists = |p: &str| Path::new(p).exists();
    let mut stale: Vec<i64> = known
        .iter()
        .filter(|(path, _)| !seen.contains(*path))
        .map(|(_, &id)| id)
        .collect();
    let mut conn = store.conn()?;
    if summary.added > 0 && !stale.is_empty() {
        let now = tracks::ids_by_path(&conn, session_id)?;
        let fresh: Vec<i64> = seen
            .iter()
            .filter(|p| !known.contains_key(*p))
            .filter_map(|p| now.get(p).copied())
            .collect();
        let merged = tracks::relink_renamed(&mut conn, &stale, &fresh, exists)?;
        summary.renamed = merged.len();
        stale.retain(|id| !merged.contains(id));
    }
    summary.removed = tracks::remove_unreferenced(&mut conn, &stale)?;
    summary.missing = tracks::mark_missing(&conn, &stale, exists)?;
    summary.returned = tracks::reattach_returned(&mut conn, session_id, exists)?;
    summary.added -= (summary.returned + summary.renamed).min(summary.added);
    summary.total = tracks::count(&conn, session_id)?;
    sessions::touch(&conn, session_id, store::now())?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::testutil::{Tags, write_wav};

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn lists_audio_only_sorted_and_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        for f in [
            "b.MP3",
            "a.wav",
            "c.aiff",
            "notes.txt",
            "cover.jpg",
            ".hidden.mp3",
            "._a.wav",
            "sub/d.flac",
            "sub/deeper/e.m4a",
            ".git/f.mp3",
        ] {
            touch(&r.join(f));
        }
        let names = |v: Vec<PathBuf>| -> Vec<String> {
            v.iter()
                .map(|p| p.strip_prefix(r).unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names(list_audio_files(r, false, &[])),
            ["a.wav", "b.MP3", "c.aiff"]
        );
        assert_eq!(
            names(list_audio_files(r, true, &[])),
            ["a.wav", "b.MP3", "c.aiff", "sub/d.flac", "sub/deeper/e.m4a"]
        );
        assert_eq!(
            names(list_audio_files(r, true, &[r.join("sub/deeper")])),
            ["a.wav", "b.MP3", "c.aiff", "sub/d.flac"]
        );
    }

    #[test]
    fn sync_adds_refreshes_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let tagged = |title| {
            Some(Tags {
                title,
                artist: "A",
                bpm: Some("128"),
                key: Some("8A"),
            })
        };
        write_wav(&r.join("one.wav"), 0.5, tagged("One"));
        write_wav(&r.join("two.wav"), 0.5, None);
        std::fs::create_dir(r.join("sub")).unwrap();
        write_wav(&r.join("sub/three.wav"), 0.5, None);

        let store = Store::open_in_memory().unwrap();
        let root = r.to_str().unwrap();
        let sid = sessions::open(&store.conn().unwrap(), root, 1).unwrap().id;

        let mut calls = Vec::new();
        let s = sync_session(&store, sid, r, false, &[], |d, t| calls.push((d, t))).unwrap();
        assert_eq!(
            s,
            ScanSummary {
                total: 2,
                added: 2,
                removed: 0,
                returned: 0,
                renamed: 0,
                missing: 0,
                unreadable: 0
            }
        );
        assert_eq!(calls, [(0, 2), (1, 2), (2, 2)]);

        let list = tracks::list(&store.conn().unwrap(), sid).unwrap();
        assert_eq!(list[0].title.as_deref(), Some("One"));
        assert_eq!(list[0].bpm, Some(128.0));
        assert_eq!(list[0].musical_key.as_deref(), Some("8A"));
        assert_eq!(list[1].title, None);

        // A handled track keeps its status across rescans.
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE tracks SET status = 'kept' WHERE id = ?1",
                [list[0].id],
            )
            .unwrap();

        // Subfolders on: one added. Then a file disappears: its row goes away.
        let s = sync_session(&store, sid, r, true, &[], |_, _| {}).unwrap();
        assert_eq!((s.total, s.added, s.removed), (3, 1, 0));
        std::fs::remove_file(r.join("two.wav")).unwrap();
        let s = sync_session(&store, sid, r, true, &[], |_, _| {}).unwrap();
        assert_eq!((s.total, s.added, s.removed), (2, 0, 1));

        let list = tracks::list(&store.conn().unwrap(), sid).unwrap();
        assert_eq!(list[0].status, "kept");
        assert_eq!(list[1].status, "pending");
    }

    #[test]
    fn rows_referenced_by_the_undo_log_survive() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_wav(&r.join("a.wav"), 0.2, None);
        let store = Store::open_in_memory().unwrap();
        let sid = sessions::open(&store.conn().unwrap(), r.to_str().unwrap(), 1)
            .unwrap()
            .id;
        sync_session(&store, sid, r, false, &[], |_, _| {}).unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO actions (session_id, track_id, kind, src, dst, ts)
                 VALUES (?1, 1, 'move', 'a', 'b', 0)",
                [sid],
            )
            .unwrap();
        std::fs::remove_file(r.join("a.wav")).unwrap();
        let s = sync_session(&store, sid, r, false, &[], |_, _| {}).unwrap();
        assert_eq!((s.total, s.removed), (1, 0));
    }

    /// Scans a real folder and prints the rows and the time taken:
    /// `KHOLE_SCAN_DIR=~/Music/x cargo test --release scan_dir_from_env -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scan_dir_from_env() {
        let Ok(dir) = std::env::var("KHOLE_SCAN_DIR") else {
            return;
        };
        let store = Store::open_in_memory().unwrap();
        let sid = sessions::open(&store.conn().unwrap(), &dir, 1).unwrap().id;
        let t = std::time::Instant::now();
        let s = sync_session(&store, sid, Path::new(&dir), true, &[], |_, _| {}).unwrap();
        println!("{s:?} in {:?}", t.elapsed());
        for t in tracks::list(&store.conn().unwrap(), sid).unwrap() {
            println!(
                "{:6} {:>4} {:>7?} {:>5?} {:?} – {:?} [{}]",
                t.format.unwrap_or_default(),
                t.bitrate.unwrap_or_default(),
                t.bpm,
                t.musical_key,
                t.artist,
                t.title,
                t.path.rsplit('/').next().unwrap()
            );
        }
    }
}
