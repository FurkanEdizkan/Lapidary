-- Sharing S7: a folder stays browsable when its owner is away.
--
-- A folder's identity, across every installation that holds it, is **(its owner's device id, its owner's id
-- for it)** — which is what `peer_share (device_id, remote_id)` already is. What changes here is what
-- `device_id` means: it was "who told us about this folder" and is now "whose folder it is". The two were the
-- same until now, because the only installation that told you about a folder was the one that owned it.
--
-- `catalogue_from` is the other half: who this copy of the catalogue was read from. Null is the owner, which
-- is every row made before this migration and every row read from the owner since. A relayed copy names the
-- member it came through, so a page can say whose reading it is showing.
ALTER TABLE peer_share ADD COLUMN catalogue_from bytea REFERENCES peer (device_id);

-- When the copy held here was read **from the owner** — by whoever read it. For a direct read that is when
-- this installation read it; for a relayed one it is when the relaying installation did, as it said. It is
-- not `synced_at`, which stays "when this installation last wrote this row" and is a different question.
--
-- Freshness, and only freshness, decides between two copies: a relayed catalogue is taken only when its
-- as-of beats the one held here. There is no round barrier — a mirror of the owner and a mirror of a member
-- are separate tasks and may finish in either order — so a direct read does not win by being direct. It wins
-- by being newer, which it almost always is, and when it is not, the newer copy is the truer one anyway.
ALTER TABLE peer_share ADD COLUMN catalogue_as_of timestamptz;

-- Every folder mirrored so far was read from its owner, so what is held is as of when it was read.
UPDATE peer_share SET catalogue_as_of = synced_at;

-- **A known ceiling.** What decides who a folder may be passed on to is the roster its owner published, as
-- this installation last read it — and it can only be read from that owner. So an owner who takes somebody off
-- a folder and then goes offline leaves every other holder passing that folder on to them until the owner
-- answers again. The roster's `seen_at` says how old the answer is; nothing here refuses an old one yet.
-- Bounding it belongs with taking a folder back (S10), where removal is the subject.
--
-- A page asks "which folders may I serve to this member", which reads the roster by device.
CREATE INDEX peer_share_relayed ON peer_share (catalogue_from) WHERE catalogue_from IS NOT NULL;
