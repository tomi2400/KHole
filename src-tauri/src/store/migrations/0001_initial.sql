-- One session per source folder.
CREATE TABLE sessions (
  id            INTEGER PRIMARY KEY,
  source_path   TEXT NOT NULL UNIQUE,
  include_sub   INTEGER NOT NULL DEFAULT 0,
  mode          TEXT NOT NULL DEFAULT 'move' CHECK (mode IN ('move', 'copy')),
  start_at      REAL NOT NULL DEFAULT 0,          -- 0 / 0.25 / 0.40
  auto_next     INTEGER NOT NULL DEFAULT 1,
  current_track INTEGER,                          -- for resuming
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE destinations (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  hotkey     INTEGER CHECK (hotkey BETWEEN 0 AND 9),  -- 1..9, 0 (= 10th); NULL = Move to… only
  position   INTEGER NOT NULL,
  UNIQUE (session_id, path)
);

CREATE TABLE tracks (
  id          INTEGER PRIMARY KEY,
  session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  path        TEXT NOT NULL,                      -- current location
  file_key    TEXT NOT NULL,                      -- size + xxh3 of first/last 64 KB (no mtime)
  title       TEXT,
  artist      TEXT,
  bpm         REAL,
  musical_key TEXT,
  duration    REAL,
  format      TEXT,
  bitrate     INTEGER,
  bpm_source  TEXT CHECK (bpm_source IN ('tag', 'analysis')),
  status      TEXT NOT NULL DEFAULT 'pending'
              CHECK (status IN ('pending', 'kept', 'moved', 'copied', 'trashed', 'missing')),
  UNIQUE (session_id, path)
);
CREATE INDEX tracks_file_key ON tracks(file_key);

-- Undo log.
CREATE TABLE actions (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  track_id   INTEGER REFERENCES tracks(id),
  kind       TEXT NOT NULL CHECK (kind IN ('move', 'copy', 'trash', 'keep')),
  src        TEXT NOT NULL,
  dst        TEXT,                                -- destination, or the path inside the Trash
  renamed    INTEGER NOT NULL DEFAULT 0,          -- got a "(2)" suffix because of a conflict
  ts         INTEGER NOT NULL,
  undone     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX actions_session ON actions(session_id, id);

-- Tied to the file, not the session, so it survives moves and copies.
CREATE TABLE analysis_cache (
  file_key         TEXT PRIMARY KEY,
  bpm              REAL,
  bpm_essentia     REAL,
  bpm_confidence   REAL,
  musical_key      TEXT,
  scale            TEXT,
  key_strength     REAL,
  analyzer_version TEXT NOT NULL,
  created_at       INTEGER NOT NULL
);

CREATE TABLE peaks_cache (
  file_key TEXT PRIMARY KEY,
  per_sec  INTEGER NOT NULL,
  duration REAL NOT NULL,
  data     BLOB NOT NULL                          -- min/max pairs
);
