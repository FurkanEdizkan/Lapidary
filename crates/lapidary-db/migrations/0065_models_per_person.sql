-- Models per person (A3, owner 2026-10-17): a person sees their own libraries and models, and only
-- those (sharing is A4). Ownership is checked by the API against these columns on every read and write.

-- A part belongs to its library's owner. Rows from before setup have no owner until setup claims them.
CREATE INDEX library_owner_id_idx ON library (owner_id);
CREATE INDEX part_owner_id_idx ON part (owner_id);

-- A library's name is unique per person now, not per install: two people may each have a "Prints".
-- Same names as before, so the refusals that read them are unchanged.
DROP INDEX library_name_unique;
DROP INDEX library_slug_unique;
CREATE UNIQUE INDEX library_name_unique ON library (owner_id, name);
CREATE UNIQUE INDEX library_slug_unique ON library (owner_id, slug);

-- Site keys are each person's own: a MyMiniFactory token is somebody's account. The keys saved before
-- this are the first admin's, which is who saved them.
ALTER TABLE site_key ADD COLUMN user_id uuid REFERENCES app_user (id) ON DELETE CASCADE;
UPDATE site_key SET user_id = (SELECT id FROM app_user WHERE is_admin ORDER BY created_at LIMIT 1);
DELETE FROM site_key WHERE user_id IS NULL;
ALTER TABLE site_key ALTER COLUMN user_id SET NOT NULL;
ALTER TABLE site_key DROP CONSTRAINT site_key_pkey;
ALTER TABLE site_key ADD PRIMARY KEY (user_id, site);

-- 0060's single profile row: accounts replaced it in A1, and the wizard that read it has run.
DROP TABLE profile;
