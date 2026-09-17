-- Sharing S8: whoever holds a folder's files can serve them.
--
-- One holder is enough for a folder's content to be there, and several may hold the same content (owner's
-- decision, 2026-09-17). What makes that true is this: an installation that pulled a folder answers for its
-- files too, so the folder's people can fetch from whichever of them is awake rather than only from its owner.
--
-- On by default, because pulling a folder and then refusing to pass it on is not what anybody means by joining
-- one. Per folder, because a machine on a metered link may want to hold a folder without serving it.
ALTER TABLE peer_share ADD COLUMN seeding boolean NOT NULL DEFAULT true;
