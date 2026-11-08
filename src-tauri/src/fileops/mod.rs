//! Decisions on tracks (keep / trash / move / copy) and their undo, with the
//! non-negotiable safety rules:
//! - never delete: trash goes to the macOS Trash, undoing a copy trashes the copy;
//! - never overwrite: a taken name becomes `name (2).ext` (see [`fs`]);
//! - every action is written to the undo log (`actions`) together with the track's new place;
//! - only the session's source folder and its destinations are touched.
//!
//! File operations run without holding the DB lock (a copy to another drive can take
//! seconds); the log entry is written right after the file is in place.

pub mod fs;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::library::file_key::file_key;
use crate::library::watch;
use crate::store::actions::{self, Kind, NewAction};
use crate::store::{self, Store, destinations, sessions, tracks, tracks::Track};

/// What the user decided for a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Decision {
    Keep,
    Trash,
    /// Move or copy (the session's mode) into a destination.
    Sort {
        destination_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The track row after the action (new path and status).
    pub track: Track,
    pub kind: &'static str,
    pub destination_id: Option<i64>,
    /// The name was taken in the destination, so the file got a counter: this is its name.
    pub renamed_to: Option<String>,
    /// The destination already had this very track (same name and content): nothing was
    /// copied, and on a move the source went to the Trash. This is the file already there.
    pub already_there: Option<String>,
}

/// Statuses whose file is still in the source folder, so a decision can be made (again).
const DECIDABLE: &[&str] = &["pending", "kept", "copied"];

pub fn decide(
    store: &Store,
    session_id: i64,
    track_id: i64,
    decision: Decision,
) -> Result<Outcome> {
    let (session, track, destination) = {
        let conn = store.conn()?;
        let session = sessions::get(&conn, session_id)?;
        let track = tracks::get(&conn, session_id, track_id)?;
        let destination = match decision {
            Decision::Sort { destination_id } => {
                Some(destinations::get(&conn, session_id, destination_id)?)
            }
            _ => None,
        };
        (session, track, destination)
    };
    if !DECIDABLE.contains(&track.status.as_str()) {
        return Err(Error::Invalid(format!(
            "This track is already {}.",
            track.status
        )));
    }
    let src = PathBuf::from(&track.path);
    if !src.starts_with(&session.source_path) {
        return Err(Error::Invalid(format!(
            "{} is outside the source folder.",
            track.path
        )));
    }
    if !src.is_file() {
        return Err(Error::NotFound(track.path.clone()));
    }

    // The watcher ignores what we do here (until `op` drops, and the paths touched).
    let op = watch::own_op();
    op.touched(&src);
    let mut already_there = None;
    let (kind, dst, renamed) = match (decision, &destination) {
        (Decision::Keep, _) => (Kind::Keep, None, false),
        (Decision::Trash, _) => (Kind::Trash, Some(fs::trash(&src)?), false),
        (Decision::Sort { .. }, Some(d)) => {
            let dir = Path::new(&d.path);
            if !dir.is_dir() {
                return Err(Error::NotFound(d.path.clone()));
            }
            let copy = session.mode == "copy";
            if let Some(existing) = same_track_in(dir, &src) {
                // Never a second copy of the same track: a copy is skipped, a move sends the
                // source to the Trash (it is in the destination already; undo brings it back).
                let dst = if copy {
                    existing.clone()
                } else {
                    fs::trash(&src)?
                };
                already_there = Some(existing);
                (if copy { Kind::Copy } else { Kind::Move }, Some(dst), false)
            } else {
                let placed = if copy {
                    fs::copy_into(&src, dir)?
                } else {
                    fs::move_into(&src, dir)?
                };
                let kind = if copy { Kind::Copy } else { Kind::Move };
                (kind, Some(placed.path), placed.renamed)
            }
        }
        (Decision::Sort { .. }, None) => unreachable!("destination was looked up above"),
    };
    if let Some(dst) = &dst {
        op.touched(dst);
    }
    let dst_str = dst.as_deref().map(path_str).transpose()?;

    // Where the track row points now: a moved or trashed file is at `dst`, a copied or kept
    // one stays where it was.
    let new_path = match kind {
        Kind::Move | Kind::Trash => dst_str.unwrap_or(&track.path),
        Kind::Copy | Kind::Keep => &track.path,
    };
    let mut conn = store.conn()?;
    let tx = conn.transaction()?;
    actions::record(
        &tx,
        &NewAction {
            session_id,
            track_id,
            kind,
            src: &track.path,
            dst: dst_str,
            renamed,
            prev_status: &track.status,
            destination_id: destination.as_ref().map(|d| d.id),
            duplicate: already_there.is_some(),
        },
        store::now(),
    )?;
    tracks::set_location(&tx, track_id, new_path, kind.status())?;
    let updated = tracks::get(&tx, session_id, track_id)?;
    tx.commit()?;

    Ok(Outcome {
        track: updated,
        kind: kind.as_str(),
        destination_id: destination.map(|d| d.id),
        renamed_to: renamed
            .then(|| dst.as_deref().and_then(Path::file_name))
            .flatten()
            .map(|n| n.to_string_lossy().into_owned()),
        already_there: already_there.map(|p| p.to_string_lossy().into_owned()),
    })
}

/// The file of the same name in `dir`, if it is the same track as `src` (size and hash of
/// head and tail, as `file_key`). Other names are not searched (duplicate finder: spec #19).
fn same_track_in(dir: &Path, src: &Path) -> Option<PathBuf> {
    let there = dir.join(src.file_name()?);
    if !there.is_file() {
        return None;
    }
    (file_key(&there).ok()? == file_key(src).ok()?).then_some(there)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Undone {
    pub track: Track,
    pub kind: &'static str,
    pub destination_id: Option<i64>,
    /// Something did not go back exactly (the original name was taken, the file had been
    /// moved away meanwhile, …).
    pub note: Option<String>,
}

/// Reverts the session's newest action that is not undone yet; `None` if there is nothing
/// left to undo.
pub fn undo_last(store: &Store, session_id: i64) -> Result<Option<Undone>> {
    let Some(action) = actions::last_undoable(&*store.conn()?, session_id)? else {
        return Ok(None);
    };
    let src = PathBuf::from(&action.src);
    let mut note = None;
    let mut status = action.prev_status.clone();
    let op = watch::own_op();
    if let Some(dst) = &action.dst {
        op.touched(dst);
    }

    let new_path = match (action.kind, action.dst.as_deref()) {
        (Kind::Keep, _) => action.src.clone(),
        (Kind::Move | Kind::Trash, Some(dst)) => {
            let dst = Path::new(dst);
            let (dir, name) = match (src.parent(), src.file_name()) {
                (Some(d), Some(n)) => (d, n),
                _ => return Err(Error::Invalid(format!("bad path {}", action.src))),
            };
            if !dir.is_dir() {
                return Err(Error::NotFound(dir.display().to_string()));
            }
            match fs::move_to(dst, dir, name) {
                Ok(placed) => {
                    if placed.renamed {
                        note = Some(format!(
                            "The original name was taken; restored as {}.",
                            placed
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ));
                    }
                    path_str(&placed.path)?.to_owned()
                }
                Err(Error::NotFound(_)) if is_back(store, action.track_id, &src) => {
                    note = Some("It was already back in place.".into());
                    action.src.clone()
                }
                Err(Error::NotFound(_)) => {
                    note = Some(format!("{} is gone, nothing to put back.", dst.display()));
                    status = "missing".into();
                    path_str(dst)?.to_owned()
                }
                Err(e) => return Err(e),
            }
        }
        (Kind::Copy, Some(_)) if action.duplicate => {
            // Nothing was copied: the file in the destination was there before.
            action.src.clone()
        }
        (Kind::Copy, Some(dst)) => {
            // Never delete: the copy goes to the Trash.
            match fs::trash(Path::new(dst)) {
                Ok(_) | Err(Error::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
            action.src.clone()
        }
        (_, None) => {
            return Err(Error::Invalid(format!(
                "action {} has no target",
                action.id
            )));
        }
    };

    op.touched(&new_path);
    let mut conn = store.conn()?;
    let tx = conn.transaction()?;
    actions::mark_undone(&tx, action.id)?;
    tracks::set_location(&tx, action.track_id, &new_path, &status)?;
    let track = tracks::get(&tx, session_id, action.track_id)?;
    tx.commit()?;
    Ok(Some(Undone {
        track,
        kind: action.kind.as_str(),
        destination_id: action.destination_id,
        note,
    }))
}

/// The track's file is at its original place again (put back by hand).
fn is_back(store: &Store, track_id: i64, src: &Path) -> bool {
    let Ok(key) = store.conn().and_then(|c| tracks::file_key(&c, track_id)) else {
        return false;
    };
    file_key(src).is_ok_and(|k| k == key)
}

fn path_str(p: &Path) -> Result<&str> {
    p.to_str()
        .ok_or_else(|| Error::Invalid(format!("not a UTF-8 path: {}", p.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::scan;

    struct Fixture {
        store: Store,
        session: i64,
        src: tempfile::TempDir,
        dst: tempfile::TempDir,
        dest_id: i64,
    }

    /// A source folder with `a.wav`, `b.wav` scanned into a session, and one destination.
    fn fixture() -> Fixture {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        for n in ["a.wav", "b.wav"] {
            std::fs::write(src.path().join(n), n).unwrap();
        }
        let store = Store::open_in_memory().unwrap();
        let src_path = src.path().canonicalize().unwrap();
        let dst_path = dst.path().canonicalize().unwrap();
        let (session, dest_id) = {
            let conn = store.conn().unwrap();
            let s = sessions::open(&conn, src_path.to_str().unwrap(), 1)
                .unwrap()
                .id;
            let d = destinations::add(&conn, s, dst_path.to_str().unwrap())
                .unwrap()
                .id;
            (s, d)
        };
        scan::sync_session(&store, session, &src_path, false, &[], |_, _| {}).unwrap();
        Fixture {
            store,
            session,
            src,
            dst,
            dest_id,
        }
    }

    impl Fixture {
        fn track(&self, name: &str) -> Track {
            let conn = self.store.conn().unwrap();
            tracks::list(&conn, self.session)
                .unwrap()
                .into_iter()
                .find(|t| t.path.ends_with(name))
                .unwrap()
        }
        fn sort(&self, id: i64) -> Result<Outcome> {
            decide(
                &self.store,
                self.session,
                id,
                Decision::Sort {
                    destination_id: self.dest_id,
                },
            )
        }
        fn set_mode(&self, mode: &str) {
            sessions::set_mode(&self.store.conn().unwrap(), self.session, mode, 2).unwrap();
        }
        fn count(&self) -> i64 {
            destinations::get(&self.store.conn().unwrap(), self.session, self.dest_id)
                .unwrap()
                .count
        }
    }

    #[test]
    fn move_then_undo() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        assert_eq!((out.kind, out.track.status.as_str()), ("move", "moved"));
        assert!(Path::new(&out.track.path).starts_with(f.dst.path().canonicalize().unwrap()));
        assert!(!Path::new(&a.path).exists());
        assert_eq!(out.renamed_to, None);
        assert_eq!(f.count(), 1);

        // A moved track cannot be decided again; its file is no longer in the source.
        assert!(matches!(f.sort(a.id), Err(Error::Invalid(_))));

        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(undone.track.path, a.path);
        assert_eq!(undone.track.status, "pending");
        assert_eq!(undone.note, None);
        assert_eq!(std::fs::read_to_string(&a.path).unwrap(), "a.wav");
        assert_eq!(f.count(), 0);
        assert_eq!(undo_last(&f.store, f.session).unwrap(), None);
    }

    #[test]
    fn list_keeps_folder_order_after_a_move() {
        let f = fixture();
        f.sort(f.track("a.wav").id).unwrap();
        let conn = f.store.conn().unwrap();
        let names: Vec<_> = tracks::list(&conn, f.session)
            .unwrap()
            .into_iter()
            .map(|t| (t.path.ends_with("a.wav"), t.status))
            .collect();
        assert_eq!(names, [(true, "moved".into()), (false, "pending".into())]);
    }

    fn rescan(f: &Fixture) -> scan::ScanSummary {
        let src = f.src.path().canonicalize().unwrap();
        scan::sync_session(&f.store, f.session, &src, false, &[], |_, _| {}).unwrap()
    }

    #[test]
    fn a_track_renamed_in_finder_keeps_its_row_and_status() {
        let f = fixture();
        let a = f.track("a.wav");
        decide(&f.store, f.session, a.id, Decision::Keep).unwrap();
        let b = f.track("b.wav");
        sessions::set_current_track(&f.store.conn().unwrap(), f.session, Some(b.id)).unwrap();
        let src = f.src.path().canonicalize().unwrap();
        std::fs::rename(&a.path, src.join("a renamed.wav")).unwrap();
        std::fs::rename(&b.path, src.join("b renamed.wav")).unwrap();

        let s = rescan(&f);
        assert_eq!((s.total, s.added, s.removed, s.renamed), (2, 0, 0, 2));
        let a2 = f.track("a renamed.wav");
        assert_eq!((a2.id, a2.status.as_str()), (a.id, "kept"));
        // The current track (unreferenced, pending) is the same row too, so the player
        // does not lose it.
        let b2 = f.track("b renamed.wav");
        assert_eq!((b2.id, b2.status.as_str()), (b.id, "pending"));
        let conn = f.store.conn().unwrap();
        assert_eq!(
            sessions::get(&conn, f.session).unwrap().current_track,
            Some(b.id)
        );
        drop(conn);
        // Its undo still works on the new path.
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(undone.track.status, "pending");
    }

    #[test]
    fn a_copy_next_to_the_original_is_a_new_track() {
        let f = fixture();
        let a = f.track("a.wav");
        let src = f.src.path().canonicalize().unwrap();
        std::fs::copy(&a.path, src.join("a copy.wav")).unwrap();
        let s = rescan(&f);
        assert_eq!((s.total, s.added, s.renamed), (3, 1, 0));
    }

    #[test]
    fn a_decided_track_deleted_in_finder_becomes_missing_and_comes_back() {
        let f = fixture();
        let a = f.track("a.wav");
        decide(&f.store, f.session, a.id, Decision::Keep).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let away = elsewhere.path().join("a.wav");
        std::fs::rename(&a.path, &away).unwrap();
        let s = rescan(&f);
        assert_eq!((s.total, s.removed, s.missing), (2, 0, 1));
        assert_eq!(f.track("a.wav").status, "missing");
        // An undecided one simply leaves the list.
        std::fs::remove_file(f.track("b.wav").path).unwrap();
        let s = rescan(&f);
        assert_eq!((s.total, s.removed), (1, 1));

        std::fs::rename(&away, &a.path).unwrap();
        rescan(&f);
        let back = f.track("a.wav");
        assert_eq!((back.id, back.status.as_str()), (a.id, "pending"));
    }

    #[test]
    fn copy_of_a_track_already_in_the_destination_is_skipped() {
        let f = fixture();
        f.set_mode("copy");
        std::fs::write(f.dst.path().join("a.wav"), "a.wav").unwrap(); // same content
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        assert_eq!(out.track.status, "copied");
        assert!(out.already_there.unwrap().ends_with("a.wav"));
        assert_eq!(out.renamed_to, None);
        assert!(!f.dst.path().join("a (2).wav").exists());
        assert_eq!(f.count(), 1);
        // Undo must not trash the file that was there before.
        undo_last(&f.store, f.session).unwrap().unwrap();
        assert!(f.dst.path().join("a.wav").exists());
        assert_eq!(f.track("a.wav").status, "pending");
    }

    #[test]
    fn same_name_with_other_content_is_not_a_duplicate() {
        let f = fixture();
        f.set_mode("copy");
        std::fs::write(f.dst.path().join("a.wav"), "different").unwrap();
        let out = f.sort(f.track("a.wav").id).unwrap();
        assert_eq!(out.already_there, None);
        assert_eq!(out.renamed_to.as_deref(), Some("a (2).wav"));
    }

    #[test]
    fn a_file_put_back_by_hand_is_the_same_track_again() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        std::fs::rename(&out.track.path, &a.path).unwrap(); // dragged back in Finder
        let summary = rescan(&f);
        assert_eq!((summary.returned, summary.added, summary.total), (1, 0, 2));
        let back = f.track("a.wav");
        assert_eq!((back.id, back.status.as_str()), (a.id, "pending"));
        // Nothing left to undo, and it can be decided again.
        assert_eq!(undo_last(&f.store, f.session).unwrap(), None);
        decide(&f.store, f.session, a.id, Decision::Keep).unwrap();
    }

    #[test]
    fn a_returned_track_already_decided_again_keeps_its_new_history() {
        // As it happened before the fix: moved, put back by hand, undo pressed (the original
        // row became "missing"), the scan added the file as a new row, and that row was
        // decided. The next scan makes them one track again, with the new row's history.
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        std::fs::rename(&out.track.path, &a.path).unwrap();
        let fresh = {
            let conn = f.store.conn().unwrap();
            conn.execute_batch(&format!(
                "UPDATE tracks SET status = 'missing' WHERE id = {id};
                 UPDATE actions SET undone = 1 WHERE track_id = {id};",
                id = a.id
            ))
            .unwrap();
            conn.query_row(
                "INSERT INTO tracks (session_id, path, file_key)
                 SELECT session_id, ?2, file_key FROM tracks WHERE id = ?1 RETURNING id",
                rusqlite::params![a.id, a.path],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
        };
        decide(&f.store, f.session, fresh, Decision::Keep).unwrap();

        assert_eq!(rescan(&f).returned, 1);
        let rows: Vec<_> = tracks::list(&f.store.conn().unwrap(), f.session)
            .unwrap()
            .into_iter()
            .filter(|t| t.path.ends_with("a.wav"))
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].id, rows[0].path.as_str(), rows[0].status.as_str()),
            (a.id, a.path.as_str(), "kept")
        );
        // The keep (made on the new row) is now this track's, and undoes as usual.
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(
            (undone.track.id, undone.track.status.as_str()),
            (a.id, "pending")
        );
        assert_eq!(undo_last(&f.store, f.session).unwrap(), None);
    }

    #[test]
    fn undo_of_a_file_already_put_back_keeps_it() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        std::fs::rename(&out.track.path, &a.path).unwrap();
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(
            (undone.track.path.as_str(), undone.track.status.as_str()),
            (a.path.as_str(), "pending")
        );
        assert!(undone.note.is_some());
    }

    #[test]
    fn a_file_copied_back_while_still_in_the_destination_stays_separate() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        std::fs::copy(&out.track.path, &a.path).unwrap();
        let summary = rescan(&f);
        assert_eq!((summary.returned, summary.added), (0, 1));
    }

    #[test]
    fn move_into_a_taken_name_gets_a_counter() {
        let f = fixture();
        std::fs::write(f.dst.path().join("a.wav"), "theirs").unwrap();
        let out = f.sort(f.track("a.wav").id).unwrap();
        assert_eq!(out.renamed_to.as_deref(), Some("a (2).wav"));
        assert_eq!(
            std::fs::read_to_string(f.dst.path().join("a.wav")).unwrap(),
            "theirs"
        );
    }

    #[test]
    fn copy_keeps_the_original_and_the_track_stays_decidable() {
        let f = fixture();
        f.set_mode("copy");
        let b = f.track("b.wav");
        let out = f.sort(b.id).unwrap();
        assert_eq!((out.kind, out.track.status.as_str()), ("copy", "copied"));
        assert_eq!(out.track.path, b.path);
        assert!(Path::new(&b.path).exists());
        assert!(f.dst.path().join("b.wav").exists());
        // A copied track's file is still in the source: it can be decided again, and undo
        // returns it to "copied".
        decide(&f.store, f.session, b.id, Decision::Keep).unwrap();
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(undone.track.status, "copied");
    }

    #[test]
    fn keep_then_undo() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = decide(&f.store, f.session, a.id, Decision::Keep).unwrap();
        assert_eq!(
            (out.track.status.as_str(), out.track.path.as_str()),
            ("kept", a.path.as_str())
        );
        undo_last(&f.store, f.session).unwrap();
        assert_eq!(f.track("a.wav").status, "pending");
    }

    #[test]
    fn undo_of_a_file_moved_away_meanwhile_marks_it_missing() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::rename(&out.track.path, elsewhere.path().join("a.wav")).unwrap();
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(undone.track.status, "missing");
        assert!(undone.note.is_some());
        // The log moved past it.
        assert_eq!(undo_last(&f.store, f.session).unwrap(), None);
    }

    #[test]
    fn undo_restore_never_overwrites() {
        let f = fixture();
        let a = f.track("a.wav");
        f.sort(a.id).unwrap();
        std::fs::write(&a.path, "new file with the old name").unwrap();
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert!(undone.track.path.ends_with("a (2).wav"));
        assert!(undone.note.is_some());
        assert_eq!(
            std::fs::read_to_string(&a.path).unwrap(),
            "new file with the old name"
        );
    }

    #[test]
    fn refuses_tracks_and_destinations_of_other_sessions() {
        let f = fixture();
        let other = sessions::open(&f.store.conn().unwrap(), "/other", 1)
            .unwrap()
            .id;
        let a = f.track("a.wav");
        assert!(decide(&f.store, other, a.id, Decision::Keep).is_err());
        let foreign_dest = destinations::add(&f.store.conn().unwrap(), other, "/x")
            .unwrap()
            .id;
        let err = decide(
            &f.store,
            f.session,
            a.id,
            Decision::Sort {
                destination_id: foreign_dest,
            },
        );
        assert!(matches!(err, Err(Error::NotFound(_))));
        assert!(Path::new(&a.path).exists());
        let _ = &f.src;
    }

    /// Touches the real Trash and restores from it: `cargo test trash_then_undo -- --ignored`.
    #[test]
    #[ignore]
    fn trash_then_undo() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = decide(&f.store, f.session, a.id, Decision::Trash).unwrap();
        assert_eq!(out.track.status, "trashed");
        assert!(out.track.path.contains("/.Trash/"), "{}", out.track.path);
        assert!(!Path::new(&a.path).exists());
        let undone = undo_last(&f.store, f.session).unwrap().unwrap();
        assert_eq!(
            (undone.track.path, undone.track.status),
            (a.path.clone(), "pending".into())
        );
        assert!(Path::new(&a.path).exists());
    }

    /// Real Trash: `cargo test put_back_from_trash -- --ignored`.
    #[test]
    #[ignore]
    fn put_back_from_trash() {
        let f = fixture();
        let a = f.track("a.wav");
        let out = decide(&f.store, f.session, a.id, Decision::Trash).unwrap();
        std::fs::rename(&out.track.path, &a.path).unwrap(); // Finder: Put Back
        assert_eq!(rescan(&f).returned, 1);
        assert_eq!(f.track("a.wav").status, "pending");
    }

    /// Real Trash: `cargo test duplicate_move -- --ignored`.
    #[test]
    #[ignore]
    fn duplicate_move_trashes_the_source_and_undo_restores_it() {
        let f = fixture();
        std::fs::write(f.dst.path().join("a.wav"), "a.wav").unwrap();
        let a = f.track("a.wav");
        let out = f.sort(a.id).unwrap();
        assert!(out.already_there.is_some());
        assert_eq!(out.track.status, "moved");
        assert!(!Path::new(&a.path).exists());
        assert!(!f.dst.path().join("a (2).wav").exists());
        undo_last(&f.store, f.session).unwrap().unwrap();
        assert!(Path::new(&a.path).exists());
        assert!(f.dst.path().join("a.wav").exists());
    }

    #[test]
    fn decision_json() {
        let d: Decision = serde_json::from_str(r#"{"kind":"sort","destinationId":3}"#).unwrap();
        assert_eq!(d, Decision::Sort { destination_id: 3 });
        let d: Decision = serde_json::from_str(r#"{"kind":"trash"}"#).unwrap();
        assert_eq!(d, Decision::Trash);
    }
}
