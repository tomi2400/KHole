-- The track list keeps folder order by a track's original path (its first action's src).
CREATE INDEX IF NOT EXISTS actions_track ON actions(track_id, id);
