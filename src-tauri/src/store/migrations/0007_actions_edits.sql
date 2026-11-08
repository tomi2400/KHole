-- Edits from the track list join the undo log: `rename` (src = old path, dst = new path)
-- and `tag` (one text field of a track's tags; `field`, `old_value`, `new_value`). The
-- CHECK on `kind` cannot be altered in SQLite, so the table is rebuilt.
CREATE TABLE actions_new (
  id             INTEGER PRIMARY KEY,
  session_id     INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  track_id       INTEGER REFERENCES tracks(id),
  kind           TEXT NOT NULL
                 CHECK (kind IN ('move', 'copy', 'trash', 'keep', 'rename', 'tag')),
  src            TEXT NOT NULL,
  dst            TEXT,
  renamed        INTEGER NOT NULL DEFAULT 0,
  ts             INTEGER NOT NULL,
  undone         INTEGER NOT NULL DEFAULT 0,
  prev_status    TEXT NOT NULL DEFAULT 'pending',
  destination_id INTEGER REFERENCES destinations(id) ON DELETE SET NULL,
  duplicate      INTEGER NOT NULL DEFAULT 0,
  field          TEXT CHECK (field IN ('title', 'artist')),
  old_value      TEXT,
  new_value      TEXT
);
INSERT INTO actions_new (id, session_id, track_id, kind, src, dst, renamed, ts, undone,
                         prev_status, destination_id, duplicate)
  SELECT id, session_id, track_id, kind, src, dst, renamed, ts, undone,
         prev_status, destination_id, duplicate
  FROM actions;
DROP TABLE actions;
ALTER TABLE actions_new RENAME TO actions;
CREATE INDEX actions_session ON actions(session_id, id);
CREATE INDEX actions_destination ON actions(destination_id);
CREATE INDEX actions_track ON actions(track_id, id);
