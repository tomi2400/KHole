//! Test helpers: tiny generated audio files instead of fixtures in git.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag, TagType};

const RATE: u32 = 44_100;

pub struct Tags<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub bpm: Option<&'a str>,
    pub key: Option<&'a str>,
}

/// 16-bit mono 44.1 kHz WAV with a 440 Hz sine, optionally with an ID3v2 tag.
pub fn write_wav(path: &Path, seconds: f64, tags: Option<Tags>) {
    let n = (seconds * RATE as f64) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let s = (i as f64 * 440.0 * std::f64::consts::TAU / RATE as f64).sin();
            (s * 8000.0) as i16
        })
        .collect();
    write_pcm(path, &samples);
    if let Some(t) = tags {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(t.title.into());
        tag.set_artist(t.artist.into());
        if let Some(bpm) = t.bpm {
            tag.insert_text(ItemKey::IntegerBpm, bpm.into());
        }
        if let Some(key) = t.key {
            tag.insert_text(ItemKey::InitialKey, key.into());
        }
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
}

/// Four-on-the-floor kicks at `bpm` over a quiet A minor chord: something the analyzer
/// can find a tempo in.
pub fn write_click_wav(path: &Path, seconds: f64, bpm: f64) {
    use std::f64::consts::TAU;
    let n = (seconds * RATE as f64) as usize;
    let beat = 60.0 / bpm;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let since = t % beat;
            // Kick: a sine sweeping 200 → 50 Hz, fading out fast.
            let kick = if since < 0.15 {
                let phase = 50.0 * since + 150.0 / 40.0 * (1.0 - (-40.0 * since).exp());
                (TAU * phase).sin() * (-since * 30.0).exp()
            } else {
                0.0
            };
            let chord: f64 = [220.0, 261.63, 329.63]
                .iter()
                .map(|f| (TAU * f * t).sin())
                .sum::<f64>()
                * 0.05;
            ((kick * 0.8 + chord) * 20_000.0) as i16
        })
        .collect();
    write_pcm(path, &samples);
}

fn write_pcm(path: &Path, samples: &[i16]) {
    let n = samples.len() as u32;
    let mut data = Vec::with_capacity(44 + samples.len() * 2);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + n * 2).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&RATE.to_le_bytes());
    data.extend_from_slice(&(RATE * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(n * 2).to_le_bytes());
    for s in samples {
        data.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}
