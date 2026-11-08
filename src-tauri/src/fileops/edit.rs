//! Edits from the track list: a new file name, or a new title / artist tag. The only time
//! KHole changes what is inside a file, so it is done carefully:
//! - a rename never replaces another file (`rename_excl`; a taken name is an error);
//! - a tag is written into a clone of the file next to it (hidden), checked (the field has
//!   the new value, the audio length, the other fields and the pictures are unchanged), and
//!   only then swapped in for the original in one atomic `rename`;
//! - every edit goes into the undo log, ⌘Z writes the old name or value back.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::fs;
use crate::error::{Error, Result};
use crate::library::file_key::file_key;
use crate::library::tags::{self, TextField, TrackMeta};
use crate::library::watch;
use crate::store::actions::{self, Action, Kind, NewAction};
use crate::store::{self, Store, destinations, sessions, tracks, tracks::Track};

use super::Undone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Title,
    Artist,
    /// The file name without its extension.
    File,
}

/// Statuses whose file is in the source or a destination (not in the Trash, not gone).
const EDITABLE: &[&str] = &["pending", "kept", "copied", "moved"];

/// Applies an edit and returns the track row after it. An unchanged value does nothing.
pub fn edit(
    store: &Store,
    session_id: i64,
    track_id: i64,
    field: Field,
    value: &str,
) -> Result<Track> {
    let (track, allowed) = {
        let conn = store.conn()?;
        let session = sessions::get(&conn, session_id)?;
        let mut allowed = destinations::paths(&conn, session_id)?;
        allowed.push(session.source_path);
        (tracks::get(&conn, session_id, track_id)?, allowed)
    };
    if !EDITABLE.contains(&track.status.as_str()) {
        return Err(Error::Invalid(format!(
            "A {} track cannot be edited.",
            track.status
        )));
    }
    let path = PathBuf::from(&track.path);
    // Only the session's folders are touched.
    if !allowed.iter().any(|dir| path.starts_with(dir)) {
        return Err(Error::Invalid(format!(
            "{} is outside the session's folders.",
            track.path
        )));
    }
    if !path.is_file() {
        return Err(Error::NotFound(track.path.clone()));
    }
    let value = value.trim();
    match field {
        Field::File => rename(store, session_id, &track, &path, value),
        Field::Title => set_tag(store, session_id, &track, &path, TextField::Title, value),
        Field::Artist => set_tag(store, session_id, &track, &path, TextField::Artist, value),
    }
}

/// `stem` + the file's own extension, if `stem` is a usable file name.
fn new_file_name(path: &Path, stem: &str) -> Result<String> {
    if stem.is_empty() {
        return Err(Error::Invalid("The file name cannot be empty.".into()));
    }
    if stem.starts_with('.') {
        return Err(Error::Invalid(
            "A name starting with a dot would hide the file.".into(),
        ));
    }
    if stem.contains(['/', ':', '\0']) {
        return Err(Error::Invalid(
            "A file name cannot contain “/” or “:”.".into(),
        ));
    }
    let name = match path.extension() {
        Some(ext) => format!("{stem}.{}", ext.to_string_lossy()),
        None => stem.to_owned(),
    };
    if name.len() > 255 {
        return Err(Error::Invalid("The file name is too long.".into()));
    }
    Ok(name)
}

fn rename(store: &Store, session_id: i64, track: &Track, path: &Path, stem: &str) -> Result<Track> {
    let name = new_file_name(path, stem)?;
    let dir = path
        .parent()
        .ok_or_else(|| Error::Invalid(format!("bad path {}", track.path)))?;
    let old_name = path.file_name().unwrap_or_default().to_string_lossy();
    if name == old_name {
        return Ok(track.clone());
    }
    let dst = dir.join(&name);
    let op = watch::own_op();
    op.touched(path);
    op.touched(&dst);
    let result = if name.to_lowercase() == old_name.to_lowercase() {
        // Only the case changes: on a case-insensitive volume the "other" name is this
        // very file, so the exclusive rename would refuse.
        std::fs::rename(path, &dst)
    } else {
        fs::rename_excl(path, &dst)
    };
    match result {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(Error::Invalid(format!(
                "“{name}” already exists in this folder."
            )));
        }
        Err(e) => return Err(e.into()),
    }
    let dst_str = super::path_str(&dst)?;
    let mut conn = store.conn()?;
    let tx = conn.transaction()?;
    actions::record(
        &tx,
        &NewAction {
            session_id,
            track_id: track.id,
            kind: Kind::Rename,
            src: &track.path,
            dst: Some(dst_str),
            renamed: false,
            prev_status: &track.status,
            destination_id: None,
            duplicate: false,
            field: None,
            old_value: None,
            new_value: None,
        },
        store::now(),
    )?;
    tracks::set_location(&tx, track.id, dst_str, &track.status)?;
    let updated = tracks::get(&tx, session_id, track.id)?;
    tx.commit()?;
    Ok(updated)
}

fn field_name(field: TextField) -> &'static str {
    match field {
        TextField::Title => "title",
        TextField::Artist => "artist",
    }
}

fn set_tag(
    store: &Store,
    session_id: i64,
    track: &Track,
    path: &Path,
    field: TextField,
    value: &str,
) -> Result<Track> {
    let meta = tags::read(path);
    let old = field.get(&meta).map(str::to_owned);
    let new = (!value.is_empty()).then_some(value);
    if old.as_deref() == new {
        return Ok(track.clone());
    }
    let (key, after) = write_tag_safely(path, field, new, &meta)?;

    let mut conn = store.conn()?;
    let tx = conn.transaction()?;
    actions::record(
        &tx,
        &NewAction {
            session_id,
            track_id: track.id,
            kind: Kind::Tag,
            src: &track.path,
            dst: None,
            renamed: false,
            prev_status: &track.status,
            destination_id: None,
            duplicate: false,
            field: Some(field_name(field)),
            old_value: old.as_deref(),
            new_value: new,
        },
        store::now(),
    )?;
    tracks::refresh(&tx, track.id, &key, &after)?;
    let updated = tracks::get(&tx, session_id, track.id)?;
    tx.commit()?;
    Ok(updated)
}

/// Writes one tag field into a clone of `path`, checks the clone, then swaps it in for the
/// original. Returns the new content key and tags. On any failure the original is untouched.
fn write_tag_safely(
    path: &Path,
    field: TextField,
    value: Option<&str>,
    before: &TrackMeta,
) -> Result<(String, TrackMeta)> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Invalid(format!("bad path {}", path.display())))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    // Hidden, so neither the scan nor the watcher sees it.
    let tmp = dir.join(format!(".khole-edit-{}-{name}", std::process::id()));
    let op = watch::own_op();
    op.touched(path);
    op.touched(&tmp);

    fs::clone_file(path, &tmp)?;
    let checked = tags::write_text(&tmp, field, value, before).and_then(|()| {
        let after = tags::read(&tmp);
        check_edit(before, &after, field, value)?;
        if tags::picture_count(&tmp) != tags::picture_count(path) {
            return Err(Error::Invalid(
                "Writing the tag would lose the cover art – nothing was changed.".into(),
            ));
        }
        Ok(after)
    });
    let after = match checked {
        Ok(after) => after,
        Err(e) => {
            // Our own scratch copy, never the user's file.
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    std::fs::rename(&tmp, path)?;
    Ok((file_key(path)?, after))
}

/// The written file must read back with the new value and otherwise as before.
fn check_edit(
    before: &TrackMeta,
    after: &TrackMeta,
    field: TextField,
    value: Option<&str>,
) -> Result<()> {
    let other = match field {
        TextField::Title => TextField::Artist,
        TextField::Artist => TextField::Title,
    };
    let same_length = match (before.duration, after.duration) {
        (Some(a), Some(b)) => (a - b).abs() < 0.05,
        (a, b) => a.is_none() == b.is_none(),
    };
    let ok = field.get(after) == value
        && other.get(after) == other.get(before)
        && after.bpm == before.bpm
        && after.musical_key == before.musical_key
        && after.format == before.format
        && same_length;
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(
            "The tag did not read back as written – nothing was changed.".into(),
        ))
    }
}

/// Reverts a rename or a tag edit (the newest action of the session).
pub fn undo(store: &Store, session_id: i64, action: &Action) -> Result<Undone> {
    let track = tracks::get(&*store.conn()?, session_id, action.track_id)?;
    let mut note = None;
    let mut refreshed = None;
    let mut new_path = track.path.clone();

    match action.kind {
        Kind::Rename => {
            let from = PathBuf::from(action.dst.as_deref().unwrap_or(&track.path));
            let to = PathBuf::from(&action.src);
            let (dir, name) = match (to.parent(), to.file_name()) {
                (Some(d), Some(n)) => (d, n),
                _ => return Err(Error::Invalid(format!("bad path {}", action.src))),
            };
            let op = watch::own_op();
            op.touched(&from);
            match fs::move_to(&from, dir, name) {
                Ok(placed) => {
                    op.touched(&placed.path);
                    if placed.renamed {
                        note = Some(format!(
                            "The old name was taken; renamed to {}.",
                            placed
                                .path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ));
                    }
                    new_path = super::path_str(&placed.path)?.to_owned();
                }
                Err(Error::NotFound(_)) => {
                    note = Some(format!("{} is gone, nothing to rename.", from.display()));
                }
                Err(e) => return Err(e),
            }
        }
        Kind::Tag => {
            let field = match action.field.as_deref() {
                Some("artist") => TextField::Artist,
                _ => TextField::Title,
            };
            let path = PathBuf::from(&track.path);
            if path.is_file() {
                let before = tags::read(&path);
                if field.get(&before) != action.old_value.as_deref() {
                    refreshed = Some(write_tag_safely(
                        &path,
                        field,
                        action.old_value.as_deref(),
                        &before,
                    )?);
                }
            } else {
                note = Some(format!("{} is gone, nothing to change back.", track.path));
            }
        }
        _ => unreachable!("decisions are undone by fileops::undo_last"),
    }

    let mut conn = store.conn()?;
    let tx = conn.transaction()?;
    actions::mark_undone(&tx, action.id)?;
    if let Some((key, meta)) = &refreshed {
        tracks::refresh(&tx, track.id, key, meta)?;
    }
    tracks::set_location(&tx, track.id, &new_path, &track.status)?;
    let track = tracks::get(&tx, session_id, track.id)?;
    tx.commit()?;
    Ok(Undone {
        track,
        kind: action.kind.as_str(),
        destination_id: None,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fileops::{Decision, decide, undo_last};
    use crate::library::scan;
    use crate::library::testutil::{Tags, write_wav};

    struct Fixture {
        store: Store,
        session: i64,
        src: PathBuf,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().canonicalize().unwrap();
        write_wav(
            &src.join("a.wav"),
            0.5,
            Some(Tags {
                title: "Acid Line",
                artist: "Someone",
                bpm: Some("124"),
                key: Some("8A"),
            }),
        );
        write_wav(&src.join("b.wav"), 0.5, None);
        let store = Store::open_in_memory().unwrap();
        let session = sessions::open(&store.conn().unwrap(), src.to_str().unwrap(), 1)
            .unwrap()
            .id;
        scan::sync_session(&store, session, &src, false, &[], |_, _| {}).unwrap();
        Fixture {
            store,
            session,
            src,
            _dir: dir,
        }
    }

    impl Fixture {
        fn track(&self, name: &str) -> Track {
            tracks::list(&self.store.conn().unwrap(), self.session)
                .unwrap()
                .into_iter()
                .find(|t| t.path.ends_with(name))
                .unwrap()
        }
        fn edit(&self, id: i64, field: Field, value: &str) -> Result<Track> {
            edit(&self.store, self.session, id, field, value)
        }
        fn undo(&self) -> Undone {
            undo_last(&self.store, self.session).unwrap().unwrap()
        }
    }

    #[test]
    fn title_edit_writes_the_file_and_undo_restores_it() {
        let f = fixture();
        let a = f.track("a.wav");
        let t = f.edit(a.id, Field::Title, "  Acid Line (Edit)  ").unwrap();
        assert_eq!(t.title.as_deref(), Some("Acid Line (Edit)"));
        let m = tags::read(Path::new(&a.path));
        assert_eq!(m.title.as_deref(), Some("Acid Line (Edit)"));
        // Nothing else changed.
        assert_eq!(
            (m.artist.as_deref(), m.bpm, m.musical_key.as_deref()),
            (Some("Someone"), Some(124.0), Some("8A"))
        );
        // No scratch file left behind.
        let names: Vec<_> = std::fs::read_dir(&f.src)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.iter().any(|n| n.starts_with(".khole")), "{names:?}");

        let undone = f.undo();
        assert_eq!(undone.kind, "tag");
        assert_eq!(undone.track.title.as_deref(), Some("Acid Line"));
        assert_eq!(
            tags::read(Path::new(&a.path)).title.as_deref(),
            Some("Acid Line")
        );
    }

    #[test]
    fn artist_of_an_untagged_file_and_clearing_it() {
        let f = fixture();
        let b = f.track("b.wav");
        let t = f.edit(b.id, Field::Artist, "New Artist").unwrap();
        assert_eq!(
            (t.artist.as_deref(), t.title.as_deref()),
            (Some("New Artist"), None)
        );
        let t = f.edit(b.id, Field::Artist, "").unwrap();
        assert_eq!(t.artist, None);
        assert_eq!(f.undo().track.artist.as_deref(), Some("New Artist"));
        assert_eq!(f.undo().track.artist, None);
    }

    #[test]
    fn an_unchanged_value_records_nothing() {
        let f = fixture();
        let a = f.track("a.wav");
        f.edit(a.id, Field::Title, "Acid Line").unwrap();
        f.edit(a.id, Field::File, "a").unwrap();
        assert_eq!(undo_last(&f.store, f.session).unwrap(), None);
    }

    #[test]
    fn rename_keeps_the_extension_and_never_replaces() {
        let f = fixture();
        let a = f.track("a.wav");
        let t = f.edit(a.id, Field::File, "01 Acid Line").unwrap();
        assert!(t.path.ends_with("/01 Acid Line.wav"));
        assert!(Path::new(&t.path).is_file() && !Path::new(&a.path).exists());
        assert_eq!(t.id, a.id);

        // b.wav exists: refused, both files untouched.
        let err = f.edit(a.id, Field::File, "b").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert!(Path::new(&t.path).is_file());

        for bad in ["", ".hidden", "x/y", "x:y"] {
            assert!(
                matches!(f.edit(a.id, Field::File, bad), Err(Error::Invalid(_))),
                "{bad}"
            );
        }

        let undone = f.undo();
        assert_eq!(
            (undone.kind, undone.track.path.as_str()),
            ("rename", a.path.as_str())
        );
        assert!(Path::new(&a.path).is_file());
    }

    #[test]
    fn case_only_rename() {
        let f = fixture();
        let a = f.track("a.wav");
        let t = f.edit(a.id, Field::File, "A").unwrap();
        assert!(t.path.ends_with("/A.wav"));
        let names: Vec<_> = std::fs::read_dir(&f.src)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"A.wav".to_owned()), "{names:?}");
    }

    #[test]
    fn a_renamed_track_survives_a_rescan_and_can_still_be_decided() {
        let f = fixture();
        let a = f.track("a.wav");
        f.edit(a.id, Field::File, "renamed").unwrap();
        let s = scan::sync_session(&f.store, f.session, &f.src, false, &[], |_, _| {}).unwrap();
        assert_eq!((s.total, s.added, s.removed), (2, 0, 0));
        let out = decide(&f.store, f.session, a.id, Decision::Keep).unwrap();
        assert!(out.track.path.ends_with("/renamed.wav"));
    }

    #[test]
    fn trashed_or_missing_tracks_are_not_editable() {
        let f = fixture();
        let b = f.track("b.wav");
        f.store
            .conn()
            .unwrap()
            .execute("UPDATE tracks SET status = 'missing' WHERE id = ?1", [b.id])
            .unwrap();
        assert!(matches!(
            f.edit(b.id, Field::Title, "x"),
            Err(Error::Invalid(_))
        ));
    }

    /// A DJ's MP3: cover art and Serato's GEOB frames (cue points) must survive a title
    /// edit and its undo byte for byte; a FLAC keeps its picture block.
    /// `cargo test keeps_cover_art -- --ignored` (needs the spike fixtures)
    #[test]
    #[ignore]
    fn keeps_cover_art_and_serato_frames() {
        use lofty::config::WriteOptions;
        use lofty::flac::FlacFile;
        use lofty::id3::v2::{BinaryFrame, Frame, FrameId, Id3v2Tag};
        use lofty::mpeg::MpegFile;
        use lofty::ogg::OggPictureStorage;
        use lofty::ogg::tag::VorbisComments;
        use lofty::picture::{MimeType, Picture, PictureType};
        use lofty::prelude::*;

        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../spike/fixtures");
        let work = tempfile::tempdir().unwrap();
        let picture = || {
            Picture::unchecked(vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4])
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build()
        };
        let serato = vec![1u8, 1, b'S', b'e', b'r', b'a', b't', b'o', 0, 42, 43, 44];

        // MP3 with title, artist, picture and a GEOB frame.
        let mp3 = work.path().join("dj.mp3");
        std::fs::copy(fixtures.join("short.mp3"), &mp3).unwrap();
        let mut tag = Id3v2Tag::new();
        tag.set_title("Old".into());
        tag.set_artist("Artist".into());
        tag.insert_picture(picture());
        tag.insert(Frame::Binary(BinaryFrame::new(
            FrameId::new("GEOB").unwrap(),
            serato.clone(),
        )));
        tag.save_to_path(&mp3, WriteOptions::default()).unwrap();
        let geob = |p: &Path| -> Vec<Vec<u8>> {
            let mut f = std::fs::File::open(p).unwrap();
            let file = MpegFile::read_from(&mut f, lofty::config::ParseOptions::new()).unwrap();
            file.id3v2()
                .unwrap()
                .into_iter()
                .filter_map(|fr| match fr {
                    Frame::Binary(b) if b.id().as_str() == "GEOB" => Some(b.data.to_vec()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(geob(&mp3), std::slice::from_ref(&serato));

        let before = tags::read(&mp3);
        write_tag_safely(&mp3, TextField::Title, Some("New"), &before).unwrap();
        assert_eq!(tags::read(&mp3).title.as_deref(), Some("New"));
        assert_eq!(tags::read(&mp3).artist.as_deref(), Some("Artist"));
        assert_eq!(tags::picture_count(&mp3), Some(1));
        assert_eq!(geob(&mp3), std::slice::from_ref(&serato));

        // FLAC with a picture block.
        let flac = work.path().join("dj.flac");
        std::fs::copy(fixtures.join("short.flac"), &flac).unwrap();
        let mut f = std::fs::File::open(&flac).unwrap();
        let mut file = FlacFile::read_from(&mut f, lofty::config::ParseOptions::new()).unwrap();
        drop(f);
        let mut vc = VorbisComments::default();
        vc.set_artist("Artist".into());
        file.set_vorbis_comments(vc);
        file.insert_picture(picture(), None).unwrap();
        file.save_to_path(&flac, WriteOptions::default()).unwrap();
        assert_eq!(tags::picture_count(&flac), Some(1));

        let before = tags::read(&flac);
        write_tag_safely(&flac, TextField::Title, Some("New"), &before).unwrap();
        assert_eq!(tags::read(&flac).title.as_deref(), Some("New"));
        assert_eq!(tags::read(&flac).artist.as_deref(), Some("Artist"));
        assert_eq!(tags::picture_count(&flac), Some(1));
    }

    /// Real files in all formats (spike fixtures, not in git): the edit must keep the audio,
    /// the other tags and the pictures. `cargo test edit_spike_fixtures -- --ignored`
    #[test]
    #[ignore]
    fn edit_spike_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../spike/fixtures");
        let work = tempfile::tempdir().unwrap();
        for file in [
            "short.mp3",
            "short.wav",
            "short.aiff",
            "short.flac",
            "short_aac.m4a",
            "short_alac.m4a",
        ] {
            let p = work.path().join(file);
            std::fs::copy(dir.join(file), &p).unwrap();
            let before = tags::read(&p);
            let pics = tags::picture_count(&p);
            write_tag_safely(&p, TextField::Title, Some("Édited – Title"), &before).unwrap();
            let after = tags::read(&p);
            println!("{file}: {before:?}\n  -> {after:?}");
            assert_eq!(after.title.as_deref(), Some("Édited – Title"), "{file}");
            assert_eq!(after.artist, before.artist, "{file}");
            assert_eq!(tags::picture_count(&p), pics, "{file}");
        }
    }
}
