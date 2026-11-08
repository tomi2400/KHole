//! `peaks_cache`: waveform peaks by `file_key`, evicted least-recently-used beyond a size cap.

use rusqlite::{Connection, OptionalExtension, params};

use crate::audio::peaks::Peaks;
use crate::error::Result;

/// Total peak bytes kept: about 800 seven-minute tracks.
pub const MAX_BYTES: i64 = 64 * 1024 * 1024;

/// Returns the cached peaks and marks them as used.
pub fn get(conn: &Connection, file_key: &str, now: i64) -> Result<Option<Peaks>> {
    let row = conn
        .query_row(
            "UPDATE peaks_cache SET used_at = ?2 WHERE file_key = ?1
             RETURNING per_sec, duration, data",
            params![file_key, now],
            |r| {
                let data: Vec<u8> = r.get(2)?;
                Ok(Peaks {
                    per_sec: r.get(0)?,
                    duration: r.get(1)?,
                    data: data.into_iter().map(|b| b as i8).collect(),
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Stores peaks, then trims the cache to `max_bytes` (oldest `used_at` first).
pub fn put(
    conn: &Connection,
    file_key: &str,
    peaks: &Peaks,
    now: i64,
    max_bytes: i64,
) -> Result<()> {
    let data: Vec<u8> = peaks.data.iter().map(|&v| v as u8).collect();
    conn.execute(
        "INSERT OR REPLACE INTO peaks_cache (file_key, per_sec, duration, data, used_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![file_key, peaks.per_sec, peaks.duration, data, now],
    )?;
    conn.execute(
        "DELETE FROM peaks_cache WHERE file_key IN (
           SELECT file_key FROM (
             SELECT file_key,
                    sum(length(data)) OVER (ORDER BY used_at DESC, file_key) AS running
             FROM peaks_cache)
           WHERE running > ?1)",
        [max_bytes],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn peaks(n: usize) -> Peaks {
        Peaks {
            per_sec: 100,
            duration: n as f64 / 200.0,
            data: (0..n).map(|i| (i % 255) as i8).collect(),
        }
    }

    fn keys(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT file_key FROM peaks_cache ORDER BY file_key")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        assert_eq!(get(&conn, "k", 1).unwrap(), None);
        let p = peaks(300);
        put(&conn, "k", &p, 1, MAX_BYTES).unwrap();
        assert_eq!(get(&conn, "k", 2).unwrap(), Some(p));
    }

    #[test]
    fn evicts_least_recently_used() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        put(&conn, "a", &peaks(100), 1, 250).unwrap();
        put(&conn, "b", &peaks(100), 2, 250).unwrap();
        get(&conn, "a", 3).unwrap(); // `a` is now newer than `b`
        put(&conn, "c", &peaks(100), 4, 250).unwrap();
        assert_eq!(keys(&conn), ["a", "c"]);
    }
}
