//! Audio helpers that run in Rust: waveform peaks via symphonia, cached in `peaks_cache`.
//! Playback itself happens in the WebView (`<audio>` + asset protocol).

pub mod peaks;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Result;
use crate::library::file_key::file_key;
use crate::store::{self, Store};

use peaks::Peaks;

/// Only the newest waveform request is worth finishing: stepping through tracks with ↓
/// would otherwise queue a full decode per track passed.
#[derive(Default)]
pub struct PeakRequests(AtomicU64);

impl PeakRequests {
    pub fn begin(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn is_current(&self, id: u64) -> bool {
        self.0.load(Ordering::SeqCst) == id
    }
}

/// Cached peaks of the file, or decoded now and cached. The key is computed from the file
/// itself (not taken from the track row) so a file replaced since the scan is not shown
/// with stale peaks. Blocking: call from a worker thread.
pub fn peaks_for(store: &Store, path: &Path, cancelled: impl Fn() -> bool) -> Result<Peaks> {
    let key =
        file_key(path).map_err(|_| crate::error::Error::NotFound(path.display().to_string()))?;
    if let Some(p) = store::peaks::get(&*store.conn()?, &key, store::now())? {
        return Ok(p);
    }
    let p = peaks::compute(path, peaks::PER_SEC, cancelled)?;
    store::peaks::put(
        &*store.conn()?,
        &key,
        &p,
        store::now(),
        store::peaks::MAX_BYTES,
    )?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::testutil::write_wav;

    #[test]
    fn caches_by_content() {
        let store = Store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        write_wav(&a, 1.0, None);
        let first = peaks_for(&store, &a, || false).unwrap();

        // A copy elsewhere hits the cache: a cancelled decode would fail otherwise.
        let b = dir.path().join("b.wav");
        std::fs::copy(&a, &b).unwrap();
        assert_eq!(peaks_for(&store, &b, || true).unwrap(), first);

        // Replaced content under the same name is decoded again.
        write_wav(&a, 2.0, None);
        assert!((peaks_for(&store, &a, || false).unwrap().duration - 2.0).abs() < 1e-3);
    }

    #[test]
    fn only_newest_request_is_current() {
        let r = PeakRequests::default();
        let a = r.begin();
        assert!(r.is_current(a));
        let b = r.begin();
        assert!(!r.is_current(a) && r.is_current(b));
    }
}
