-- A `blob` row's `stored_bytes` describes its content-addressed copy under `blobs/`, and since
-- migration `0013` nothing else: a file filed in a model directory keeps its size on its `file`
-- row. Ingest, a revision and `migrate_storage` still wrote that size onto the blob row of a hash
-- with no content-addressed copy at all, and purge, the quarantine sweep and the instance storage
-- figure each counted the phantom copy beside the real model file (the
-- purge-removes-the-model-directory design, §6: 22,660 bytes reported, 12,976 freed). Those
-- writers now write 0, and this corrects the rows they already wrote.
--
-- A row is phantom when its level is 0 and every copy of its bytes is filed: no `file` row still
-- at the content-addressed path, and no derivative, whose bytes are always content-addressed. A
-- quarantined row is phantom when the model file its bytes were is quarantined beside it.
--
-- One case this cannot tell apart, and so under-counts: a 3MF upload is staged uncompressed, at
-- level 0, under `blobs/`, and that staged copy is real. Its row reads as phantom and becomes 0.
update blob b
   set stored_bytes = 0
 where b.zstd_level = 0
   and b.stored_bytes <> 0
   and not exists (select 1 from derivative d where d.blake3 = b.blake3)
   and not exists (select 1 from file f where f.blake3 = b.blake3 and f.storage_path is null)
   and (exists (select 1 from file f where f.blake3 = b.blake3)
        or (b.quarantined_at is not null
            and exists (select 1 from quarantined_file q where q.blake3 = b.blake3)));
