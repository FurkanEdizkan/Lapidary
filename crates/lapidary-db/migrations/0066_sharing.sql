-- Sharing a model (A4, owner 2026-10-17): with chosen people, or with everybody on the install. A share
-- lets a person look at the model and download it; only its owner changes it.

CREATE TABLE part_share (
  part_id    uuid NOT NULL REFERENCES part (id) ON DELETE CASCADE,
  user_id    uuid NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (part_id, user_id)
);
CREATE INDEX part_share_user_id_idx ON part_share (user_id);

ALTER TABLE part ADD COLUMN shared_with_everyone boolean NOT NULL DEFAULT false;
CREATE INDEX part_shared_with_everyone_idx ON part (shared_with_everyone) WHERE shared_with_everyone;
