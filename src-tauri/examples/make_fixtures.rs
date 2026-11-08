//! Generates a small test library for manual testing (the audio files stay out of git).
//!
//!     cargo run --example make_fixtures -- ~/Music/KHole-test
//!
//! Writes 12 short tracks (kick + bass at different tempos) as WAV, and converts them to
//! AIFF, FLAC, AAC and ALAC with macOS `afconvert`. Some get full tags, some no BPM/key
//! (for the analyzer), some no tags at all. Also a subfolder, a hidden file and a non-audio
//! file to exercise the scanner. No MP3: macOS has no MP3 encoder.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Command;

use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};

const RATE: u32 = 44_100;
const SECONDS: f64 = 40.0;

struct Spec {
    file: &'static str,
    format: Option<(&'static str, &'static str)>, // afconvert (file type, data format)
    bpm: f64,
    title: Option<&'static str>,
    artist: Option<&'static str>,
    tag_bpm: bool,
    key: Option<&'static str>,
}

const SPECS: &[Spec] = &[
    spec(
        "01 Night Drive.wav",
        None,
        124.0,
        Some("Night Drive"),
        Some("Kessel"),
        true,
        Some("8A"),
    ),
    spec(
        "02 Low Orbit.aiff",
        AIFF,
        128.0,
        Some("Low Orbit"),
        Some("Varga & Lind"),
        true,
        Some("Am"),
    ),
    spec(
        "03 Concrete.flac",
        FLAC,
        132.0,
        Some("Concrete"),
        Some("Mira K"),
        true,
        Some("F#m"),
    ),
    spec(
        "04 Hollow Body.m4a",
        AAC,
        122.0,
        Some("Hollow Body"),
        Some("Pale Sun"),
        false,
        None,
    ),
    spec(
        "05 Copper Wire.m4a",
        ALAC,
        126.0,
        Some("Copper Wire"),
        Some("Okra"),
        true,
        None,
    ),
    spec(
        "06 untagged_promo_master.wav",
        None,
        130.0,
        None,
        None,
        false,
        None,
    ),
    spec(
        "07 Static Bloom.aiff",
        AIFF,
        118.0,
        Some("Static Bloom"),
        Some("Hanna Feld"),
        false,
        Some("10B"),
    ),
    spec(
        "08 Tunnel Vision.flac",
        FLAC,
        140.0,
        Some("Tunnel Vision"),
        Some("Duna"),
        true,
        Some("Gm"),
    ),
    spec(
        "09 a very long title that should be truncated in the track list (extended mix).wav",
        None,
        125.0,
        Some("A Very Long Title That Should Be Truncated In The Track List (Extended Mix)"),
        Some("An Artist With An Equally Long Name feat. Somebody Else"),
        true,
        Some("5A"),
    ),
    spec(
        "10 Ékezetes Cím.flac",
        FLAC,
        127.0,
        Some("Ékezetes Cím"),
        Some("Árvíztűrő"),
        true,
        Some("C"),
    ),
    spec(
        "sub/11 Deep Cut.wav",
        None,
        121.0,
        Some("Deep Cut"),
        Some("Sublevel"),
        true,
        Some("2A"),
    ),
    spec(
        "sub/more/12 Basement.aiff",
        AIFF,
        135.0,
        Some("Basement"),
        Some("Sublevel"),
        false,
        None,
    ),
];

const AIFF: Option<(&str, &str)> = Some(("AIFF", "BEI16"));
const FLAC: Option<(&str, &str)> = Some(("flac", "flac"));
const AAC: Option<(&str, &str)> = Some(("m4af", "aac"));
const ALAC: Option<(&str, &str)> = Some(("m4af", "alac"));

#[allow(clippy::too_many_arguments)]
const fn spec(
    file: &'static str,
    format: Option<(&'static str, &'static str)>,
    bpm: f64,
    title: Option<&'static str>,
    artist: Option<&'static str>,
    tag_bpm: bool,
    key: Option<&'static str>,
) -> Spec {
    Spec {
        file,
        format,
        bpm,
        title,
        artist,
        tag_bpm,
        key,
    }
}

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: make_fixtures <output folder>"),
    );
    if out.exists() && std::fs::read_dir(&out).unwrap().next().is_some() {
        panic!("{} is not empty; pick a new folder", out.display());
    }
    for s in SPECS {
        let path = out.join(s.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let wav = path.with_extension("tmp.wav");
        write_wav(&wav, s.bpm);
        match s.format {
            None => std::fs::rename(&wav, &path).unwrap(),
            Some((file_type, data_format)) => {
                afconvert(&wav, &path, file_type, data_format);
                std::fs::remove_file(&wav).unwrap();
            }
        }
        tag(&path, s);
        println!("{}", path.display());
    }
    std::fs::write(out.join("readme.txt"), "not audio").unwrap();
    std::fs::write(out.join(".hidden.wav"), "hidden").unwrap();
}

/// 16-bit stereo: four-on-the-floor kick plus an off-beat bass note.
fn write_wav(path: &Path, bpm: f64) {
    let n = (SECONDS * RATE as f64) as u32;
    let beat = 60.0 / bpm;
    let mut data = Vec::with_capacity(44 + n as usize * 4);
    for chunk in [
        &b"RIFF"[..],
        &(36 + n * 4).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
    ] {
        data.extend_from_slice(chunk);
    }
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&RATE.to_le_bytes());
    data.extend_from_slice(&(RATE * 4).to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(n * 4).to_le_bytes());
    for i in 0..n {
        let t = i as f64 / RATE as f64;
        let in_beat = t % beat;
        let kick = (TAU * (50.0 + 100.0 * (-in_beat * 30.0).exp()) * in_beat).sin()
            * (-in_beat * 8.0).exp();
        let off = (t + beat / 2.0) % beat;
        let bass = (TAU * 55.0 * t).sin() * (-off * 6.0).exp() * 0.5;
        let s = ((kick + bass) * 0.6 * i16::MAX as f64) as i16;
        data.extend_from_slice(&s.to_le_bytes());
        data.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

fn afconvert(src: &Path, dst: &Path, file_type: &str, data_format: &str) {
    let ok = Command::new("afconvert")
        .args(["-f", file_type, "-d", data_format])
        .arg(src)
        .arg(dst)
        .status()
        .expect("afconvert not found")
        .success();
    assert!(ok, "afconvert failed for {}", dst.display());
}

fn tag(path: &Path, s: &Spec) {
    if s.title.is_none() && s.artist.is_none() {
        return;
    }
    let mut file = lofty::read_from_path(path).unwrap();
    let mut tag = Tag::new(file.primary_tag_type());
    if let Some(t) = s.title {
        tag.set_title(t.into());
    }
    if let Some(a) = s.artist {
        tag.set_artist(a.into());
    }
    if s.tag_bpm {
        // ID3 and MP4 store BPM as `IntegerBpm` (TBPM / tmpo), Vorbis comments as `Bpm`;
        // keys a format has no field for are dropped on save.
        tag.insert_text(ItemKey::IntegerBpm, format!("{}", s.bpm));
        tag.insert_text(ItemKey::Bpm, format!("{}", s.bpm));
    }
    if let Some(k) = s.key {
        tag.insert_text(ItemKey::InitialKey, k.into());
    }
    file.insert_tag(tag);
    file.save_to_path(path, WriteOptions::default()).unwrap();
}
