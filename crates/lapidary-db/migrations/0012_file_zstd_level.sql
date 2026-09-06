-- How a source file was written and what it occupies, recorded on the row that names it.
--
-- `blob.zstd_level` described one copy of one hash, which was true while source bytes were
-- deduplicated and every hash had exactly one file on disk. Slice 7 ended that (0008): a
-- file now lives in its model's own directory, one copy per model, written uncompressed so
-- the owner opening the folder sees `cliff.stl` and not a zstd frame named `cliff.stl`.
--
-- During the migration window the two facts diverge and one column cannot hold both. A
-- scan meeting bytes identical to a still-un-migrated blob at a different `source_path`
-- writes a raw file into a model directory and links it onto the existing `blob` row,
-- which still says `zstd_level = 3` because the legacy copy at `blobs/ab/cd/<hash>` really
-- is compressed. Whichever way that shared column is set, one of the two rows is then
-- read at the wrong level: the download route zstd-decodes a raw STL and 500s, or
-- `migrate_storage` reads a compressed copy raw and refuses the hash as corrupt.
--
-- `stored_bytes` splits for exactly the same reason and in the same window: the legacy copy
-- occupies its compressed size and the model file occupies its real one, and a card reading
-- the shared column reported 91,204 bytes for a 204,800-byte file it had just called
-- uncompressed. Two numbers about one file, contradicting each other on the same card.
-- `CLAUDE.md` does not grade that as cosmetic -- measurement must not lie -- and leaving the
-- level here while the size stayed on `blob` would split one fact across two tables, which
-- is how the next reader gets it wrong again.
--
-- So both move to the row that knows them. After this, `blob.zstd_level` and
-- `blob.stored_bytes` describe one thing only -- the legacy content-addressed copy, read
-- only while `file.storage_path` is null -- and the `file` columns are what every reader of
-- a file follows. `blob.size_bytes` stays shared and correct: it is the uncompressed length
-- of the bytes, which is a property of the hash.
--
-- Both nullable, like `storage_path` and for the same reason: a row written before this
-- migration by something outside `insert_part_chain` has nothing to record, and `NULL`
-- keeps meaning "nobody recorded how these bytes were stored", which `download.rs` answers
-- with a refusal naming the blob rather than serving something and hoping.
alter table file add column zstd_level smallint;
alter table file add column stored_bytes bigint;

-- The backfill is exact rather than a guess, because both writers of `storage_path` write
-- their file with `Compression::AsIs`: `lapidary-ingest`'s handler at ingest and
-- `migrate_storage`'s copy. A row with a path is therefore raw whatever the shared blob
-- row says -- including the rows this migration exists to fix, which are the only ones
-- where the two disagree. A row without a path is still at the old path, at the level the
-- blob row records, which is exactly what that column has always meant.
update file f
   set zstd_level = case when f.storage_path is not null then 0 else b.zstd_level end,
       stored_bytes = case
         when f.storage_path is not null then f.size_bytes
         else b.stored_bytes
       end
  from blob b
 where b.blake3 = f.blake3;
