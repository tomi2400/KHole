-- Lower bound of the session's BPM range (the range is [bpm_min, 2 * bpm_min)); analysed
-- BPMs outside it are halved or doubled into it. NULL: no folding.
ALTER TABLE sessions ADD COLUMN bpm_min INTEGER;
