-- Sharing S2a: the categories this installation offers the people it is paired with.
--
-- A share is a category and everything under it, including parts added later (owner's decision,
-- 2026-09-16), so nothing here lists parts: the catalogue is worked out from the folder tree each time it
-- is read. Everyone paired sees every share; asking first, and who was granted, arrive with S4, where
-- something reads them. Removal is soft, as everywhere else.
CREATE TABLE share (
    id uuid PRIMARY KEY,
    library_id uuid NOT NULL REFERENCES library (id),
    folder_id uuid NOT NULL REFERENCES folder (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz
);

-- One live share per category: sharing a category again is the same share, not a second one.
CREATE UNIQUE INDEX share_one_live_per_folder ON share (folder_id) WHERE removed_at IS NULL;
