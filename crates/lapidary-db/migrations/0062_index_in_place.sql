-- M4: your folders, read where they are (owner, 2026-10-17: "we don't want to ingest into a
-- custom location; use the directed file path of each file or folder").
--
-- A scan records where a file is and reads it there. Nothing is copied into the store, renamed,
-- moved or written beside it any more. So a `file` row says which directory its `storage_path`
-- is relative to, and whether the bytes it describes are still there to read.

-- A library's own folder, relative to the ingest mount. '' is the whole mount, which is what
-- every library scanned until now.
ALTER TABLE library ADD COLUMN root text NOT NULL DEFAULT '';

-- `library`: the owner's folders, under the ingest mount (read only, never written).
-- `inbox`:   uploads, link imports and the seeded examples, written there once by Lapidary.
-- `store`:   Lapidary's own blob root. Every row from before this migration is one, and its bytes
--            stay exactly where they are: a path under the store, or `blobs/ab/cd/<hash>` when
--            `storage_path` is NULL, as before.
ALTER TABLE file ADD COLUMN root text NOT NULL DEFAULT 'store';
ALTER TABLE file ALTER COLUMN root DROP DEFAULT;
ALTER TABLE file ADD CONSTRAINT file_root_known CHECK (root IN ('library', 'inbox', 'store'));

-- False once the file at this row's path was replaced by a later revision's bytes: the row keeps
-- the figures and the hash, and Lapidary keeps no copy of what was overwritten.
ALTER TABLE file ADD COLUMN on_disk boolean NOT NULL DEFAULT true;

-- The file's modification time when it was indexed, in nanoseconds since the epoch, beside the
-- size already in `size_bytes`. A download compares both before it sends a byte. NULL for a
-- `store` row: those bytes are Lapidary's, and the end-of-stream hash is their check.
ALTER TABLE file ADD COLUMN mtime_ns bigint;

-- Nothing reads or writes this any more. It listed purged parts' model files in the store, for
-- the reaper to unlink; that half of the reaper is gone, so the files it named stay where they
-- are, which is the safe direction to be wrong in.
DROP TABLE quarantined_file;

-- The job kinds that went with the old store. A row still queued would fail on its first attempt
-- as a kind this build does not know; failing it here says why instead.
UPDATE job
   SET state = 'failed',
       leased_by = NULL,
       lease_expires_at = NULL,
       updated_at = now(),
       last_error = CASE kind
         WHEN 'ingest_blob' THEN
           'Lapidary no longer copies uploads into its store; it indexes your files where they '
           'are. Upload this file again.'
         WHEN 'migrate_storage' THEN
           'Lapidary no longer moves files into its store; files already there stay where they '
           'are and still download. Nothing needs doing.'
         ELSE
           'Lapidary no longer writes metadata.json beside your files. Nothing needs doing.'
       END
 WHERE kind IN ('ingest_blob', 'migrate_storage', 'describe_part')
   AND state IN ('pending', 'running');
