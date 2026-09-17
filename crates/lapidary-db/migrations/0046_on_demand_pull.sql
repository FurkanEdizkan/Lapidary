-- Sharing S9: opening one part of a shared folder, rather than pulling the whole thing.
--
-- Everyone in a folder sees all of it without holding any of it (owner's decision, 2026-09-17), so the usual
-- way to get a part is to open it and ask for that one. A pull of one part is the same pull as a pull of a
-- folder — the same queue, the same staging, the same import — with one file in it.
--
-- Null is the whole folder, which is every pull made before this migration and every "Pull all" since.
ALTER TABLE pull ADD COLUMN source_path text;
