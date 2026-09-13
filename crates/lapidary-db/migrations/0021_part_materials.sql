-- The material facet's column. `DATA.md` §3.2: whatever a filter or a facet reads is a typed
-- column with an index, never a path into `metadata_json`.
--
-- An array, because an assembly carries as many materials as its bodies do and one file can
-- declare several. `not null default '{}'`, so a mesh — which declares none — is an empty array
-- rather than a NULL every predicate would have to coalesce.
--
-- GIN, and the queries spell the test `materials @> array[$n]` rather than `$n = any(materials)`:
-- containment is what a GIN index on an array serves, and `any` would scan every row.
--
-- Backfilled from what stage 4 already wrote, so a part ingested before this column reads the
-- same as one ingested after it.
alter table part add column materials text[] not null default '{}';

update part
   set materials = array(select jsonb_array_elements_text(metadata_json->'cad'->'materials'))
 where jsonb_typeof(metadata_json->'cad'->'materials') = 'array';

create index part_materials on part using gin (materials);
