-- The keys a person gives Lapidary for the sites it imports from (F3, 2026-10-16): Thingiverse's app
-- token and MyMiniFactory's account token. Printables needs none. One row per site, replaced when a
-- new key is saved and deleted when it is cleared.
--
-- Plain text, deliberately and said so: this is a single-person install on a machine its owner runs,
-- the database is theirs, and a key encrypted with a secret kept beside it would only look safer. The
-- API never returns a key once saved — only that one is there and its last four characters.
CREATE TABLE site_key (
  site       text PRIMARY KEY,
  secret     text NOT NULL,
  checked_ok boolean,
  checked_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT site_key_known_site CHECK (site IN ('thingiverse', 'myminifactory')),
  CONSTRAINT site_key_not_blank CHECK (length(btrim(secret)) > 0)
);
