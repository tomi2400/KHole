-- The track list's sort order in the session (TanStack Table sorting state as JSON, e.g.
-- [{"id":"bpm","desc":false}]); NULL: folder order. Opaque to the backend.
ALTER TABLE sessions ADD COLUMN sort TEXT;
