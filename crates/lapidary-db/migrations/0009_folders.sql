-- Location becomes a thing the user can change, and the store becomes a folder they can
-- open. See docs/superpowers/specs/2026-09-06-folder-tree-and-moves-design.md.
--
-- Three columns, three jobs, and the names must stay distinct:
--   part.source_path   -- immutable. Where the file sat in the INGEST directory (0007).
--   part.folder_id     -- mutable. Which category the user has put it in.
--   file.storage_path  -- mutable. Where the bytes actually sit in the STORE.

create table folder (
  id          uuid primary key,
  library_id  uuid not null references library(id),
  parent_id   uuid references folder(id),          -- null = library root
  name        text not null,                       -- what the user typed
  slug        text not null,                       -- what the filesystem got
  created_at  timestamptz not null default now(),
  deleted_at  timestamptz,

  -- `nulls not distinct` is load-bearing, not decoration. PostgreSQL treats NULLs as
  -- distinct in a unique constraint by default, so the plain form silently permits two
  -- root categories both named Terrain -- the first thing a corpus scan produces.
  constraint folder_name_unique_per_parent
    unique nulls not distinct (library_id, parent_id, name),

  -- Both are needed. "Rocks?" and "Rocks*" are distinct names that slug to the same
  -- directory, so without this the tree is legal and the disk is not.
  constraint folder_slug_unique_per_parent
    unique nulls not distinct (library_id, parent_id, slug)
);

create index folder_library_parent on folder (library_id, parent_id);

alter table part add column folder_id uuid references folder(id);   -- null = library root
create index part_folder_id on part (folder_id);

-- Where the bytes actually are, relative to the storage root. Stays nullable: null means
-- "still at the old content-addressed path", which is a live state for as long as the
-- migrate_storage job takes to drain -- hours on a real corpus. A later migration makes it
-- NOT NULL, once it has drained everywhere.
alter table file add column storage_path text;

create table part_move (
  id           uuid primary key,
  part_id      uuid not null references part(id),
  from_folder  uuid references folder(id),
  to_folder    uuid references folder(id),
  moved_at     timestamptz not null default now(),
  moved_by     uuid                                -- null until Phase 8 has a principal
);

create index part_move_part_id on part_move (part_id, moved_at desc);

-- Rebuild the category tree from the nested source_paths slice 6a's recursive scan wrote.
--
-- Level by level rather than one recursive CTE: a CTE cannot insert rows and then use the
-- ids it just generated as the next level's parents.
do $$
declare lvl int := 1; maxlvl int;
begin
  create temporary table _dirs on commit drop as
    select p.id as part_id, p.library_id,
           string_to_array(regexp_replace(p.source_path, '/[^/]*$', ''), '/') as segs
      from part p where position('/' in p.source_path) > 0;

  create temporary table _map (library_id uuid, path text, folder_id uuid,
                               primary key (library_id, path)) on commit drop;

  select max(array_length(segs, 1)) into maxlvl from _dirs;

  -- 16 matches scan.rs's MAX_DEPTH. A tree deeper than that was already truncated on the
  -- way in, so following it here would build folders holding nothing.
  while lvl <= coalesce(maxlvl, 0) and lvl <= 16 loop
    with want as (
      select distinct d.library_id,
             array_to_string(d.segs[1:lvl], '/') as path,
             d.segs[lvl] as name,
             case when lvl = 1 then null
                  else array_to_string(d.segs[1:lvl-1], '/') end as parent_path
        from _dirs d where array_length(d.segs, 1) >= lvl
    ), ins as (
      insert into folder (id, library_id, parent_id, name, slug)
      select gen_random_uuid(), w.library_id, m.folder_id, w.name, w.name
        from want w
        left join _map m on m.library_id = w.library_id and m.path = w.parent_path
      returning id, library_id, parent_id, name
    )
    insert into _map (library_id, path, folder_id)
    select i.library_id,
           case when lvl = 1 then i.name
                else (select m2.path from _map m2 where m2.folder_id = i.parent_id)
                     || '/' || i.name end,
           i.id
      from ins i;
    lvl := lvl + 1;
  end loop;

  update part p set folder_id = m.folder_id
    from _dirs d
    join _map m on m.library_id = d.library_id
               and m.path = array_to_string(d.segs, '/')
   where p.id = d.part_id;
end $$;
