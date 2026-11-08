-- Undo needs the status a track had before the action (pending, kept or copied), and the
-- destination tiles count what went to them.
ALTER TABLE actions ADD COLUMN prev_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE actions ADD COLUMN destination_id INTEGER REFERENCES destinations(id) ON DELETE SET NULL;
CREATE INDEX actions_destination ON actions(destination_id);
