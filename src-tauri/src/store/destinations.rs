//! A session's destination folders and their number keys (1–9, then 0 for the 10th).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};

/// The number keys in keyboard order.
pub const HOTKEYS: [i64; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: i64,
    pub path: String,
    /// 1–9 or 0; `None` = reachable only from Move to… (M6).
    pub hotkey: Option<i64>,
    pub position: i64,
    /// Tracks sorted into it in this session (undone actions excluded).
    pub count: i64,
    /// False if the folder is gone (moved, deleted) or its drive is not mounted.
    pub available: bool,
}

pub fn list(conn: &Connection, session_id: i64) -> Result<Vec<Destination>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.path, d.hotkey, d.position,
                (SELECT count(*) FROM actions a WHERE a.destination_id = d.id AND a.undone = 0)
         FROM destinations d WHERE d.session_id = ?1
         ORDER BY d.position, d.id",
    )?;
    let rows = stmt.query_map([session_id], |r| {
        let path: String = r.get(1)?;
        Ok(Destination {
            id: r.get(0)?,
            available: Path::new(&path).is_dir(),
            path,
            hotkey: r.get(2)?,
            position: r.get(3)?,
            count: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(conn: &Connection, session_id: i64, id: i64) -> Result<Destination> {
    list(conn, session_id)?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| Error::NotFound(format!("destination {id}")))
}

/// Appends a folder and gives it the first free number key, if any is left.
pub fn add(conn: &Connection, session_id: i64, path: &str) -> Result<Destination> {
    let taken: Vec<i64> = conn
        .prepare("SELECT hotkey FROM destinations WHERE session_id = ?1 AND hotkey IS NOT NULL")?
        .query_map([session_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let hotkey = HOTKEYS.into_iter().find(|k| !taken.contains(k));
    let inserted = conn
        .query_row(
            "INSERT INTO destinations (session_id, path, hotkey, position)
             VALUES (?1, ?2, ?3,
                     (SELECT coalesce(max(position), -1) + 1 FROM destinations
                      WHERE session_id = ?1))
             ON CONFLICT (session_id, path) DO NOTHING
             RETURNING id",
            params![session_id, path, hotkey],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    match inserted {
        Some(id) => get(conn, session_id, id),
        None => Err(Error::Invalid(
            "That folder is already a destination.".into(),
        )),
    }
}

pub fn remove(conn: &Connection, session_id: i64, id: i64) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM destinations WHERE id = ?1 AND session_id = ?2",
        params![id, session_id],
    )?;
    if n == 0 {
        return Err(Error::NotFound(format!("destination {id}")));
    }
    Ok(())
}

/// Sets a destination's number key. If another destination had that key, the two swap,
/// so a key never points to two folders.
pub fn set_hotkey(
    conn: &mut Connection,
    session_id: i64,
    id: i64,
    hotkey: Option<i64>,
) -> Result<()> {
    if hotkey.is_some_and(|k| !HOTKEYS.contains(&k)) {
        return Err(Error::Invalid(format!("hotkey {hotkey:?} is not 0–9")));
    }
    let tx = conn.transaction()?;
    let old: Option<i64> = tx
        .query_row(
            "SELECT hotkey FROM destinations WHERE id = ?1 AND session_id = ?2",
            params![id, session_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("destination {id}")))?;
    if let Some(k) = hotkey {
        tx.execute(
            "UPDATE destinations SET hotkey = ?3 WHERE session_id = ?1 AND hotkey = ?2 AND id != ?4",
            params![session_id, k, old, id],
        )?;
    }
    tx.execute(
        "UPDATE destinations SET hotkey = ?2 WHERE id = ?1",
        params![id, hotkey],
    )?;
    tx.commit()?;
    Ok(())
}

/// Destination folders of a session, for scoping and for keeping them out of the scan.
pub fn paths(conn: &Connection, session_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM destinations WHERE session_id = ?1")?;
    let rows = stmt.query_map([session_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Store, sessions};

    fn keys(conn: &Connection, s: i64) -> Vec<(String, Option<i64>)> {
        list(conn, s)
            .unwrap()
            .into_iter()
            .map(|d| (d.path, d.hotkey))
            .collect()
    }

    #[test]
    fn add_assigns_keys_in_keyboard_order() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 1).unwrap().id;
        for i in 0..11 {
            add(&conn, s, &format!("/d{i}")).unwrap();
        }
        let k: Vec<_> = keys(&conn, s).into_iter().map(|(_, k)| k).collect();
        assert_eq!(
            k,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]
                .map(Some)
                .into_iter()
                .chain([None])
                .collect::<Vec<_>>()
        );
        assert!(matches!(add(&conn, s, "/d3"), Err(Error::Invalid(_))));

        // A freed key is reused by the next folder.
        let d2 = list(&conn, s).unwrap()[1].id;
        remove(&conn, s, d2).unwrap();
        assert_eq!(add(&conn, s, "/new").unwrap().hotkey, Some(2));
    }

    #[test]
    fn set_hotkey_swaps() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 1).unwrap().id;
        let a = add(&conn, s, "/a").unwrap().id;
        let b = add(&conn, s, "/b").unwrap().id;
        set_hotkey(&mut conn, s, b, Some(1)).unwrap();
        assert_eq!(
            keys(&conn, s),
            [("/a".into(), Some(2)), ("/b".into(), Some(1))]
        );
        set_hotkey(&mut conn, s, a, None).unwrap();
        assert_eq!(keys(&conn, s)[0].1, None);
        assert!(set_hotkey(&mut conn, s, a, Some(10)).is_err());
        assert!(set_hotkey(&mut conn, s, 99, Some(3)).is_err());
    }

    #[test]
    fn destinations_belong_to_their_session() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        let s1 = sessions::open(&conn, "/s1", 1).unwrap().id;
        let s2 = sessions::open(&conn, "/s2", 1).unwrap().id;
        let d = add(&conn, s1, "/d").unwrap().id;
        assert!(get(&conn, s2, d).is_err());
        assert!(remove(&conn, s2, d).is_err());
        assert_eq!(add(&conn, s2, "/d").unwrap().hotkey, Some(1));
        assert_eq!(paths(&conn, s1).unwrap(), ["/d"]);
    }
}
