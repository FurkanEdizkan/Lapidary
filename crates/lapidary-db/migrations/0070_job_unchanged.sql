-- A33: how many files a scan found unchanged since they were indexed, and so did not queue. Kept on the scan job's own
-- row, and counted by `batch_status` as files the batch holds and skipped, so a rescan still says what it found.
ALTER TABLE job ADD COLUMN unchanged integer NOT NULL DEFAULT 0;
