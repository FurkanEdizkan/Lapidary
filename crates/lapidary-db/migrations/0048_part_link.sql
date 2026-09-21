-- Phase 6: what a person decided about two parts that look alike (design: docs/goals/phase-6.md).
--
-- `variant`: they belong together, and are shown on each other's page. `distinct`: not the same, whatever the
-- shapes say. Either takes the pair out of the duplicates queue for good. `folded_into`: `part_id` was folded
-- into `other_id` — soft-removed as its duplicate, by the same rule as any removal, and brought back by Restore.
-- "Fold into" is this phase's name for the roadmap's "merge"; CLAUDE.md reserves "no merge" for versioning.
-- Nothing is moved and nothing is deleted.
--
-- A `variant` or `distinct` pair is one row, stored with the smaller id first, so a pair cannot be decided twice
-- in two directions. `folded_into` has a direction — which part was kept — so it is stored as it happened.
-- Purged with either part (`PgParts::purge`); no cascade, as everywhere.
CREATE TABLE part_link (
    part_id uuid NOT NULL REFERENCES part (id),
    other_id uuid NOT NULL REFERENCES part (id),
    library_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('variant', 'distinct', 'folded_into')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (part_id, other_id),
    CHECK (part_id <> other_id),
    CHECK (kind = 'folded_into' OR part_id < other_id)
);

-- A part's links are read from either side.
CREATE INDEX part_link_other ON part_link (other_id);
