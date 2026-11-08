//! One session per source folder: its settings and resume point.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub source_path: String,
    pub include_sub: bool,
    pub mode: String,
    pub start_at: f64,
    pub auto_next: bool,
    pub current_track: Option<i64>,
}

const COLUMNS: &str = "id, source_path, include_sub, mode, start_at, auto_next, current_track";

fn from_row(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        source_path: r.get(1)?,
        include_sub: r.get(2)?,
        mode: r.get(3)?,
        start_at: r.get(4)?,
        auto_next: r.get(5)?,
        current_track: r.get(6)?,
    })
}

/// Returns the folder's session, creating it with defaults on first import.
/// Opening counts as use, so it moves to the top of the recent list.
pub fn open(conn: &Connection, source_path: &str, now: i64) -> Result<Session> {
    Ok(conn.query_row(
        &format!(
            "INSERT INTO sessions (source_path, created_at, updated_at) VALUES (?1, ?2, ?2)
             ON CONFLICT (source_path) DO UPDATE SET updated_at = excluded.updated_at
             RETURNING {COLUMNS}"
        ),
        params![source_path, now],
        from_row,
    )?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Session> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("session {id}")))
}

pub fn set_include_sub(conn: &Connection, id: i64, value: bool, now: i64) -> Result<Session> {
    update(conn, id, "include_sub", &value, now)
}

/// Where playback starts, as a fraction of the track (0 = from the top, 0.25, 0.4, …).
pub fn set_start_at(conn: &Connection, id: i64, value: f64, now: i64) -> Result<Session> {
    if !(0.0..=0.9).contains(&value) {
        return Err(Error::Invalid(format!(
            "start position {value} is out of range"
        )));
    }
    update(conn, id, "start_at", &value, now)
}

/// Move or copy when a track goes to a destination.
pub fn set_mode(conn: &Connection, id: i64, mode: &str, now: i64) -> Result<Session> {
    if !matches!(mode, "move" | "copy") {
        return Err(Error::Invalid(format!("unknown mode {mode}")));
    }
    update(conn, id, "mode", &mode, now)
}

/// Whether the next track starts playing after a decision.
pub fn set_auto_next(conn: &Connection, id: i64, value: bool, now: i64) -> Result<Session> {
    update(conn, id, "auto_next", &value, now)
}

/// Sets one settings column (a fixed name from this module, never user input).
fn update(
    conn: &Connection,
    id: i64,
    column: &'static str,
    value: &dyn rusqlite::ToSql,
    now: i64,
) -> Result<Session> {
    conn.query_row(
        &format!(
            "UPDATE sessions SET {column} = ?2, updated_at = ?3 WHERE id = ?1
             RETURNING {COLUMNS}"
        ),
        params![id, value, now],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("session {id}")))
}

/// Remembers the loaded track so the session resumes there. A track of another session is
/// refused; `None` clears it.
pub fn set_current_track(conn: &Connection, id: i64, track_id: Option<i64>) -> Result<()> {
    let n = conn.execute(
        "UPDATE sessions SET current_track = ?2 WHERE id = ?1
           AND (?2 IS NULL OR EXISTS (SELECT 1 FROM tracks WHERE id = ?2 AND session_id = ?1))",
        params![id, track_id],
    )?;
    if n == 0 {
        return Err(Error::NotFound(format!(
            "session {id} / track {track_id:?}"
        )));
    }
    Ok(())
}

pub fn touch(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentSession {
    pub id: i64,
    pub source_path: String,
    pub total: i64,
    pub remaining: i64,
    /// Unix milliseconds.
    pub updated_at: i64,
    /// False if the folder was moved or deleted since (or its volume is unmounted).
    pub available: bool,
}

pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<RecentSession>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.source_path, count(t.id), coalesce(sum(t.status = 'pending'), 0),
                s.updated_at
         FROM sessions s LEFT JOIN tracks t ON t.session_id = s.id
         GROUP BY s.id
         ORDER BY s.updated_at DESC, s.id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        let source_path: String = r.get(1)?;
        Ok(RecentSession {
            id: r.get(0)?,
            available: Path::new(&source_path).is_dir(),
            source_path,
            total: r.get(2)?,
            remaining: r.get(3)?,
            updated_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn open_is_get_or_create() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let a = open(&conn, "/music/promo", 1).unwrap();
        assert_eq!(a.mode, "move");
        assert!(!a.include_sub && a.auto_next && a.start_at == 0.0);
        let b = open(&conn, "/music/promo", 2).unwrap();
        assert_eq!(a, b);
        assert_eq!(get(&conn, a.id).unwrap(), a);
        assert!(matches!(get(&conn, 99), Err(Error::NotFound(_))));
    }

    #[test]
    fn include_sub_persists() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let id = open(&conn, "/x", 1).unwrap().id;
        assert!(set_include_sub(&conn, id, true, 2).unwrap().include_sub);
        assert!(get(&conn, id).unwrap().include_sub);
        assert!(set_include_sub(&conn, 42, true, 2).is_err());
    }

    #[test]
    fn start_at_and_current_track() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let a = open(&conn, "/a", 1).unwrap().id;
        let b = open(&conn, "/b", 1).unwrap().id;
        assert_eq!(set_start_at(&conn, a, 0.25, 2).unwrap().start_at, 0.25);
        assert!(set_start_at(&conn, a, 1.0, 2).is_err());
        assert!(set_start_at(&conn, a, -0.1, 2).is_err());

        conn.execute_batch(&format!(
            "INSERT INTO tracks (id, session_id, path, file_key) VALUES (7, {a}, 'p', 'k')"
        ))
        .unwrap();
        set_current_track(&conn, a, Some(7)).unwrap();
        assert_eq!(get(&conn, a).unwrap().current_track, Some(7));
        assert!(set_current_track(&conn, b, Some(7)).is_err());
        set_current_track(&conn, a, None).unwrap();
        assert_eq!(get(&conn, a).unwrap().current_track, None);
    }

    #[test]
    fn mode_and_auto_next() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let id = open(&conn, "/x", 1).unwrap().id;
        assert_eq!(set_mode(&conn, id, "copy", 2).unwrap().mode, "copy");
        assert!(set_mode(&conn, id, "link", 2).is_err());
        assert!(!set_auto_next(&conn, id, false, 2).unwrap().auto_next);
        assert_eq!(get(&conn, id).unwrap().mode, "copy");
    }

    #[test]
    fn recent_orders_by_use_and_counts_remaining() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().to_str().unwrap();
        let a = open(&conn, existing, 1).unwrap().id;
        let b = open(&conn, "/gone", 2).unwrap().id;
        conn.execute_batch(&format!(
            "INSERT INTO tracks (session_id, path, file_key, status) VALUES
               ({a}, 'p1', 'k1', 'pending'), ({a}, 'p2', 'k2', 'kept'), ({a}, 'p3', 'k3', 'pending')"
        ))
        .unwrap();
        let r = recent(&conn, 5).unwrap();
        assert_eq!(r.iter().map(|s| s.id).collect::<Vec<_>>(), [b, a]);
        assert_eq!((r[1].total, r[1].remaining, r[1].available), (3, 2, true));
        assert_eq!((r[0].total, r[0].remaining, r[0].available), (0, 0, false));

        open(&conn, existing, 3).unwrap();
        assert_eq!(recent(&conn, 1).unwrap()[0].id, a);
    }
}
