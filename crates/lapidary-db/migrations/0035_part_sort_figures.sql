-- The grid's sort keys, copied from each part's latest revision onto the part row, so one index per key
-- hands a library's parts back in order (DATA.md §3.2). `insert_revision_chain`, the only writer of a
-- revision row, keeps them current. NULL is a figure nobody measured, as on `revision`; the order reads
-- it as -infinity, and so does each index.
ALTER TABLE part
  ADD COLUMN latest_volume         double precision,
  ADD COLUMN latest_surface_area   double precision,
  ADD COLUMN latest_longest_side   double precision,
  ADD COLUMN latest_triangle_count integer;

UPDATE part p
   SET latest_volume = r.volume,
       latest_surface_area = r.surface_area,
       latest_longest_side = greatest(r.bbox_x, r.bbox_y, r.bbox_z),
       latest_triangle_count = r.triangle_count
  FROM (SELECT DISTINCT ON (part_id) part_id, volume, surface_area, bbox_x, bbox_y, bbox_z, triangle_count
          FROM revision
         ORDER BY part_id, created_at DESC, id DESC) r
 WHERE r.part_id = p.id;

CREATE INDEX part_volume_order_idx
  ON part (library_id, coalesce(latest_volume, '-infinity') DESC, id DESC);
CREATE INDEX part_surface_area_order_idx
  ON part (library_id, coalesce(latest_surface_area, '-infinity') DESC, id DESC);
CREATE INDEX part_longest_side_order_idx
  ON part (library_id, coalesce(latest_longest_side, '-infinity') DESC, id DESC);
CREATE INDEX part_triangles_order_idx
  ON part (library_id, coalesce(latest_triangle_count::double precision, '-infinity') DESC, id DESC);
