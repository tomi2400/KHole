-- A failed analysis (undecodable, too short, sidecar error) is cached as well, with the
-- reason, so the same file is not tried again on every scan. It is retried once the
-- analyzer version changes, like any other result.
ALTER TABLE analysis_cache ADD COLUMN error TEXT;
