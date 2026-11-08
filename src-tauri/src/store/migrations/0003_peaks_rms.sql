-- Peaks format 2: `[rms, peak]` pairs instead of `[min, max]`. The cache is only a cache:
-- drop the old rows, they are recomputed on first view.
DELETE FROM peaks_cache;
