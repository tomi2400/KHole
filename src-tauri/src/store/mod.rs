//! SQLite storage (rusqlite, bundled). Schema changes are append-only migrations tracked
//! with `PRAGMA user_version`.

pub mod actions;
pub mod analysis;
pub mod destinations;
pub mod peaks;
pub mod sessions;
pub mod tracks;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Ordered migrations; the index + 1 is the schema version after applying it.
/// Never edit a released migration – add a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_peaks_used_at.sql"),
    include_str!("migrations/0003_peaks_rms.sql"),
    include_str!("migrations/0004_actions_undo.sql"),
    include_str!("migrations/0005_actions_track_index.sql"),
    include_str!("migrations/0006_actions_duplicate.sql"),
    include_str!("migrations/0007_actions_edits.sql"),
    include_str!("migrations/0008_analysis_error.sql"),
    include_str!("migrations/0009_bpm_overrides.sql"),
    include_str!("migrations/0010_bpm_range.sql"),
];

/// Timestamps in the DB are Unix milliseconds.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| Error::StorePoisoned)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn()?
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

    #[test]
    fn migrates_fresh_db_to_latest() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let conn = store.conn().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            tables,
            [
                "actions",
                "analysis_cache",
                "bpm_overrides",
                "destinations",
                "peaks_cache",
                "sessions",
                "tracks"
            ]
        );
    }

    #[test]
    fn reopening_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("khole.db");
        drop(Store::open(&path).unwrap());
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn foreign_keys_cascade() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO sessions (source_path, created_at, updated_at) VALUES ('/x', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO destinations (session_id, path, hotkey, position) VALUES (1, '/y', 1, 0)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM sessions", []).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM destinations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    /// 0007 rebuilds `actions`: the log of an existing DB must survive it.
    #[test]
    fn actions_survive_the_0007_rebuild() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..6] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 6).unwrap();
        conn.execute_batch(
            "INSERT INTO sessions (id, source_path, created_at, updated_at) VALUES (1, '/x', 0, 0);
             INSERT INTO destinations (id, session_id, path, hotkey, position) VALUES (1, 1, '/y', 1, 0);
             INSERT INTO tracks (id, session_id, path, file_key) VALUES (1, 1, '/y/a.mp3', 'k');
             INSERT INTO actions (session_id, track_id, kind, src, dst, ts, prev_status,
                                  destination_id, duplicate)
               VALUES (1, 1, 'move', '/x/a.mp3', '/y/a.mp3', 5, 'kept', 1, 1);",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let row: (String, String, String, i64, i64) = conn
            .query_row(
                "SELECT kind, dst, prev_status, destination_id, duplicate FROM actions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(row, ("move".into(), "/y/a.mp3".into(), "kept".into(), 1, 1));
        conn.execute(
            "INSERT INTO actions (session_id, track_id, kind, src, ts, field, old_value)
             VALUES (1, 1, 'tag', '/y/a.mp3', 6, 'title', 'Old')",
            [],
        )
        .unwrap();
        // Still cascades with its session.
        conn.execute("DELETE FROM sessions", []).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM actions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn rejects_invalid_status() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO sessions (source_path, created_at, updated_at) VALUES ('/x', 0, 0)",
            [],
        )
        .unwrap();
        let err = conn.execute(
            "INSERT INTO tracks (session_id, path, file_key, status) VALUES (1, '/x/a.mp3', 'k', 'deleted')",
            [],
        );
        assert!(err.is_err());
    }
}
