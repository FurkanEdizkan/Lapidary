-- Governance is gone (S4, the owner's decision of 2026-10-16): no check-outs, no library modes, and the
-- lifecycle column nothing ever wrote. Every library now keeps a part's file history, which is what
-- `controlled` used to switch on; `revision` and its files are untouched, so no history is lost.
--
-- `job_outcome_known` keeps `unkept`: finished rows hold it, and finished history keeps its word (0054).
DROP TABLE part_lock;
ALTER TABLE revision DROP COLUMN lifecycle_state;
ALTER TABLE library DROP COLUMN mode;
