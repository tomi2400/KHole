use std::{fs::File, path::Path, time::Instant};

use lofty::prelude::*;
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// Decode a file and compute min/max peaks at `per_sec` buckets per second (mono mix).
fn peaks(path: &Path, per_sec: u32) -> Result<(u32, u64, Vec<(f32, f32)>), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no audio params")?
        .clone();
    let track_id = track.id;
    let sr = params.sample_rate.ok_or("no sample rate")?;
    let ch = params.channels.as_ref().map(|c| c.count()).unwrap_or(2).max(1);
    let mut dec = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let bucket = (sr / per_sec) as usize;
    let (mut out, mut lo, mut hi, mut n, mut frames) = (Vec::new(), f32::MAX, f32::MIN, 0usize, 0u64);
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match dec.decode(&packet) {
            Ok(ab) => {
                buf.resize(ab.samples_interleaved(), 0.0);
                ab.copy_to_slice_interleaved(&mut buf);
                for f in buf.chunks_exact(ch) {
                    let v = f.iter().sum::<f32>() / ch as f32;
                    lo = lo.min(v);
                    hi = hi.max(v);
                    n += 1;
                    frames += 1;
                    if n == bucket {
                        out.push((lo, hi));
                        lo = f32::MAX;
                        hi = f32::MIN;
                        n = 0;
                    }
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok((sr, frames, out))
}

/// Decode a whole file to mono f32 samples.
fn decode_mono(path: &Path) -> Result<(u32, Vec<f32>), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("no params")?.clone();
    let track_id = track.id;
    let sr = params.sample_rate.ok_or("no sample rate")?;
    let ch = params.channels.as_ref().map(|c| c.count()).unwrap_or(2).max(1);
    let mut dec = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let (mut out, mut buf) = (Vec::new(), Vec::<f32>::new());
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        if let Ok(ab) = dec.decode(&packet) {
            buf.resize(ab.samples_interleaved(), 0.0);
            ab.copy_to_slice_interleaved(&mut buf);
            out.extend(buf.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
        }
    }
    Ok((sr, out))
}

/// Move to Trash via NSFileManager and return the resulting path inside the Trash.
fn trash_with_result(path: &Path) -> Result<String, String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let fm = NSFileManager::defaultManager();
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().ok_or("non-utf8")?));
    let mut result: Option<objc2::rc::Retained<NSURL>> = None;
    fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut result))
        .map_err(|e| e.to_string())?;
    let r = result.ok_or("no resulting URL")?;
    Ok(r.path().map(|p| p.to_string()).unwrap_or_default())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("--pcm") {
        // Decode to mono f32le on stdout; print the sample rate on stderr.
        use std::io::Write;
        let (sr, samples) = decode_mono(Path::new(&args[1])).unwrap();
        eprintln!("{sr}");
        let mut out = std::io::BufWriter::new(std::io::stdout().lock());
        for s in samples {
            out.write_all(&s.to_le_bytes()).unwrap();
        }
        return;
    }
    if args.first().map(|s| s.as_str()) == Some("--trash-test") {
        let dir = std::env::temp_dir().join("khole-trash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("khole trash test ű.txt");
        std::fs::write(&f, b"hello").unwrap();
        let t = Instant::now();
        match trash_with_result(&f) {
            Ok(p) => {
                println!("trashed in {:?} -> {p}", t.elapsed());
                std::fs::rename(&p, &f).expect("restore failed");
                println!("restored: {}", f.exists());
                std::fs::remove_file(&f).ok();
            }
            Err(e) => println!("trash error: {e}"),
        }
        return;
    }
    for a in &args {
        let p = Path::new(a);
        let name = p.file_name().unwrap().to_string_lossy();
        let t = Instant::now();
        let tags = match lofty::read_from_path(p) {
            Ok(tf) => {
                let pr = tf.properties();
                format!(
                    "lofty {:?} dur={:.1}s br={:?} sr={:?} bits={:?} tag={}",
                    t.elapsed(),
                    pr.duration().as_secs_f32(),
                    pr.audio_bitrate(),
                    pr.sample_rate(),
                    pr.bit_depth(),
                    tf.primary_tag().is_some()
                )
            }
            Err(e) => format!("lofty ERR {e}"),
        };
        let t = Instant::now();
        match peaks(p, 100) {
            Ok((sr, frames, pk)) => println!(
                "{name:24} peaks {:>9.1?} dur={:.1}s n={} | {tags}",
                t.elapsed(),
                frames as f64 / sr as f64,
                pk.len()
            ),
            Err(e) => println!("{name:24} peaks ERR {e} | {tags}"),
        }
    }
}
