-- Sharing S1b: who this installation is to the people it shares with, and who they are.
--
-- `peer_identity` is one row, written by the peer role as it starts. `device_id` is the digest of the key
-- that role keeps on disk; the key itself never comes here, because a key in the database is a key the
-- api's own credentials can read. `name` is what this installation calls itself, typed by its owner
-- through the api and said to every paired machine in the hello. No row means the peer role has never run
-- here, which is how the page tells an installation with sharing switched off.
CREATE TABLE peer_identity (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    device_id bytea NOT NULL CHECK (length(device_id) = 32),
    name text CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 64)
);

-- One row per installation its owner paired with, by pasting that installation's device id and where to
-- reach it. Removal is soft: `removed_at` hides the row and takes the device off the list the peer role
-- accepts, and adding the same id again brings the row back rather than making a second one.
--
-- `name` is what the other installation calls itself, as its last hello said. `last_seen_at` and
-- `last_error` are the peer role's hello round: an answer sets the first and clears the second, and a
-- failure sets the second and keeps the first, so the page can say both why and since when.
CREATE TABLE peer (
    device_id bytea PRIMARY KEY CHECK (length(device_id) = 32),
    address text NOT NULL CHECK (address = btrim(address) AND char_length(address) BETWEEN 3 AND 255),
    name text CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 64),
    added_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz,
    last_seen_at timestamptz,
    last_error text
);
