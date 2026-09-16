-- Sharing S3: somebody else's share, pulled into one of this installation's libraries.
--
-- The api records a pull and wakes the peer role, which does the work: it fetches each file the destination does not
-- already hold into its staging volume, bundles them, and queues the bundles into one import batch. The row is what
-- survives a restart, so a peer role stopped half way picks the same pull up and fetches only the remainder.
--
-- `peer_share_id` is set null rather than cascaded when the mirror forgets a share, and the share's name and sharer are
-- copied here, so a pull whose share was withdrawn still says which share it was.
CREATE TABLE pull (
    id uuid PRIMARY KEY,
    peer_share_id uuid REFERENCES peer_share (id) ON DELETE SET NULL,
    device_id bytea NOT NULL REFERENCES peer (device_id),
    share_name text NOT NULL,
    library_id uuid NOT NULL REFERENCES library (id),
    state text NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'fetching', 'importing', 'done', 'failed')),
    files_total integer NOT NULL DEFAULT 0,
    files_done integer NOT NULL DEFAULT 0,
    bytes_total bigint NOT NULL DEFAULT 0,
    bytes_done bigint NOT NULL DEFAULT 0,
    -- The import batch, once the bundles are queued.
    batch_id uuid,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX pull_unfinished ON pull (created_at) WHERE state IN ('queued', 'fetching', 'importing');

-- Who a pulled part came from. Written once its import settles; a part pulled again keeps one row, with the newer date.
-- No cascade: purge deletes it by name, as it deletes every child of a part (`PgParts::purge`).
CREATE TABLE part_provenance (
    part_id uuid PRIMARY KEY REFERENCES part (id),
    device_id bytea NOT NULL,
    sharer_name text,
    pulled_at timestamptz NOT NULL DEFAULT now()
);
