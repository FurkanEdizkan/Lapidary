-- Accounts (A1, owner 2026-10-17): several people use one install. The first one is made by the setup
-- wizard and is the admin; the admin makes the rest. A session is a random token in an HttpOnly cookie,
-- kept here only as its BLAKE3 hash, so a copy of this table signs nobody in.
--
-- `profile` (0060) stays until A3, so the wizard can offer its display name to the first account.

CREATE TABLE app_user (
  id            uuid PRIMARY KEY,
  -- Case-folded by the API before it gets here; the check is what keeps it that way.
  username      text NOT NULL UNIQUE CHECK (username ~ '^[a-z0-9][a-z0-9._-]{0,39}$'),
  display_name  text CHECK (display_name IS NULL OR length(btrim(display_name)) BETWEEN 1 AND 80),
  -- An argon2id PHC string.
  password_hash text NOT NULL,
  is_admin      boolean NOT NULL DEFAULT false,
  created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE session (
  token_hash   bytea PRIMARY KEY CHECK (length(token_hash) = 32),
  user_id      uuid NOT NULL REFERENCES app_user (id) ON DELETE CASCADE,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL
);
CREATE INDEX session_user_id_idx ON session (user_id);

-- This install's own settings, one row each: what the wizard and Settings change at runtime. `deploy/.env`
-- still wins where it sets the same thing.
CREATE TABLE instance_setting (
  key        text PRIMARY KEY,
  value      jsonb NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- Who a library and a part belong to. NULL only until setup: the wizard's account claims every row that
-- exists, which is how an install from before accounts keeps its models. A3 makes them NOT NULL.
ALTER TABLE library ADD COLUMN owner_id uuid REFERENCES app_user (id);
ALTER TABLE part ADD COLUMN owner_id uuid REFERENCES app_user (id);
