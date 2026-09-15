-- A CAD revision's B-rep faces and edges, as the bridge counts them (goal 4, stage 4): every face and edge of
-- the placed shape, analytic or not. A mesh revision has neither, and neither has a CAD revision read before
-- the bridge counted them, so the revision diff leaves the counts out rather than showing a zero.
alter table revision
  add column face_count integer,
  add column edge_count integer,
  add constraint revision_topology_counts check (
    (face_count is null) = (edge_count is null)
    and coalesce(face_count, 0) >= 0
    and coalesce(edge_count, 0) >= 0
  );
