-- Sharing S6: the people a folder goes to learn about each other.
--
-- A folder with several holders only works if its members can reach each other: one holder is enough for the
-- content to be there, and the others must be able to ask that one. They are strangers until somebody says
-- otherwise, and the owner is the one who says (owner's decision, 2026-09-17): the folder's owner publishes its
-- roster, everybody mirroring that folder reads it, and each member accepts once. An accepted machine is an
-- ordinary paired installation — the roster is not a second credential system — so every route it asks about
-- anything else answers it the refusal a stranger gets.
--
-- `introduced_by` records who published the roster that brought somebody here, for the People list to say so.
-- It is a person's own row afterwards: removing the introducer never removes them.
ALTER TABLE peer ADD COLUMN introduced_by bytea REFERENCES peer (device_id);

-- What the other installation said it can do, as its last hello listed it. Empty is an installation from before
-- this: the protocol number stays 1 for ever (an older reader refuses any other), and what is new is asked for
-- only of an installation that says it answers.
ALTER TABLE peer ADD COLUMN features text[] NOT NULL DEFAULT '{}';

-- A mirrored folder's roster, as its owner published it: who else holds or reads that folder, where they are,
-- and whether its owner lets them fetch files. It is a copy of somebody else's list, so it carries no foreign
-- key to `peer` — the whole point of a row here is a machine this installation has not paired with yet.
--
-- "Accepted" is not a column: an accepted introduction is a live `peer` row for that device, which is the same
-- row pairing by hand makes. `declined_at` is the only answer kept here, so a declined introduction stops being
-- offered without being forgotten and re-offered on the next round.
CREATE TABLE peer_share_member (
    peer_share_id uuid NOT NULL REFERENCES peer_share (id) ON DELETE CASCADE,
    device_id bytea NOT NULL CHECK (length(device_id) = 32),
    name text,
    address text NOT NULL,
    may_fetch boolean NOT NULL DEFAULT true,
    seen_at timestamptz NOT NULL DEFAULT now(),
    declined_at timestamptz,
    PRIMARY KEY (peer_share_id, device_id)
);

-- The page asks "who is offered and not answered yet", which reads every folder's roster by device.
CREATE INDEX peer_share_member_device ON peer_share_member (device_id);
