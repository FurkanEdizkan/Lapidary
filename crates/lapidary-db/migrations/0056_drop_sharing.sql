-- Sharing between installations is gone (S3, the owner's decision of 2026-10-16). These are the ten
-- tables 0037-0046 and 0052 made for it, children before parents. Nothing outside them pointed in:
-- a pulled part was an ordinary part, and only `part_provenance` said where it came from. No
-- column on a core table, no job kind and no outcome belonged to sharing, so this is the whole of it.
DROP TABLE part_provenance;
DROP TABLE pull;
DROP TABLE share_grant;
DROP TABLE share_member;
DROP TABLE share;
DROP TABLE peer_share_member;
DROP TABLE peer_share_part;
DROP TABLE peer_share;
DROP TABLE peer;
DROP TABLE peer_identity;
