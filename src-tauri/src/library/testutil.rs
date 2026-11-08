//! Test helpers: tiny generated audio files instead of fixtures in git.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag, TagType};

pub struct Tags<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub bpm: Option<&'a str>,
    pub key: Option<&'a str>,
}

/// 16-bit mono 44.1 kHz WAV with a 440 Hz sine, optionally with an ID3v2 tag.
pub fn write_wav(path: &Path, seconds: f64, tags: Option<Tags>) {
    const RATE: u32 = 44_100;
    let n = (seconds * RATE as f64) as u32;
    let mut data = Vec::with_capacity(44 + n as usize * 2);
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
    for i in 0..n {
        let s = (i as f64 * 440.0 * std::f64::consts::TAU / RATE as f64).sin();
        data.extend_from_slice(&((s * 8000.0) as i16).to_le_bytes());
    }
    std::fs::write(path, data).unwrap();

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
