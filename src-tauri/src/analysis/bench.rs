//! Accuracy and speed on a real folder (open questions 1–2 in docs/DEVELOPMENT.md):
//! tag BPM / key against the analysis, wall-clock time with the queue's thread count.
//!
//! `KHOLE_BENCH_DIR=~/Music/promo cargo test --release bench_folder -- --ignored --nocapture`

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::tests::built_sidecar;
use crate::library::{scan, tags};
use crate::store::analysis::Analysis;

/// Camelot code of a key written as `Am`, `A minor`, `Amin`, `F#`, `Gb major` or `8A`.
fn camelot(key: &str) -> Option<String> {
    let k = key.trim();
    let upper = k.to_ascii_uppercase();
    if let Some(num) = upper.strip_suffix('A').or_else(|| upper.strip_suffix('B'))
        && let Ok(n) = num.parse::<u8>()
        && (1..=12).contains(&n)
    {
        return Some(format!("{n}{}", &upper[upper.len() - 1..]));
    }
    let mut chars = k.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut pc: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let mut rest = rest.as_str();
    if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
        pc += 1;
        rest = r;
    } else if let Some(r) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('♭')) {
        pc -= 1;
        rest = r;
    }
    let rest = rest.trim().to_ascii_lowercase();
    let minor = rest.starts_with('m') && !rest.starts_with("maj");
    let major_pc = if minor { pc + 3 } else { pc }.rem_euclid(12);
    let fifths = (major_pc * 7).rem_euclid(12); // C = 0, G = 1, D = 2 …
    let n = (7 + fifths) % 12 + 1; // C major = 8B
    Some(format!("{n}{}", if minor { 'A' } else { 'B' }))
}

#[derive(Debug, PartialEq)]
enum BpmMatch {
    Same,
    HalfOrDouble,
    Off,
}

fn compare_bpm(tag: f64, got: f64) -> BpmMatch {
    let near = |a: f64, b: f64| (a - b).abs() <= 0.5;
    if near(tag, got) {
        BpmMatch::Same
    } else if near(tag, got * 2.0) || near(tag * 2.0, got) {
        BpmMatch::HalfOrDouble
    } else {
        BpmMatch::Off
    }
}

#[test]
fn camelot_codes() {
    assert_eq!(camelot("Am").as_deref(), Some("8A"));
    assert_eq!(camelot("A minor").as_deref(), Some("8A"));
    assert_eq!(camelot("08A").as_deref(), Some("8A"));
    assert_eq!(camelot("C").as_deref(), Some("8B"));
    assert_eq!(camelot("F#").as_deref(), Some("2B"));
    assert_eq!(camelot("Gb major").as_deref(), Some("2B"));
    assert_eq!(camelot("Ebm").as_deref(), Some("2A"));
    assert_eq!(camelot("D#min").as_deref(), Some("2A"));
    assert_eq!(camelot("Abm").as_deref(), Some("1A"));
    assert_eq!(camelot("x"), None);
    assert_eq!(compare_bpm(124.0, 62.02), BpmMatch::HalfOrDouble);
    assert_eq!(compare_bpm(124.0, 124.3), BpmMatch::Same);
}

#[test]
#[ignore]
fn bench_folder() {
    let Ok(dir) = std::env::var("KHOLE_BENCH_DIR") else {
        eprintln!("set KHOLE_BENCH_DIR to a folder of real tracks");
        return;
    };
    let dir = PathBuf::from(dir.replacen('~', &std::env::var("HOME").unwrap(), 1));
    if built_sidecar().is_none() {
        return;
    }
    let files = scan::list_audio_files(&dir, true, &[]);
    let threads = super::queue::worker_count();
    println!("{} files, {threads} threads", files.len());

    type Row = (PathBuf, Duration, f64, Result<Analysis, String>);
    let results: Mutex<Vec<Row>> = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let wall = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let t = Instant::now();
                    let r = super::analyze(built_sidecar().unwrap(), path, || false)
                        .map_err(|e| e.to_string());
                    let secs = tags::read(path).duration.unwrap_or(0.0);
                    results
                        .lock()
                        .unwrap()
                        .push((path.clone(), t.elapsed(), secs, r));
                }
            });
        }
    });
    let wall = wall.elapsed();

    let mut rows = results.into_inner().unwrap();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let (mut bpm_n, mut same, mut half, mut key_n, mut key_same, mut failed) = (0, 0, 0, 0, 0, 0);
    let (mut audio, mut cpu) = (0.0, Duration::ZERO);
    println!(
        "{:40} {:>7} {:>7} {:>5}  {:>4} {:>4}  {:>6}",
        "file", "tag", "bpm", "conf", "tag", "key", "time"
    );
    for (path, took, secs, r) in &rows {
        let meta = tags::read(path);
        audio += secs;
        cpu += *took;
        let name: String = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .chars()
            .take(40)
            .collect();
        let a = match r {
            Ok(a) => a,
            Err(e) => {
                failed += 1;
                println!("{name:40} failed: {e}");
                continue;
            }
        };
        let mut mark = String::new();
        if let Some(tag) = meta.bpm {
            bpm_n += 1;
            match compare_bpm(tag, a.bpm) {
                BpmMatch::Same => same += 1,
                BpmMatch::HalfOrDouble => {
                    half += 1;
                    mark.push_str(" ½×2");
                }
                BpmMatch::Off => mark.push_str(" BPM?"),
            }
        }
        let got_key = crate::store::analysis::key_name(&a.key, Some(&a.scale));
        let (tag_c, got_c) = (
            meta.musical_key.as_deref().and_then(camelot),
            camelot(&got_key),
        );
        if let Some(t) = &tag_c {
            key_n += 1;
            if Some(t) == got_c.as_ref() {
                key_same += 1;
            } else {
                mark.push_str(" KEY?");
            }
        }
        println!(
            "{name:40} {:>7} {:>7.2} {:>5.2}  {:>4} {:>4}  {:>5.1?}{mark}",
            meta.bpm.map_or("-".into(), |b| format!("{b:.2}")),
            a.bpm,
            a.bpm_confidence,
            tag_c.unwrap_or_else(|| "-".into()),
            got_c.unwrap_or_default(),
            took,
        );
    }
    println!(
        "\nBPM: {same}/{bpm_n} within ±0.5, {half} half/double · key: {key_same}/{key_n} same (Camelot) · {failed} failed"
    );
    println!(
        "time: wall {wall:.1?} for {:.0} min of audio · {:.2} s per audio minute per thread · {:.2} s per track (wall)",
        audio / 60.0,
        cpu.as_secs_f64() / (audio / 60.0).max(1e-9),
        wall.as_secs_f64() / rows.len().max(1) as f64,
    );
}
