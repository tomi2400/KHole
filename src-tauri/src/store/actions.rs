//! The undo log: every decision on a track, with where its file went, and every edit
//! (rename, title / artist tag, BPM) made in the list.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Copy,
    Trash,
    Keep,
    /// The file got a new name (`src` → `dst`, same folder).
    Rename,
    /// One text field of the tags changed (`field`, `old_value` → `new_value`).
    Tag,
    /// The BPM was set by hand, in KHole only (`old_value` → `new_value`; `None` = none).
    Bpm,
}

impl Kind {
    /// Edits from the list (undone by `fileops::edit::undo`), not decisions.
    pub fn is_edit(self) -> bool {
        matches!(self, Kind::Rename | Kind::Tag | Kind::Bpm)
    }
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Move => "move",
            Kind::Copy => "copy",
            Kind::Trash => "trash",
            Kind::Keep => "keep",
            Kind::Rename => "rename",
            Kind::Tag => "tag",
            Kind::Bpm => "bpm",
        }
    }

    fn parse(s: &str) -> rusqlite::Result<Self> {
        Ok(match s {
            "move" => Kind::Move,
            "copy" => Kind::Copy,
            "trash" => Kind::Trash,
            "keep" => Kind::Keep,
            "rename" => Kind::Rename,
            "tag" => Kind::Tag,
            "bpm" => Kind::Bpm,
            other => {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    format!("unknown action kind {other}").into(),
                ));
            }
        })
    }

    /// The track status after this action; edits leave it as it was.
    pub fn status(self) -> Option<&'static str> {
        match self {
            Kind::Move => Some("moved"),
            Kind::Copy => Some("copied"),
            Kind::Trash => Some("trashed"),
            Kind::Keep => Some("kept"),
            Kind::Rename | Kind::Tag | Kind::Bpm => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: i64,
    pub track_id: i64,
    pub kind: Kind,
    /// Where the file was.
    pub src: String,
    /// Where it went (the copy, or the path inside the Trash); `None` for keep.
    pub dst: Option<String>,
    pub prev_status: String,
    pub destination_id: Option<i64>,
    /// The destination already had this track; nothing was copied there.
    pub duplicate: bool,
    /// A tag edit: which field (`title`, `artist`) and its value before.
    pub field: Option<String>,
    pub old_value: Option<String>,
}

pub struct NewAction<'a> {
    pub session_id: i64,
    pub track_id: i64,
    pub kind: Kind,
    pub src: &'a str,
    pub dst: Option<&'a str>,
    pub renamed: bool,
    pub prev_status: &'a str,
    pub destination_id: Option<i64>,
    pub duplicate: bool,
    /// Tag edits only.
    pub field: Option<&'a str>,
    pub old_value: Option<&'a str>,
    pub new_value: Option<&'a str>,
}

pub fn record(conn: &Connection, a: &NewAction, ts: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO actions (session_id, track_id, kind, src, dst, renamed, ts, prev_status,
                              destination_id, duplicate, field, old_value, new_value)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            a.session_id,
            a.track_id,
            a.kind.as_str(),
            a.src,
            a.dst,
            a.renamed,
            ts,
            a.prev_status,
            a.destination_id,
            a.duplicate,
            a.field,
            a.old_value,
            a.new_value
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The newest action of the session that is not undone yet.
pub fn last_undoable(conn: &Connection, session_id: i64) -> Result<Option<Action>> {
    Ok(conn
        .query_row(
            "SELECT id, track_id, kind, src, dst, prev_status, destination_id, duplicate, field,
                    old_value
             FROM actions
             WHERE session_id = ?1 AND undone = 0 AND track_id IS NOT NULL
             ORDER BY id DESC LIMIT 1",
            [session_id],
            |r| {
                Ok(Action {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    kind: Kind::parse(&r.get::<_, String>(2)?)?,
                    src: r.get(3)?,
                    dst: r.get(4)?,
                    prev_status: r.get(5)?,
                    destination_id: r.get(6)?,
                    duplicate: r.get(7)?,
                    field: r.get(8)?,
                    old_value: r.get(9)?,
                })
            },
        )
        .optional()?)
}

pub fn mark_undone(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE actions SET undone = 1 WHERE id = ?1", [id])?;
    Ok(())
}

/// Closes the open actions of a track whose file came back on its own (Put Back from the
/// Trash, dragged back in Finder): there is nothing left to undo for them.
pub fn close_open(conn: &Connection, track_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE actions SET undone = 1 WHERE track_id = ?1 AND undone = 0",
        [track_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Store, sessions};

    #[test]
    fn last_undoable_walks_back() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 1).unwrap().id;
        conn.execute_batch(&format!(
            "INSERT INTO tracks (id, session_id, path, file_key) VALUES (1, {s}, '/src/a', 'k')"
        ))
        .unwrap();
        let new = |kind, dst| NewAction {
            session_id: s,
            track_id: 1,
            kind,
            src: "/src/a",
            dst,
            renamed: false,
            prev_status: "pending",
            destination_id: None,
            duplicate: false,
            field: None,
            old_value: None,
            new_value: None,
        };
        assert_eq!(last_undoable(&conn, s).unwrap(), None);
        let a = record(&conn, &new(Kind::Keep, None), 1).unwrap();
        let b = record(&conn, &new(Kind::Trash, Some("/T/a")), 2).unwrap();
        let last = last_undoable(&conn, s).unwrap().unwrap();
        assert_eq!(
            (last.id, last.kind, last.dst.as_deref()),
            (b, Kind::Trash, Some("/T/a"))
        );
        mark_undone(&conn, b).unwrap();
        assert_eq!(last_undoable(&conn, s).unwrap().unwrap().id, a);
        mark_undone(&conn, a).unwrap();
        assert_eq!(last_undoable(&conn, s).unwrap(), None);
    }
}
