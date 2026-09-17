-- Sharing S4: asking first, and a pull that waits or pauses.
--
-- A share is open unless its owner asks to be asked: then the people paired still see what it offers, and fetching its
-- files needs a grant. One row a person a share, asked once; the owner grants or denies it, and may change their mind.
ALTER TABLE share ADD COLUMN mode text NOT NULL DEFAULT 'open' CHECK (mode IN ('open', 'ask'));

CREATE TABLE share_grant (
    share_id uuid NOT NULL REFERENCES share (id),
    device_id bytea NOT NULL REFERENCES peer (device_id),
    state text NOT NULL DEFAULT 'asked' CHECK (state IN ('asked', 'granted', 'denied')),
    asked_at timestamptz NOT NULL DEFAULT now(),
    decided_at timestamptz,
    -- Who decided. Null while the installation's owner is the only one who can: the seam Phase 8's users fill.
    decided_by text,
    PRIMARY KEY (share_id, device_id)
);

-- A pull waits while its sharer has not granted it, and stays put while paused here. A paused pull is not picked up.
ALTER TABLE pull DROP CONSTRAINT pull_state_check;
ALTER TABLE pull ADD CONSTRAINT pull_state_check
    CHECK (state IN ('queued', 'fetching', 'waiting', 'paused', 'importing', 'done', 'failed'));
DROP INDEX pull_unfinished;
CREATE INDEX pull_unfinished ON pull (created_at) WHERE state IN ('queued', 'fetching', 'waiting', 'importing');
