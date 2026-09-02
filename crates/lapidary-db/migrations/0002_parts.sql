-- Phase 1 slice 1. Shape follows docs/DATA.md §3.2; part_source and part_image are
-- Phase 2 and deliberately absent.

CREATE TABLE library (
  id          uuid PRIMARY KEY,
  name        text NOT NULL,
  mode        text NOT NULL DEFAULT 'hobby',   -- hobby | controlled, per LibraryMode
  created_at  timestamptz NOT NULL DEFAULT now()
);

-- Nothing in this slice creates a library and there is no library UI yet, so one is
-- seeded here with a fixed id. Whichever slice adds a second library replaces this seed
-- rather than building beside it.
INSERT INTO library (id, name) VALUES
  ('01931b6e-0000-7000-8000-000000000001', 'Default');

CREATE TABLE blob (
  blake3            text PRIMARY KEY,
  size_bytes        bigint NOT NULL,
  stored_bytes      bigint NOT NULL,          -- after compression; show real disk usage
  zstd_level        smallint,
  dict_id           uuid,                     -- per-library dictionaries land later
  ref_count         integer NOT NULL DEFAULT 0,
  quarantined_at    timestamptz,
  last_accessed_at  timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE part (
  id             uuid PRIMARY KEY,            -- uuid v7
  library_id     uuid NOT NULL REFERENCES library(id),
  part_number    text,
  name           text NOT NULL,
  classification text,
  created_at     timestamptz NOT NULL DEFAULT now(),
  updated_at     timestamptz NOT NULL DEFAULT now(),
  created_by     uuid,
  deleted_at     timestamptz,                 -- soft delete; never hard-deleted here
  metadata_json  jsonb NOT NULL DEFAULT '{}',
  -- STORED is mandatory: PG18 defaults to VIRTUAL and virtual columns cannot be indexed.
  -- `simple` deliberately: Phase 2 owns search, and part numbers must not be stemmed.
  search tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(part_number, '')), 'A') ||
    setweight(to_tsvector('simple', name), 'B')
  ) STORED
);

CREATE INDEX part_library_id_desc ON part (library_id, id DESC);

CREATE TABLE revision (
  id                  uuid PRIMARY KEY,
  part_id             uuid NOT NULL REFERENCES part(id),
  rev_label           text NOT NULL,
  parent_revision_id  uuid REFERENCES revision(id),
  origin              text NOT NULL,          -- 'ingest' in this slice
  author              uuid,
  message             text,
  created_at          timestamptz NOT NULL DEFAULT now(),
  lifecycle_state     text,
  locked_by           uuid,
  locked_at           timestamptz,
  -- Every measured value carries its own provenance. A single row-level flag would have
  -- to lie in Phase 2, where a STEP revision has an analytic volume and a tessellated
  -- triangle count on the same row.
  volume              double precision,
  volume_source       text,                   -- tessellated | analytic
  surface_area        double precision,
  surface_area_source text,
  bbox_x              double precision,
  bbox_y              double precision,
  bbox_z              double precision,
  bbox_source         text,
  -- triangle_count has no _source column: it counts tessellated primitives and cannot
  -- be analytic. Do not add one for symmetry.
  triangle_count      integer,
  is_watertight       boolean,
  units               text,
  mass_props_json     jsonb
);

CREATE INDEX revision_part_id ON revision (part_id);

CREATE TABLE file (
  id           uuid PRIMARY KEY,
  revision_id  uuid NOT NULL REFERENCES revision(id),
  role         text NOT NULL,                 -- 'source' in this slice
  format       text NOT NULL,                 -- 'stl'
  blake3       text NOT NULL REFERENCES blob(blake3),
  size_bytes   bigint NOT NULL,
  created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX file_revision_id ON file (revision_id);
CREATE INDEX file_blake3 ON file (blake3);

CREATE TABLE derivative (
  id             uuid PRIMARY KEY,
  revision_id    uuid NOT NULL REFERENCES revision(id),
  kind           text NOT NULL,               -- 'thumbnail' in this slice
  blake3         text,                        -- NULL when stored inline
  thumb_bytes    bytea,                       -- inline when < 64 KB, per DATA.md §1.5
  kernel_version text NOT NULL,
  params_json    jsonb NOT NULL,
  created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX derivative_revision_kind ON derivative (revision_id, kind);
