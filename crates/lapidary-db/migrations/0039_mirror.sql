-- Sharing S2b: what the people this installation is paired with share, mirrored here, so a shared library
-- browses while the other machine is asleep.
--
-- A cache of another machine's list, and never this installation's data: nothing here is anything its owner
-- made. The hello round replaces a share's parts whole each time it reads the catalogue again, and deletes a
-- share, with its parts, once the other side stops offering it. `CLAUDE.md`'s rule for cache eviction holds —
-- it must never read as data loss — which is why the page says the sharer stopped offering a part rather than
-- that anything here was removed.
--
-- `digest` is the one the parts were read under; `synced_at` is null until a whole catalogue has been read.
CREATE TABLE peer_share (
    id uuid PRIMARY KEY,
    device_id bytea NOT NULL REFERENCES peer (device_id),
    remote_id uuid NOT NULL,
    name text NOT NULL,
    part_count bigint NOT NULL,
    digest text NOT NULL DEFAULT '',
    synced_at timestamptz,
    UNIQUE (device_id, remote_id)
);

CREATE TABLE peer_share_part (
    peer_share_id uuid NOT NULL REFERENCES peer_share (id) ON DELETE CASCADE,
    source_path text NOT NULL,
    remote_part uuid NOT NULL,
    name text NOT NULL,
    part_number text,
    tags text[] NOT NULL DEFAULT '{}',
    licences text[] NOT NULL DEFAULT '{}',
    blake3 text,
    size_bytes bigint,
    format text,
    thumbnail bytea,
    PRIMARY KEY (peer_share_id, source_path)
);
