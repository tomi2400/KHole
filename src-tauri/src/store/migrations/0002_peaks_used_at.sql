-- LRU eviction for the waveform cache: peaks are cheap to recompute (~0.3 s per track),
-- so the cache is capped by size and the least recently shown tracks go first.
ALTER TABLE peaks_cache ADD COLUMN used_at INTEGER NOT NULL DEFAULT 0;
CREATE INDEX peaks_cache_used_at ON peaks_cache (used_at);
