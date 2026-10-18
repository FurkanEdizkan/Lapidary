-- Packs (A13, owner 2026-10-18): a .zip, .7z or .rar in the models folder kept as one part, never unpacked. What is
-- inside it is read once at ingest, from the archive's own index, because opening a part never reads its file:
-- {"entries": [{"path": …, "size": …}, …], "total": n} for a list (at most 5,000 entries; `total` counts them all),
-- or {"unread": "<why>"} when there is none. NULL on every revision that is not a pack.

ALTER TABLE revision ADD COLUMN contents jsonb;
