-- Saved filters: a name for a set of the grid's filters, kept per library (`FEATURES.md`, "Saved
-- filters / smart collections"). `search` holds what the grid's URL carries -- q, folderId, format,
-- material, tag -- as the API validated it. Nothing in SQL reads inside it, and it is never itself a
-- filter, a sort or a facet, so it is JSONB by `DATA.md` §3.2's rule.
--
-- There are no users yet, so a library's saved filters are everyone's who opens that library.

create table saved_filter (
  id          uuid primary key,
  library_id  uuid not null references library(id) on delete cascade,
  name        text not null check (name = btrim(name) and char_length(name) between 1 and 80),
  search      jsonb not null check (jsonb_typeof(search) = 'object'),
  created_at  timestamptz not null default now(),

  -- The name a person picks the filter by, so one per library. Also the index a library's list reads.
  constraint saved_filter_name_unique_per_library unique (library_id, name)
);
