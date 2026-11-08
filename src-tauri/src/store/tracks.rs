//! A session's track rows.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::library::tags::TrackMeta;
use crate::store::actions;

pub struct NewTrack {
    pub path: String,
    pub file_key: String,
    pub meta: TrackMeta,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub bpm: Option<f64>,
    pub musical_key: Option<String>,
    pub duration: Option<f64>,
    pub format: Option<String>,
    pub bitrate: Option<i64>,
    pub status: String,
}

/// Inserts new rows and refreshes the tags of known ones in one transaction. A `missing`
/// track whose file is back becomes `pending` again; other statuses are left alone.
pub fn upsert(conn: &mut Connection, session_id: i64, rows: &[NewTrack]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO tracks (session_id, path, file_key, title, artist, bpm, musical_key,
                                 duration, format, bitrate, bpm_source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (session_id, path) DO UPDATE SET
               file_key = excluded.file_key, title = excluded.title, artist = excluded.artist,
               bpm = excluded.bpm, musical_key = excluded.musical_key,
               duration = excluded.duration, format = excluded.format,
               bitrate = excluded.bitrate, bpm_source = excluded.bpm_source,
               status = CASE status WHEN 'missing' THEN 'pending' ELSE status END",
        )?;
        for t in rows {
            let m = &t.meta;
            stmt.execute(params![
                session_id,
                t.path,
                t.file_key,
                m.title,
                m.artist,
                m.bpm,
                m.musical_key,
                m.duration,
                m.format,
                m.bitrate,
                m.bpm.map(|_| "tag"),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn ids_by_path(conn: &Connection, session_id: i64) -> Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT path, id FROM tracks WHERE session_id = ?1")?;
    let rows = stmt.query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Deletes the given rows unless the undo log references them; returns how many went.
pub fn remove_unreferenced(conn: &mut Connection, ids: &[i64]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut removed = 0;
    {
        let mut stmt = tx.prepare(
            "DELETE FROM tracks WHERE id = ?1
               AND NOT EXISTS (SELECT 1 FROM actions WHERE track_id = ?1)",
        )?;
        for id in ids {
            removed += stmt.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(removed)
}

pub fn count(conn: &Connection, session_id: i64) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM tracks WHERE session_id = ?1",
        [session_id],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

pub fn file_key(conn: &Connection, id: i64) -> Result<String> {
    conn.query_row("SELECT file_key FROM tracks WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("track {id}")))
}

/// The current path of a track.
pub fn path(conn: &Connection, id: i64) -> Result<String> {
    conn.query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("track {id}")))
}

const COLUMNS: &str =
    "id, path, title, artist, bpm, musical_key, duration, format, bitrate, status";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        bpm: r.get(4)?,
        musical_key: r.get(5)?,
        duration: r.get(6)?,
        format: r.get(7)?,
        bitrate: r.get(8)?,
        status: r.get(9)?,
    })
}

/// The session's tracks in folder order (case-insensitive path). A moved or trashed track
/// keeps its place: it sorts by where it was before its first action.
pub fn list(conn: &Connection, session_id: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM tracks WHERE session_id = ?1
         ORDER BY coalesce(
                    (SELECT a.src FROM actions a WHERE a.track_id = tracks.id ORDER BY a.id LIMIT 1),
                    path) COLLATE NOCASE,
                  id"
    ))?;
    let rows = stmt.query_map([session_id], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A track of the session (a track of another session is not found).
pub fn get(conn: &Connection, session_id: i64, id: i64) -> Result<Track> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM tracks WHERE id = ?1 AND session_id = ?2"),
        params![id, session_id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("track {id}")))
}

/// Records where a track's file is now and what happened to it. A stale row that still
/// claims the new path (the file was not there, we just created it) gives way, unless the
/// undo log needs it.
pub fn set_location(conn: &Connection, id: i64, path: &str, status: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM tracks
         WHERE path = ?2 AND id != ?1
           AND session_id = (SELECT session_id FROM tracks WHERE id = ?1)
           AND NOT EXISTS (SELECT 1 FROM actions WHERE track_id = tracks.id)",
        params![id, path],
    )?;
    conn.execute(
        "UPDATE tracks SET path = ?2, status = ?3 WHERE id = ?1",
        params![id, path, status],
    )?;
    Ok(())
}

/// A trashed or moved track whose file came back into the source (Put Back from the Trash,
/// dragged back in Finder) is found by the scan as a new row. The two are merged: the
/// original row (`o`) takes the new row's (`f`) path, status and actions, and its own open
/// actions are closed, as there is nothing left to undo for them.
///
/// A pair qualifies only if `o`'s recorded file is gone (`exists` is false: a moved track
/// whose file is still in the destination was copied back, and stays two tracks), both have
/// the same content key, and `f` sits exactly where one of `o`'s actions took the file from.
/// Returns how many tracks came back.
pub fn reattach_returned(
    conn: &mut Connection,
    session_id: i64,
    exists: impl Fn(&str) -> bool,
) -> Result<usize> {
    let pairs: Vec<(i64, String, i64)> = conn
        .prepare(
            "SELECT o.id, o.path, f.id FROM tracks o
             JOIN tracks f ON f.session_id = o.session_id AND f.file_key = o.file_key
                          AND f.id != o.id
             WHERE o.session_id = ?1 AND o.status IN ('trashed', 'moved', 'missing')
               AND f.path IN (SELECT a.src FROM actions a WHERE a.track_id = o.id)
             ORDER BY o.id, f.id",
        )?
        .query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let tx = conn.transaction()?;
    let mut used = HashSet::new();
    for (orig, orig_path, fresh) in pairs {
        if used.contains(&orig) || used.contains(&fresh) || exists(&orig_path) {
            continue;
        }
        used.insert(orig);
        used.insert(fresh);
        actions::close_open(&tx, orig)?;
        tx.execute(
            "UPDATE actions SET track_id = ?1 WHERE track_id = ?2",
            [orig, fresh],
        )?;
        tx.execute(
            "UPDATE sessions SET current_track = ?1 WHERE current_track = ?2",
            [orig, fresh],
        )?;
        let (path, status): (String, String) = tx.query_row(
            "DELETE FROM tracks WHERE id = ?1 RETURNING path, status",
            [fresh],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute(
            "UPDATE tracks SET path = ?2, status = ?3 WHERE id = ?1",
            params![orig, path, status],
        )?;
    }
    tx.commit()?;
    Ok(used.len() / 2)
}
