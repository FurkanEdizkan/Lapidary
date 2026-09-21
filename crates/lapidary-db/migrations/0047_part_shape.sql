-- Phase 6: a shape profile per part, for near-duplicates and "more like this" (design:
-- docs/goals/phase-6.md).
--
-- The profile is computed by the worker from the part's current revision's L0 tessellation, never from its
-- source file, and never on the open path. `l0_blake3` is the tessellation it was computed from, so a rebuilt
-- L0 is noticed; `version` is `SHAPE_VERSION` in `lapidary-core`, so a changed algorithm is noticed. Either
-- makes the row stale, and a stale row is ignored and computed again.
--
-- **No pgvector** (owner's decision, 2026-09-21): at the sizes this app serves an exact scan over a plain array
-- beats an approximate index, and the test databases carry no `vector` extension. The ceiling is about 100k
-- parts a library; the upgrade is one statement:
--   ALTER TABLE part_shape ALTER descriptor TYPE vector(35) USING descriptor::vector;
--
-- One row a part — its current shape. Purged with the part (`PgParts::purge`); no cascade, as everywhere.
CREATE TABLE part_shape (
    part_id uuid PRIMARY KEY REFERENCES part (id),
    library_id uuid NOT NULL,
    revision_id uuid NOT NULL REFERENCES revision (id),
    l0_blake3 text NOT NULL,
    version smallint NOT NULL,
    -- The mean distance between two points on the surface, in millimetres.
    size_mm float8 NOT NULL CHECK (size_mm > 0),
    descriptor real[] NOT NULL CHECK (cardinality(descriptor) = 35),
    computed_at timestamptz NOT NULL DEFAULT now()
);

-- Near-duplicate candidates are the parts of about the same size in the same library: a ±2% band on this
-- index returns tens of rows, not the library.
CREATE INDEX part_shape_size ON part_shape (library_id, size_mm);

-- The job that computes a profile finishes as `profiled` (goal G2).
ALTER TABLE job DROP CONSTRAINT job_outcome_known;
ALTER TABLE job ADD CONSTRAINT job_outcome_known
    CHECK (outcome IS NULL OR outcome IN ('ingested', 'skipped', 'rendered', 'scanned', 'migrated', 'revised',
                                          'unkept', 'described', 'profiled'));
