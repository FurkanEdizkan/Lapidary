-- Sharing S5: who a shared category goes to.
--
-- Until now a share reached everyone paired, which is right for two people and wrong the moment somebody
-- pairs with a third. A share now names its people (owner's decision, 2026-09-17), so a folder can go to
-- two of the five installations you know.
--
-- `audience` is what keeps that decision from changing anything already shared: every share made before
-- this migration stays `everyone`, including for people paired after it, until its owner picks members.
-- Picking members sets `members`, and from then on the list is the whole of who sees it. A share with an
-- empty list reaches nobody, which is what stopping without stopping looks like; stopping is still
-- `removed_at`, and still the thing that withdraws a folder.
ALTER TABLE share ADD COLUMN audience text NOT NULL DEFAULT 'everyone'
    CHECK (audience IN ('everyone', 'members'));

-- One row a person a share. Removal is soft, as everywhere in sharing: a member taken off a folder keeps
-- the parts they already pulled — those are theirs now — and the row records that they were once in it.
CREATE TABLE share_member (
    share_id uuid NOT NULL REFERENCES share (id),
    device_id bytea NOT NULL REFERENCES peer (device_id),
    added_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz,
    PRIMARY KEY (share_id, device_id)
);

-- Every read asks "is this device a live member of this share", so the index is on the device.
CREATE INDEX share_member_live ON share_member (device_id) WHERE removed_at IS NULL;
