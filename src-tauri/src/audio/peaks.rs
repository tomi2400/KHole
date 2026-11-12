//! Waveform peaks: decode to mono (see `decode`), keep the RMS and peak of each bucket.

use std::ops::ControlFlow;
use std::path::Path;

use super::decode::MonoDecoder;
#[cfg(test)]
use crate::error::Error;
use crate::error::Result;

/// Buckets per second. A 7-minute track is 42 000 buckets (84 KB), plenty for a full-width
/// waveform and some zoom later.
pub const PER_SEC: u32 = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    pub per_sec: u32,
    /// Seconds, from the decoded frame count (exact, unlike container headers).
    pub duration: f64,
    /// Interleaved `[rms, peak]` pairs of the mono mix, scaled to 0..=127. The waveform draws
    /// the RMS: mastered tracks sit near full scale almost everywhere, so peaks alone show a
    /// solid block, while RMS shows breakdowns, builds and drops.
    pub data: Vec<i8>,
}

impl Peaks {
    /// Wire format for the UI (a raw `ArrayBuffer`, no JSON):
    /// `u32 per_sec | u32 format | f64 duration | i8 data…`, little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.data.len());
        out.extend_from_slice(&self.per_sec.to_le_bytes());
        out.extend_from_slice(&FORMAT.to_le_bytes());
        out.extend_from_slice(&self.duration.to_le_bytes());
        out.extend(self.data.iter().map(|&v| v as u8));
        out
    }
}

/// Data layout version (1 was `[min, max]`); also in the wire header.
pub const FORMAT: u32 = 2;

fn quantize(v: f32) -> i8 {
    (v.clamp(0.0, 1.0) * 127.0).round() as i8
}

#[derive(Default)]
struct Bucket {
    sum_sq: f64,
    peak: f32,
    n: u32,
}

impl Bucket {
    fn add(&mut self, v: f32) {
        self.sum_sq += (v * v) as f64;
        self.peak = self.peak.max(v.abs());
        self.n += 1;
    }

    fn take(&mut self) -> [i8; 2] {
        let rms = (self.sum_sq / self.n.max(1) as f64).sqrt() as f32;
        let out = [quantize(rms), quantize(self.peak)];
        *self = Bucket::default();
        out
    }
}

/// Decodes the whole file. `cancelled` is polled between packets so a superseded request
/// (the user already moved to another track) stops early with [`Error::Cancelled`].
pub fn compute(path: &Path, per_sec: u32, cancelled: impl Fn() -> bool) -> Result<Peaks> {
    let decoder = MonoDecoder::open(path)?;
    let rate = decoder.sample_rate();

    // Bucket k ends at frame ⌊(k+1)·rate / per_sec⌋, so odd rates (22 050 Hz…) do not drift.
    let bucket_end = |k: u64| (k + 1) * rate as u64 / per_sec as u64;
    let mut data = Vec::new();
    let mut acc = Bucket::default();
    let (mut frames, mut bucket, mut next_end) = (0u64, 0u64, bucket_end(0));

    decoder.run(cancelled, |block| {
        for &v in block {
            acc.add(v);
            frames += 1;
            if frames == next_end {
                data.extend(acc.take());
                bucket += 1;
                next_end = bucket_end(bucket);
            }
        }
        Ok(ControlFlow::Continue(()))
    })?;
    if acc.n > 0 {
        data.extend(acc.take());
    }
    Ok(Peaks {
        per_sec,
        duration: frames as f64 / rate as f64,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::testutil::write_wav;

    #[test]
    fn sine_peaks_have_expected_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write_wav(&path, 2.5, None);
        let p = compute(&path, PER_SEC, || false).unwrap();
        assert!((p.duration - 2.5).abs() < 1e-3, "{}", p.duration);
        assert_eq!(p.data.len(), 250 * 2);
        // The test sine peaks at 8000 / 32768 ≈ 0.244 → 31; its RMS is peak / √2 → 22.
        for pair in p.data.as_chunks::<2>().0 {
            assert!((21..=23).contains(&pair[0]), "{pair:?}");
            assert!((30..=32).contains(&pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn odd_rate_division_does_not_drift() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write_wav(&path, 1.0, None);
        // 44 100 / 7 is not an integer: still exactly 7 buckets for one second.
        let p = compute(&path, 7, || false).unwrap();
        assert_eq!(p.data.len(), 7 * 2);
    }

    #[test]
    fn cancel_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write_wav(&path, 1.0, None);
        assert!(matches!(
            compute(&path, PER_SEC, || true),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            compute(&dir.path().join("gone.wav"), PER_SEC, || false),
            Err(Error::NotFound(_))
        ));
        let junk = dir.path().join("junk.mp3");
        std::fs::write(&junk, b"not audio at all").unwrap();
        assert!(matches!(
            compute(&junk, PER_SEC, || false),
            Err(Error::Decode(_))
        ));
    }

    /// Timing on the spike fixtures (not in git):
    /// `cargo test peaks_of_spike_fixtures -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn peaks_of_spike_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../spike/fixtures");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files {
            let t = std::time::Instant::now();
            let p = compute(&f, PER_SEC, || false).unwrap();
            println!(
                "{:28} {:>8.1?} {:7.2}s {:>7} pairs",
                f.file_name().unwrap().to_string_lossy(),
                t.elapsed(),
                p.duration,
                p.data.len() / 2
            );
        }
    }

    #[test]
    fn wire_format() {
        let p = Peaks {
            per_sec: 100,
            duration: 1.5,
            data: vec![3, 4],
        };
        let b = p.to_bytes();
        assert_eq!(&b[0..4], &100u32.to_le_bytes());
        assert_eq!(&b[8..16], &1.5f64.to_le_bytes());
        assert_eq!(&b[4..8], &FORMAT.to_le_bytes());
        assert_eq!(&b[16..], &[3, 4]);
    }
}
