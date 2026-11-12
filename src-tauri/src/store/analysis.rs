//! `analysis_cache`: BPM / key results of the sidecar by `file_key`, so a file is analysed
//! once, wherever it moves or is copied to. Failures are cached too (`error`).

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;

/// What the sidecar found (its JSON output).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Analysis {
    pub bpm: f64,
    pub bpm_essentia: f64,
    pub bpm_confidence: f64,
    pub key: String,
    pub scale: String,
    pub key_strength: f64,
}

pub fn put(
    conn: &Connection,
    file_key: &str,
    result: std::result::Result<&Analysis, &str>,
    version: &str,
    now: i64,
) -> Result<()> {
    let (a, error) = match result {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(e)),
    };
    conn.execute(
        "INSERT OR REPLACE INTO analysis_cache
           (file_key, bpm, bpm_essentia, bpm_confidence, musical_key, scale, key_strength,
            analyzer_version, created_at, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            file_key,
            a.map(|a| a.bpm),
            a.map(|a| a.bpm_essentia),
            a.map(|a| a.bpm_confidence),
            a.map(|a| a.key.as_str()),
            a.map(|a| a.scale.as_str()),
            a.map(|a| a.key_strength),
            version,
            now,
            error,
        ],
    )?;
    Ok(())
}

/// Whether the file already has a result (or a failure) of this analyzer version.
pub fn has_current(conn: &Connection, file_key: &str, version: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM analysis_cache WHERE file_key = ?1 AND analyzer_version = ?2",
            params![file_key, version],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The BPM set by hand for this file, if any.
pub fn bpm_override(conn: &Connection, file_key: &str) -> Result<Option<f64>> {
    Ok(conn
        .query_row(
            "SELECT bpm FROM bpm_overrides WHERE file_key = ?1",
            [file_key],
            |r| r.get(0),
        )
        .optional()?)
}

/// Sets (or with `None` removes) the hand-set BPM of a file.
pub fn set_bpm_override(
    conn: &Connection,
    file_key: &str,
    bpm: Option<f64>,
    now: i64,
) -> Result<()> {
    match bpm {
        Some(b) => conn.execute(
            "INSERT OR REPLACE INTO bpm_overrides (file_key, bpm, created_at) VALUES (?1, ?2, ?3)",
            params![file_key, b, now],
        )?,
        None => conn.execute("DELETE FROM bpm_overrides WHERE file_key = ?1", [file_key])?,
    };
    Ok(())
}

/// A file whose content key changed but whose audio did not (a title or artist tag was
/// written) keeps its result and its hand-set BPM under the new key.
pub fn carry_over(conn: &Connection, old_key: &str, new_key: &str) -> Result<()> {
    if old_key != new_key {
        conn.execute(
            "INSERT OR IGNORE INTO bpm_overrides (file_key, bpm, created_at)
             SELECT ?2, bpm, created_at FROM bpm_overrides WHERE file_key = ?1",
            params![old_key, new_key],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO analysis_cache
               (file_key, bpm, bpm_essentia, bpm_confidence, musical_key, scale, key_strength,
                analyzer_version, created_at, error)
             SELECT ?2, bpm, bpm_essentia, bpm_confidence, musical_key, scale, key_strength,
                    analyzer_version, created_at, error
             FROM analysis_cache WHERE file_key = ?1",
            params![old_key, new_key],
        )?;
    }
    Ok(())
}

/// Essentia's key and scale as DJ software shows it: `A` + `minor` → `Am`, `F#` + `major`
/// → `F#`.
pub fn key_name(key: &str, scale: Option<&str>) -> String {
    match scale {
        Some("minor") => format!("{key}m"),
        _ => key.to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub track_id: i64,
    pub path: String,
    pub file_key: String,
}

/// The session's tracks that lack a BPM or a key in their tags and have no result of this
/// analyzer version yet. Trashed and missing tracks are left out (no file to read). The
/// current track comes first, then the ones still to decide, each in folder order.
pub fn wanted(conn: &Connection, session_id: i64, version: &str) -> Result<Vec<Job>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.path, t.file_key FROM tracks t
         JOIN sessions s ON s.id = t.session_id
         WHERE t.session_id = ?1
           AND (t.bpm IS NULL OR t.musical_key IS NULL)
           AND t.status NOT IN ('trashed', 'missing')
           AND NOT EXISTS (SELECT 1 FROM analysis_cache a
                           WHERE a.file_key = t.file_key AND a.analyzer_version = ?2)
         ORDER BY t.id IS s.current_track DESC, t.status = 'pending' DESC,
                  t.path COLLATE NOCASE, t.id",
    )?;
    let rows = stmt.query_map(params![session_id, version], |r| {
        Ok(Job {
            track_id: r.get(0)?,
            path: r.get(1)?,
            file_key: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tags::TrackMeta;
    use crate::store::{Store, sessions, tracks};

    fn analysis(bpm: f64) -> Analysis {
        Analysis {
            bpm,
            bpm_essentia: bpm - 0.4,
            bpm_confidence: 3.5,
            key: "A".into(),
            scale: "minor".into(),
            key_strength: 0.8,
        }
    }

    fn track(
        path: &str,
        key: &str,
        bpm: Option<f64>,
        musical_key: Option<&str>,
    ) -> tracks::NewTrack {
        tracks::NewTrack {
            path: path.into(),
            file_key: key.into(),
            meta: TrackMeta {
                bpm,
                musical_key: musical_key.map(Into::into),
                format: "MP3".into(),
                ..TrackMeta::default()
            },
        }
    }

    fn ids(jobs: &[Job]) -> Vec<&str> {
        jobs.iter().map(|j| j.path.as_str()).collect()
    }

    #[test]
    fn wanted_skips_tagged_cached_and_gone() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 0).unwrap();
        tracks::upsert(
            &mut conn,
            s.id,
            &[
                track("/src/a.mp3", "ka", Some(124.0), Some("Am")), // fully tagged
                track("/src/b.mp3", "kb", Some(124.0), None),       // key missing
                track("/src/c.mp3", "kc", None, None),
                track("/src/d.mp3", "kd", None, None), // cached below
                track("/src/e.mp3", "ke", None, None), // trashed below
            ],
        )
        .unwrap();
        put(&conn, "kd", Ok(&analysis(126.0)), "1", 0).unwrap();
        conn.execute(
            "UPDATE tracks SET status = 'trashed' WHERE path = '/src/e.mp3'",
            [],
        )
        .unwrap();

        assert_eq!(
            ids(&wanted(&conn, s.id, "1").unwrap()),
            ["/src/b.mp3", "/src/c.mp3"]
        );
        // A new analyzer version wants the cached one again.
        assert_eq!(
            ids(&wanted(&conn, s.id, "2").unwrap()),
            ["/src/b.mp3", "/src/c.mp3", "/src/d.mp3"]
        );
        // A cached failure counts as done.
        put(&conn, "kc", Err("too little audio"), "1", 0).unwrap();
        assert_eq!(ids(&wanted(&conn, s.id, "1").unwrap()), ["/src/b.mp3"]);
        assert!(has_current(&conn, "kc", "1").unwrap());
        assert!(!has_current(&conn, "kc", "2").unwrap());
    }

    #[test]
    fn current_then_pending_first() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 0).unwrap();
        tracks::upsert(
            &mut conn,
            s.id,
            &[
                track("/src/a.mp3", "ka", None, None),
                track("/src/b.mp3", "kb", None, None),
                track("/src/c.mp3", "kc", None, None),
            ],
        )
        .unwrap();
        conn.execute(
            "UPDATE tracks SET status = 'kept' WHERE path = '/src/a.mp3'",
            [],
        )
        .unwrap();
        let c: i64 = conn
            .query_row("SELECT id FROM tracks WHERE path = '/src/c.mp3'", [], |r| {
                r.get(0)
            })
            .unwrap();
        sessions::set_current_track(&conn, s.id, Some(c)).unwrap();
        assert_eq!(
            ids(&wanted(&conn, s.id, "1").unwrap()),
            ["/src/c.mp3", "/src/b.mp3", "/src/a.mp3"]
        );
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name("A", Some("minor")), "Am");
        assert_eq!(key_name("F#", Some("major")), "F#");
        assert_eq!(key_name("Eb", None), "Eb");
    }

    #[test]
    fn list_fills_in_from_the_analysis() {
        let store = Store::open_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let s = sessions::open(&conn, "/src", 0).unwrap();
        tracks::upsert(
            &mut conn,
            s.id,
            &[
                track("/src/a.mp3", "ka", Some(128.0), None),
                track("/src/b.mp3", "kb", None, Some("8A")),
                track("/src/c.mp3", "kc", None, None),
            ],
        )
        .unwrap();
        for k in ["ka", "kb"] {
            put(&conn, k, Ok(&analysis(123.96)), "1", 0).unwrap();
        }
        put(&conn, "kc", Err("decode"), "1", 0).unwrap();
        let list = tracks::list(&conn, s.id).unwrap();
        let got: Vec<_> = list
            .iter()
            .map(|t| {
                (
                    t.bpm,
                    t.musical_key.as_deref(),
                    t.bpm_source,
                    t.key_analyzed,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (Some(128.0), Some("Am"), Some("tag"), true), // the tag BPM wins
                (Some(124.0), Some("8A"), Some("analysis"), false), // the tag key wins
                (None, None, None, false),                    // failed: nothing to show
            ]
        );
        // The session's BPM range folds the analysed tempo, not the tag.
        sessions::set_bpm_min(&conn, s.id, Some(50), 0).unwrap();
        let list = tracks::list(&conn, s.id).unwrap();
        assert_eq!((list[0].bpm, list[1].bpm), (Some(128.0), Some(62.0)));
        sessions::set_bpm_min(&conn, s.id, None, 0).unwrap();
        // A hand-set BPM wins over the tag and the analysis.
        for k in ["ka", "kb", "kc"] {
            set_bpm_override(&conn, k, Some(155.0), 0).unwrap();
        }
        let list = tracks::list(&conn, s.id).unwrap();
        assert!(
            list.iter()
                .all(|t| t.bpm == Some(155.0) && t.bpm_source == Some("manual"))
        );
        set_bpm_override(&conn, "ka", None, 0).unwrap();
        assert_eq!(tracks::list(&conn, s.id).unwrap()[0].bpm, Some(128.0));
    }

    #[test]
    fn carry_over_keeps_the_result() {
        let store = Store::open_in_memory().unwrap();
        let conn = store.conn().unwrap();
        put(&conn, "old", Ok(&analysis(124.0)), "1", 0).unwrap();
        set_bpm_override(&conn, "old", Some(155.0), 0).unwrap();
        carry_over(&conn, "old", "new").unwrap();
        assert!(has_current(&conn, "new", "1").unwrap());
        assert_eq!(bpm_override(&conn, "new").unwrap(), Some(155.0));
        carry_over(&conn, "none", "other").unwrap();
        assert!(!has_current(&conn, "other", "1").unwrap());
    }
}
