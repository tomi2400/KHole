//! Tag and audio-property reading with lofty, and the one thing KHole writes: the title or
//! artist of a track (edited in the list).
//!
//! Writing goes through the concrete tag of each format (ID3v2, Vorbis comments, MP4 ilst),
//! never lofty's generic `Tag`: converting to it and back drops frames it does not model,
//! like the GEOB frames where Serato keeps cue points. Only the one field changes.

use std::fs::File;
use std::path::Path;

use lofty::aac::AacFile;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::FileType;
use lofty::flac::FlacFile;
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::Mp4File;
use lofty::mpeg::MpegFile;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub bpm: Option<f64>,
    pub musical_key: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    pub format: String,
    /// kbps.
    pub bitrate: Option<u32>,
}

/// Reads tags and properties. Unreadable files still get a format from their extension,
/// so they show up in the list (they may still play in the WebView).
pub fn read(path: &Path) -> TrackMeta {
    read_tags(path).unwrap_or_else(|_| TrackMeta {
        format: format_from_extension(path),
        ..TrackMeta::default()
    })
}

fn read_tags(path: &Path) -> Result<TrackMeta, lofty::error::FileParseError> {
    let file = Probe::open(path)?
        .options(ParseOptions::new().read_cover_art(false))
        .read()?;
    let props = file.properties();
    let duration = props.duration().as_secs_f64();
    let tag = file.primary_tag().or_else(|| file.first_tag());

    Ok(TrackMeta {
        title: tag.and_then(|t| clean(t.title().as_deref())),
        artist: tag.and_then(|t| clean(t.artist().as_deref())),
        bpm: tag.and_then(bpm),
        musical_key: tag.and_then(|t| clean(t.get_string(ItemKey::InitialKey))),
        duration: (duration > 0.0).then_some(duration),
        format: format_name(file.file_type(), props.bit_depth().is_some())
            .unwrap_or_else(|| format_from_extension(path)),
        bitrate: props.audio_bitrate().filter(|&b| b > 0),
    })
}

/// A tag field the user can edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    Title,
    Artist,
}

impl TextField {
    pub fn get(self, meta: &TrackMeta) -> Option<&str> {
        match self {
            TextField::Title => meta.title.as_deref(),
            TextField::Artist => meta.artist.as_deref(),
        }
    }
}

fn set<T: Accessor>(tag: &mut T, field: TextField, value: Option<&str>) {
    match (field, value) {
        (TextField::Title, Some(v)) => tag.set_title(v.to_owned()),
        (TextField::Title, None) => tag.remove_title(),
        (TextField::Artist, Some(v)) => tag.set_artist(v.to_owned()),
        (TextField::Artist, None) => tag.remove_artist(),
    }
}

/// The format's main tag, or a new one seeded with what the list shows (title and artist
/// may have come from a secondary tag, e.g. RIFF INFO, and must not vanish).
fn main_tag<'a, T: Accessor + Default>(tag: &'a mut Option<T>, seed: &TrackMeta) -> &'a mut T {
    tag.get_or_insert_with(|| {
        let mut t = T::default();
        if let Some(v) = &seed.title {
            t.set_title(v.clone());
        }
        if let Some(v) = &seed.artist {
            t.set_artist(v.clone());
        }
        t
    })
}

/// Sets (or with `None` removes) one text field of the file's main tag, in place. `seed` is
/// what [`read`] returned for the file. The caller works on a copy (see `fileops::edit`).
pub fn write_text(
    path: &Path,
    field: TextField,
    value: Option<&str>,
    seed: &TrackMeta,
) -> crate::error::Result<()> {
    let bad = |e: &dyn std::fmt::Display| {
        crate::error::Error::Invalid(format!("cannot write the tag: {e}"))
    };
    let file_type = Probe::open(path)
        .and_then(|p| p.guess_file_type().map_err(Into::into))
        .map_err(|e| bad(&e))?
        .file_type();
    let opts = ParseOptions::new();
    let mut f = File::open(path)?;
    macro_rules! edit {
        ($file:ty, $tag:ident, $set:ident) => {{
            let mut file = <$file>::read_from(&mut f, opts).map_err(|e| bad(&e))?;
            drop(f);
            let mut tag = file.$tag().cloned();
            set(main_tag(&mut tag, seed), field, value);
            file.$set(tag.expect("main_tag inserted one"));
            file.save_to_path(path, WriteOptions::default())
                .map_err(|e| bad(&e))?;
        }};
    }
    match file_type {
        Some(FileType::Mpeg) => edit!(MpegFile, id3v2, set_id3v2),
        Some(FileType::Wav) => edit!(WavFile, id3v2, set_id3v2),
        Some(FileType::Aiff) => edit!(AiffFile, id3v2, set_id3v2),
        Some(FileType::Aac) => edit!(AacFile, id3v2, set_id3v2),
        Some(FileType::Flac) => edit!(FlacFile, vorbis_comments, set_vorbis_comments),
        Some(FileType::Mp4) => edit!(Mp4File, ilst, set_ilst),
        _ => {
            return Err(crate::error::Error::Invalid(
                "Tags of this file type cannot be edited.".into(),
            ));
        }
    }
    Ok(())
}

/// Number of embedded pictures in all tags; a tag write must not change it.
pub fn picture_count(path: &Path) -> Option<usize> {
    let file = Probe::open(path).ok()?.read().ok()?;
    Some(file.tags().iter().map(|t| t.pictures().len()).sum())
}

fn clean(s: Option<&str>) -> Option<String> {
    let s = s?.trim().trim_matches('\0').trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// ID3 `TBPM` maps to `IntegerBpm` in lofty but often holds decimals ("123.97"), so both
/// keys are parsed as floats. Implausible values are ignored (left for the analyzer).
fn bpm(tag: &Tag) -> Option<f64> {
    [ItemKey::Bpm, ItemKey::IntegerBpm]
        .into_iter()
        .filter_map(|k| tag.get_string(k))
        .filter_map(|s| s.trim().replace(',', ".").parse::<f64>().ok())
        .find(|b| (40.0..=300.0).contains(b))
}

fn format_name(file_type: FileType, has_bit_depth: bool) -> Option<String> {
    let name = match file_type {
        FileType::Mpeg => "MP3",
        FileType::Flac => "FLAC",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        // lofty reports a bit depth for lossless MP4 audio only.
        FileType::Mp4 if has_bit_depth => "ALAC",
        FileType::Mp4 | FileType::Aac => "AAC",
        FileType::Vorbis | FileType::Opus | FileType::Speex => "OGG",
        _ => return None,
    };
    Some(name.to_owned())
}

fn format_from_extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::testutil::{Tags, write_wav};

    #[test]
    fn reads_tags_and_properties_from_wav() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.wav");
        write_wav(
            &p,
            2.0,
            Some(Tags {
                title: "Acid Line",
                artist: "Someone",
                bpm: Some("123.97"),
                key: Some("Am"),
            }),
        );
        let m = read(&p);
        assert_eq!(m.title.as_deref(), Some("Acid Line"));
        assert_eq!(m.artist.as_deref(), Some("Someone"));
        assert_eq!(m.bpm, Some(123.97));
        assert_eq!(m.musical_key.as_deref(), Some("Am"));
        assert_eq!(m.format, "WAV");
        assert!((m.duration.unwrap() - 2.0).abs() < 0.01);
        assert_eq!(m.bitrate, Some(706)); // 44.1 kHz * 16 bit mono = 705.6 kbps
    }

    #[test]
    fn untagged_file_has_properties_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.wav");
        write_wav(&p, 1.0, None);
        let m = read(&p);
        assert_eq!((m.title, m.bpm, m.musical_key), (None, None, None));
        assert_eq!(m.format, "WAV");
    }

    #[test]
    fn garbage_falls_back_to_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("broken.flac");
        std::fs::write(&p, b"not audio").unwrap();
        assert_eq!(
            read(&p),
            TrackMeta {
                format: "FLAC".into(),
                ..TrackMeta::default()
            }
        );
    }

    #[test]
    fn implausible_bpm_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.wav");
        write_wav(
            &p,
            1.0,
            Some(Tags {
                title: "x",
                artist: "y",
                bpm: Some("0"),
                key: None,
            }),
        );
        assert_eq!(read(&p).bpm, None);
    }
}

/// Reads the spike fixtures when present (they are not in git): `cargo test -- --ignored`.
#[cfg(test)]
mod fixture_tests {
    #[test]
    #[ignore]
    fn spike_fixture_formats() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../spike/fixtures");
        for (file, format) in [
            ("short.mp3", "MP3"),
            ("short.wav", "WAV"),
            ("short.aiff", "AIFF"),
            ("short.flac", "FLAC"),
            ("short_aac.m4a", "AAC"),
            ("short_alac.m4a", "ALAC"),
        ] {
            let m = super::read(&dir.join(file));
            println!("{file}: {m:?}");
            assert_eq!(m.format, format, "{file}");
            assert!(m.duration.is_some_and(|d| (d - 30.0).abs() < 0.5), "{file}");
        }
    }
}
