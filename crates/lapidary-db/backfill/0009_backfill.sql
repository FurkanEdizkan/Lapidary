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
