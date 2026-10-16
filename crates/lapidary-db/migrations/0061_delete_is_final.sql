-- M3: delete is delete (owner, 2026-10-17: "we don't want a restore option; if it is deleted it
-- is deleted"). Soft delete, restore and purge go, and with them `deleted_at` on `part` and
-- `folder`.
--
-- Every part and folder somebody had removed is deleted here for good, the way
-- `PgParts::delete` and `PgFolders::delete_subtree` now delete them: rows only, child-first,
-- and **no model file is touched**. No `quarantined_file` row is written and no source-file
-- hash is recounted, because both are what used to hand a part's own file to the reaper. Only
-- the derivative hashes are recounted, with `recount` (`repo.rs`) copied as `0055` copied it,
-- so a cached export nothing points at any more enters the ordinary quarantine.
--
-- Temporary tables for the same reason `0055` gives: every sub-statement of one query sees the
-- same snapshot, so the deletes have to be in the past before the count is taken.

CREATE TEMP TABLE gone_folder (id uuid PRIMARY KEY);
CREATE TEMP TABLE gone_part (id uuid PRIMARY KEY);
CREATE TEMP TABLE gone_blob (h text PRIMARY KEY);

-- A removed folder takes every folder under it, removed or not, as the subtree delete did.
INSERT INTO gone_folder (id)
WITH RECURSIVE down AS (
    SELECT id FROM folder WHERE deleted_at IS NOT NULL
    UNION
    SELECT f.id FROM folder f JOIN down ON f.parent_id = down.id
)
SELECT id FROM down;

INSERT INTO gone_part (id)
SELECT id FROM part
WHERE deleted_at IS NOT NULL OR folder_id IN (SELECT id FROM gone_folder);

INSERT INTO gone_blob (h)
SELECT DISTINCT d.blake3 FROM derivative d
JOIN revision r ON r.id = d.revision_id
WHERE r.part_id IN (SELECT id FROM gone_part) AND d.blake3 IS NOT NULL;

DELETE FROM part_image WHERE part_id IN (SELECT id FROM gone_part);
DELETE FROM part_source WHERE part_id IN (SELECT id FROM gone_part);
DELETE FROM derivative WHERE revision_id IN
    (SELECT id FROM revision WHERE part_id IN (SELECT id FROM gone_part));
DELETE FROM file WHERE revision_id IN
    (SELECT id FROM revision WHERE part_id IN (SELECT id FROM gone_part));
DELETE FROM revision WHERE part_id IN (SELECT id FROM gone_part);
DELETE FROM part_move WHERE part_id IN (SELECT id FROM gone_part)
    OR from_folder IN (SELECT id FROM gone_folder)
    OR to_folder IN (SELECT id FROM gone_folder);
DELETE FROM part WHERE id IN (SELECT id FROM gone_part);
DELETE FROM folder WHERE id IN (SELECT id FROM gone_folder);

WITH counts AS (
    SELECT h AS blake3,
           (SELECT count(*) FROM file f WHERE f.blake3 = h)
         + (SELECT count(*) FROM derivative d WHERE d.blake3 = h) AS actual
    FROM gone_blob
)
UPDATE blob b SET
    ref_count = c.actual,
    quarantined_at = CASE WHEN c.actual = 0
                          THEN coalesce(b.quarantined_at, now())
                          ELSE NULL END
FROM counts c
WHERE b.blake3 = c.blake3;

DROP TABLE gone_blob;
DROP TABLE gone_part;
DROP TABLE gone_folder;

ALTER TABLE part DROP COLUMN deleted_at;
ALTER TABLE folder DROP COLUMN deleted_at;
