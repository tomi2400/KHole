//! A session's track rows.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::library::tags::TrackMeta;
use crate::store::{actions, analysis, sessions};

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
    /// Where the BPM comes from: `manual` (set in KHole), `tag` or `analysis`.
    pub bpm_source: Option<&'static str>,
    /// The key comes from the analysis (the tags have none).
    pub key_analyzed: bool,
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

/// Track columns, with the analysis result (by content key) filling in a missing BPM / key:
/// the tags win over the analysis. A BPM set by hand wins over both.
const COLUMNS: &str = "tracks.id, tracks.path, tracks.title, tracks.artist, tracks.bpm,
     tracks.musical_key, tracks.duration, tracks.format, tracks.bitrate, tracks.status,
     an.bpm, an.musical_key, an.scale, ov.bpm, s.bpm_min";
const FROM: &str = "tracks JOIN sessions s ON s.id = tracks.session_id
                    LEFT JOIN analysis_cache an ON an.file_key = tracks.file_key
                    LEFT JOIN bpm_overrides ov ON ov.file_key = tracks.file_key";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    let tag_bpm: Option<f64> = r.get(4)?;
    let tag_key: Option<String> = r.get(5)?;
    // Folded into the session's range, if it has one; one decimal is what the analysis can
    // tell (the fit is ±0.01 on steady tracks).
    let range: Option<u32> = r.get(14)?;
    let an_bpm = r
        .get::<_, Option<f64>>(10)?
        .map(|b| range.map_or(b, |min| sessions::fold_bpm(b, min)))
        .map(|b| (b * 10.0).round() / 10.0);
    let an_key = r
        .get::<_, Option<String>>(11)?
        .map(|k| analysis::key_name(&k, r.get::<_, Option<String>>(12).ok().flatten().as_deref()));
    let manual: Option<f64> = r.get(13)?;
    let (bpm, bpm_source) = match (manual, tag_bpm, an_bpm) {
        (Some(b), _, _) => (Some(b), Some("manual")),
        (None, Some(b), _) => (Some(b), Some("tag")),
        (None, None, Some(b)) => (Some(b), Some("analysis")),
        _ => (None, None),
    };
    Ok(Track {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        key_analyzed: tag_key.is_none() && an_key.is_some(),
        bpm,
        bpm_source,
        musical_key: tag_key.or(an_key),
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
        "SELECT {COLUMNS} FROM {FROM} WHERE tracks.session_id = ?1
         ORDER BY coalesce(
                    (SELECT a.src FROM actions a WHERE a.track_id = tracks.id ORDER BY a.id LIMIT 1),
                    path) COLLATE NOCASE,
                  tracks.id"
    ))?;
    let rows = stmt.query_map([session_id], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A track of the session (a track of another session is not found).
pub fn get(conn: &Connection, session_id: i64, id: i64) -> Result<Track> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM {FROM} WHERE tracks.id = ?1 AND tracks.session_id = ?2"),
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

/// Refreshes a track's content key and tags after its file was edited (tag written). The
/// audio did not change, so the analysis result moves along to the new key.
pub fn refresh(conn: &Connection, id: i64, file_key: &str, m: &TrackMeta) -> Result<()> {
    let old_key = self::file_key(conn, id)?;
    analysis::carry_over(conn, &old_key, file_key)?;
    conn.execute(
        "UPDATE tracks SET file_key = ?2, title = ?3, artist = ?4, bpm = ?5, musical_key = ?6,
                duration = ?7, format = ?8, bitrate = ?9
         WHERE id = ?1",
        params![
            id,
            file_key,
            m.title,
            m.artist,
            m.bpm,
            m.musical_key,
            m.duration,
            m.format,
            m.bitrate
        ],
    )?;
    Ok(())
}

/// Statuses whose file belongs in the source folder.
const IN_SOURCE: &str = "'pending', 'kept', 'copied', 'missing'";

/// A file renamed or moved within the source in Finder is found by the scan as a new row
/// (`fresh`), while its old row is left without a file (`stale`). The two are merged: the
/// old row takes the new path, so its status, history and place in the player survive; a
/// `missing` track becomes `pending` again.
///
/// A pair qualifies only if the old row's file is gone (`exists` is false), its status says
/// the file belongs in the source, both have the same content key, and the new row has no
/// history of its own. Each row pairs at most once. Returns the ids of the old rows merged.
pub fn relink_renamed(
    conn: &mut Connection,
    stale: &[i64],
    fresh: &[i64],
    exists: impl Fn(&str) -> bool,
) -> Result<Vec<i64>> {
    if stale.is_empty() || fresh.is_empty() {
        return Ok(Vec::new());
    }
    let key_of =
        |conn: &Connection, ids: &[i64], extra: &str| -> Result<Vec<(i64, String, String)>> {
            let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            let mut stmt = conn.prepare(&format!(
                "SELECT id, path, file_key FROM tracks WHERE id IN ({list}) {extra} ORDER BY id"
            ))?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
    let olds = key_of(conn, stale, &format!("AND status IN ({IN_SOURCE})"))?;
    let news = key_of(
        conn,
        fresh,
        "AND NOT EXISTS (SELECT 1 FROM actions WHERE track_id = tracks.id)",
    )?;

    let tx = conn.transaction()?;
    let mut used = HashSet::new();
    let mut merged = Vec::new();
    for (old, old_path, key) in &olds {
        if exists(old_path) {
            continue;
        }
        let Some((new, new_path, _)) = news
            .iter()
            .find(|(id, _, k)| k == key && !used.contains(id))
        else {
            continue;
        };
        used.insert(*new);
        tx.execute(
            "UPDATE sessions SET current_track = ?1 WHERE current_track = ?2",
            [old, new],
        )?;
        tx.execute("DELETE FROM tracks WHERE id = ?1", [new])?;
        tx.execute(
            "UPDATE tracks SET path = ?2,
                    status = CASE status WHEN 'missing' THEN 'pending' ELSE status END
             WHERE id = ?1",
            params![old, new_path],
        )?;
        merged.push(*old);
    }
    tx.commit()?;
    Ok(merged)
}

/// Rows whose file should be in the source but is gone, and that the undo log keeps (so
/// they were not removed), become `missing`. Returns how many.
pub fn mark_missing(
    conn: &Connection,
    ids: &[i64],
    exists: impl Fn(&str) -> bool,
) -> Result<usize> {
    let mut n = 0;
    let mut stmt = conn.prepare(
        "SELECT path FROM tracks WHERE id = ?1 AND status IN ('pending', 'kept', 'copied')",
    )?;
    for id in ids {
        let path: Option<String> = stmt.query_row([id], |r| r.get(0)).optional()?;
        if path.is_some_and(|p| !exists(&p)) {
            n += conn.execute("UPDATE tracks SET status = 'missing' WHERE id = ?1", [id])?;
        }
    }
    Ok(n)
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
