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
    /// Analysed BPMs are folded into `[bpm_min, 2 * bpm_min)`; `None`: as analysed.
    pub bpm_min: Option<u32>,
    /// The track list's sort order (UI-defined JSON); `None`: folder order.
    pub sort: Option<String>,
}

const COLUMNS: &str =
    "id, source_path, include_sub, mode, start_at, auto_next, current_track, bpm_min, sort";

fn from_row(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        source_path: r.get(1)?,
        include_sub: r.get(2)?,
        mode: r.get(3)?,
        start_at: r.get(4)?,
        auto_next: r.get(5)?,
        current_track: r.get(6)?,
        bpm_min: r.get(7)?,
        sort: r.get(8)?,
    })
}

/// Returns the folder's session. Opening counts as use, so it moves to the top of the
/// recent list.
///
/// A folder opened for the first time starts from the most recently used session: its
/// settings (Move / Copy, Start at, Auto-next, BPM range) and its destinations with their
/// number keys, so this week's promo folder is ready to Start. Include subfolders (a
/// property of the folder) and the sort order start fresh; a destination that is the new
/// source folder itself is left out.
pub fn open(conn: &Connection, source_path: &str, now: i64) -> Result<Session> {
    let existing: Option<i64> = conn
        .query_row(
            "UPDATE sessions SET updated_at = ?2 WHERE source_path = ?1 RETURNING id",
            params![source_path, now],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return get(conn, id);
    }

    let tx = conn.unchecked_transaction()?;
    let template: Option<i64> = tx
        .query_row(
            "SELECT id FROM sessions ORDER BY updated_at DESC, id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let id: i64 = tx.query_row(
        "INSERT INTO sessions (source_path, mode, start_at, auto_next, bpm_min,
                               created_at, updated_at)
         SELECT ?1, coalesce(t.mode, 'move'), coalesce(t.start_at, 0),
                coalesce(t.auto_next, 1), t.bpm_min, ?2, ?2
         FROM (SELECT 1) LEFT JOIN sessions t ON t.id = ?3
         RETURNING id",
        params![source_path, now, template],
        |r| r.get(0),
    )?;
    if let Some(t) = template {
        tx.execute(
            "INSERT INTO destinations (session_id, path, hotkey, position)
             SELECT ?1, path, hotkey, position FROM destinations
             WHERE session_id = ?2 AND path != ?3
             ORDER BY position, id",
            params![id, t, source_path],
        )?;
    }
    tx.commit()?;
    get(conn, id)
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

/// The BPM range analysed tempos are folded into (see [`fold_bpm`]); `None` turns it off.
pub fn set_bpm_min(conn: &Connection, id: i64, value: Option<u32>, now: i64) -> Result<Session> {
    if value.is_some_and(|v| !(BPM_MIN_LOW..=BPM_MIN_HIGH).contains(&v)) {
        return Err(Error::Invalid(format!(
            "BPM range from {value:?} is out of range"
        )));
    }
    update(conn, id, "bpm_min", &value, now)
}

const BPM_MIN_LOW: u32 = 40;
const BPM_MIN_HIGH: u32 = 150;

/// Halves or doubles an analysed tempo until it falls in `[min, 2 * min)`, judged by the
/// whole number shown, as Traktor's analysis range does.
pub fn fold_bpm(bpm: f64, min: u32) -> f64 {
    let (lo, hi) = (f64::from(min), f64::from(min * 2));
    let mut b = bpm;
    if b <= 0.0 || !b.is_finite() {
        return bpm;
    }
    while b.round() >= hi {
        b /= 2.0;
    }
    while b.round() < lo {
        b *= 2.0;
    }
    b
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

/// Longest sort state accepted; a real one is a few dozen bytes.
const SORT_MAX: usize = 1024;

/// Remembers the track list's sort order (`None`: folder order).
pub fn set_sort(conn: &Connection, id: i64, sort: Option<&str>, now: i64) -> Result<Session> {
    if sort.is_some_and(|s| s.len() > SORT_MAX) {
        return Err(Error::Invalid("sort state is too long".into()));
    }
    update(conn, id, "sort", &sort, now)
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

/// Points a session at its folder's new place (moved or renamed in Finder, or a drive
/// mounted elsewhere). Every stored path under the old folder follows: tracks,
/// destinations inside it, and both ends of the undo log, so ⌘Z still puts files back.
/// Fails if another session already has `new_path`.
pub fn relocate(conn: &mut Connection, id: i64, new_path: &str, now: i64) -> Result<Session> {
    let old = get(conn, id)?.source_path;
    if old == new_path {
        return get(conn, id);
    }
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE source_path = ?1)",
        [new_path],
        |r| r.get(0),
    )?;
    if taken {
        return Err(Error::Invalid(
            "That folder already has its own session (forget it first on the Start screen).".into(),
        ));
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE sessions SET source_path = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, new_path, now],
    )?;
    // `old` itself or anything below it (`old/…`); no LIKE, folder names may hold % or _.
    let moved = |column: &str| {
        format!(
            "{column} = ?2 || substr({column}, length(?1) + 1)
             WHERE ({column} = ?1 OR substr({column}, 1, length(?1) + 1) = ?1 || '/')"
        )
    };
    tx.execute(
        &format!("UPDATE tracks SET {} AND session_id = ?3", moved("path")),
        params![old, new_path, id],
    )?;
    tx.execute(
        &format!(
            "UPDATE destinations SET {} AND session_id = ?3",
            moved("path")
        ),
        params![old, new_path, id],
    )?;
    for column in ["src", "dst"] {
        tx.execute(
            &format!("UPDATE actions SET {} AND session_id = ?3", moved(column)),
            params![old, new_path, id],
        )?;
    }
    tx.commit()?;
    get(conn, id)
}

/// How many of the session's tracks that should be in its source folder (not moved,
/// trashed or missing) would be found if the folder were at `new_path`: a sanity check
/// before relocating. Returns `(found, expected)`.
pub fn relocation_matches(conn: &Connection, id: i64, new_path: &Path) -> Result<(i64, i64)> {
    let old = get(conn, id)?.source_path;
    let mut stmt = conn.prepare(
        "SELECT path FROM tracks
         WHERE session_id = ?1 AND status IN ('pending', 'kept', 'copied')",
    )?;
    let paths = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let expected = paths.len() as i64;
    let found = paths
        .iter()
        .filter_map(|p| Path::new(p).strip_prefix(&old).ok())
        .filter(|rel| new_path.join(rel).is_file())
        .count() as i64;
    Ok((found, expected))
}

/// Decisions made in the session that are not undone (what forgetting it would lose).
pub fn decisions(conn: &Connection, id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM actions WHERE session_id = ?1 AND undone = 0",
        [id],
        |r| r.get(0),
    )?)
}

/// Forgets a session: its settings, destinations, tracks and undo log. Files are not
/// touched, and the analysis / waveform caches (tied to the files) stay.
pub fn forget(conn: &mut Connection, id: i64) -> Result<()> {
    let tx = conn.transaction()?;
    // The undo log references tracks without a cascade: it goes first.
    tx.execute("DELETE FROM actions WHERE session_id = ?1", [id])?;
    tx.execute("DELETE FROM tracks WHERE session_id = ?1", [id])?;
    tx.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn forget_removes_everything_but_the_caches() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let a = open(&conn, "/music/promo", 1).unwrap().id;
        let b = open(&conn, "/music/other", 1).unwrap().id;
        conn.execute_batch(&format!(
            "INSERT INTO destinations (session_id, path, hotkey, position) VALUES ({a}, '/d', 1, 0);
             INSERT INTO tracks (id, session_id, path, file_key) VALUES (1, {a}, '/d/x.mp3', 'k');
             INSERT INTO tracks (id, session_id, path, file_key) VALUES (2, {b}, '/o/y.mp3', 'k');
             INSERT INTO actions (session_id, track_id, kind, src, dst, ts)
               VALUES ({a}, 1, 'move', '/music/promo/x.mp3', '/d/x.mp3', 1);
             INSERT INTO bpm_overrides (file_key, bpm, created_at) VALUES ('k', 155, 0);"
        ))
        .unwrap();
        assert_eq!(decisions(&conn, a).unwrap(), 1);
        forget(&mut conn, a).unwrap();
        assert!(matches!(get(&conn, a), Err(Error::NotFound(_))));
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM tracks"), 1); // the other session's
        assert_eq!(count("SELECT count(*) FROM actions"), 0);
        assert_eq!(count("SELECT count(*) FROM destinations"), 0);
        assert_eq!(count("SELECT count(*) FROM bpm_overrides"), 1);
        // Opening the folder again starts afresh.
        assert_ne!(open(&conn, "/music/promo", 2).unwrap().id, b);
    }

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
    fn a_new_folder_starts_from_the_last_session() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let a = open(&conn, "/promo/w45", 1).unwrap().id;
        set_mode(&conn, a, "copy", 2).unwrap();
        set_start_at(&conn, a, 0.4, 3).unwrap();
        set_auto_next(&conn, a, false, 4).unwrap();
        set_bpm_min(&conn, a, Some(88), 5).unwrap();
        set_include_sub(&conn, a, true, 6).unwrap();
        set_sort(&conn, a, Some(r#"[{"id":"bpm","desc":true}]"#), 7).unwrap();
        crate::store::destinations::add(&conn, a, "/crates/peak").unwrap();
        let d2 = crate::store::destinations::add(&conn, a, "/crates/warmup")
            .unwrap()
            .id;
        crate::store::destinations::add(&conn, a, "/promo/w46").unwrap();
        crate::store::destinations::set_hotkey(&mut conn, a, d2, Some(7)).unwrap();

        let b = open(&conn, "/promo/w46", 10).unwrap();
        assert_eq!(
            (b.mode.as_str(), b.start_at, b.auto_next, b.bpm_min),
            ("copy", 0.4, false, Some(88))
        );
        assert!(!b.include_sub && b.sort.is_none() && b.current_track.is_none());
        let dests: Vec<_> = crate::store::destinations::list(&conn, b.id)
            .unwrap()
            .into_iter()
            .map(|d| (d.path, d.hotkey))
            .collect();
        // The new source itself is not its own destination.
        assert_eq!(
            dests,
            [
                ("/crates/peak".to_string(), Some(1)),
                ("/crates/warmup".to_string(), Some(7))
            ]
        );

        // Reopening keeps the folder's own settings, whatever was used last.
        set_mode(&conn, b.id, "move", 11).unwrap();
        assert_eq!(open(&conn, "/promo/w45", 12).unwrap().mode, "copy");
        assert_eq!(open(&conn, "/promo/w46", 13).unwrap().mode, "move");
    }

    #[test]
    fn sort_persists() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let id = open(&conn, "/x", 1).unwrap().id;
        let s = r#"[{"id":"title","desc":false}]"#;
        assert_eq!(
            set_sort(&conn, id, Some(s), 2).unwrap().sort.as_deref(),
            Some(s)
        );
        assert_eq!(get(&conn, id).unwrap().sort.as_deref(), Some(s));
        assert!(set_sort(&conn, id, Some(&"x".repeat(2000)), 3).is_err());
        assert_eq!(set_sort(&conn, id, None, 4).unwrap().sort, None);
    }

    #[test]
    fn relocate_moves_every_path_under_the_folder() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let a = open(&conn, "/old/promo", 1).unwrap().id;
        let other = open(&conn, "/old/promo_x", 1).unwrap().id;
        conn.execute_batch(&format!(
            "INSERT INTO destinations (session_id, path, hotkey, position) VALUES
               ({a}, '/old/promo/keep', 1, 0), ({a}, '/crates/peak', 2, 1);
             INSERT INTO tracks (id, session_id, path, file_key, status) VALUES
               (1, {a}, '/old/promo/a.mp3', 'k1', 'pending'),
               (2, {a}, '/old/promo/sub/b.mp3', 'k2', 'kept'),
               (3, {a}, '/crates/peak/c.mp3', 'k3', 'moved'),
               (4, {other}, '/old/promo_x/d.mp3', 'k4', 'pending');
             INSERT INTO actions (session_id, track_id, kind, src, dst, ts) VALUES
               ({a}, 3, 'move', '/old/promo/c.mp3', '/crates/peak/c.mp3', 1),
               ({a}, 2, 'keep', '/old/promo/sub/b.mp3', NULL, 2);"
        ))
        .unwrap();

        assert!(matches!(
            relocate(&mut conn, a, "/old/promo_x", 5),
            Err(Error::Invalid(_))
        ));
        let s = relocate(&mut conn, a, "/new/Promo 100%_", 5).unwrap();
        assert_eq!(s.source_path, "/new/Promo 100%_");

        let col = |sql: &str| -> Vec<String> {
            let mut st = conn.prepare(sql).unwrap();
            st.query_map([], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(
            col("SELECT path FROM tracks ORDER BY id"),
            [
                "/new/Promo 100%_/a.mp3",
                "/new/Promo 100%_/sub/b.mp3",
                "/crates/peak/c.mp3",
                "/old/promo_x/d.mp3" // another session, and only a name prefix
            ]
        );
        assert_eq!(
            col("SELECT path FROM destinations ORDER BY position"),
            ["/new/Promo 100%_/keep", "/crates/peak"]
        );
        assert_eq!(
            col("SELECT src FROM actions ORDER BY id"),
            ["/new/Promo 100%_/c.mp3", "/new/Promo 100%_/sub/b.mp3"]
        );
        assert_eq!(
            col("SELECT coalesce(dst, '-') FROM actions ORDER BY id"),
            ["/crates/peak/c.mp3", "-"]
        );
    }

    #[test]
    fn relocation_matches_counts_tracks_found() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.mp3"), b"x").unwrap();
        std::fs::write(dir.path().join("sub/b.mp3"), b"x").unwrap();
        let a = open(&conn, "/gone", 1).unwrap().id;
        conn.execute_batch(&format!(
            "INSERT INTO tracks (session_id, path, file_key, status) VALUES
               ({a}, '/gone/a.mp3', 'k1', 'pending'), ({a}, '/gone/sub/b.mp3', 'k2', 'copied'),
               ({a}, '/gone/c.mp3', 'k3', 'pending'), ({a}, '/d/x.mp3', 'k4', 'moved')"
        ))
        .unwrap();
        assert_eq!(relocation_matches(&conn, a, dir.path()).unwrap(), (2, 3));
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
    fn bpm_range_folds() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let id = open(&conn, "/a", 1).unwrap().id;
        assert_eq!(get(&conn, id).unwrap().bpm_min, None);
        assert_eq!(
            set_bpm_min(&conn, id, Some(88), 2).unwrap().bpm_min,
            Some(88)
        );
        assert!(set_bpm_min(&conn, id, Some(200), 2).is_err());
        assert_eq!(set_bpm_min(&conn, id, None, 2).unwrap().bpm_min, None);

        assert_eq!(fold_bpm(77.5, 88), 155.0);
        assert_eq!(fold_bpm(155.0, 70), 77.5);
        assert_eq!(fold_bpm(175.4, 88), 175.4); // shown as 175
        assert_eq!(fold_bpm(175.6, 88), 87.8); // would show as 176
        assert_eq!(fold_bpm(40.0, 120), 160.0);
        assert_eq!(fold_bpm(128.0, 100), 128.0);
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
