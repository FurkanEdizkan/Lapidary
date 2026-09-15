-- Custom fields: a library's own named values on its parts (DATA §3.5, amended 2026-09-15; the local
-- product spec §1).
--
-- The values live in `part.metadata_json->'custom'`, beside `cad`. One GIN index serves every field's
-- filter as `@>`, so no index is ever built from a key a person typed; `indexed` says which fields the
-- grid offers as filters, at most 8 per library, which the API checks under the library's row lock.

CREATE TABLE custom_field (
    id uuid PRIMARY KEY,
    library_id uuid NOT NULL REFERENCES library (id) ON DELETE CASCADE,
    key text NOT NULL CHECK (key ~ '^[a-z0-9_]{1,40}$'),
    label text NOT NULL CHECK (label = btrim(label) AND char_length(label) BETWEEN 1 AND 80),
    type text NOT NULL CHECK (type IN ('text', 'number', 'choice')),
    options_json jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(options_json) = 'array'),
    indexed boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT custom_field_key_unique_per_library UNIQUE (library_id, key)
);

CREATE INDEX part_custom_gin ON part USING gin ((metadata_json -> 'custom') jsonb_path_ops);
