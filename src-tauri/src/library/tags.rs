//! Tag and audio-property reading with lofty. Nothing is ever written back to the files.

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::FileType;
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
