-- The same track (name and content) was already in the destination, so nothing was copied
-- (copy) or the source went to the Trash (move). Undoing a duplicate copy must not trash
-- the file that was there before.
ALTER TABLE actions ADD COLUMN duplicate INTEGER NOT NULL DEFAULT 0;
