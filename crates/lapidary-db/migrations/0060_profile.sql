-- This install's profile (M2, 2026-10-17): one row, because there is no sign-in and one person runs
-- the install. The name the user menu and the Profile page show. Accounts, if they ever come, replace
-- this table rather than extend it.
CREATE TABLE profile (
  only_row     boolean PRIMARY KEY DEFAULT true CHECK (only_row),
  display_name text CHECK (display_name IS NULL OR length(btrim(display_name)) BETWEEN 1 AND 80),
  updated_at   timestamptz NOT NULL DEFAULT now()
);
INSERT INTO profile (only_row) VALUES (true);
