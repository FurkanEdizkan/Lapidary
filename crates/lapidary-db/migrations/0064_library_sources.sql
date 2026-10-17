-- One storage location (A2, owner 2026-10-17). The models folder is one mount; a library is no longer
-- one folder of it but the files and folders a person added from the Import browser. A scan walks
-- those, and a part's `source_path` is its path under the models folder.

CREATE TABLE library_source (
  id            uuid PRIMARY KEY,
  library_id    uuid NOT NULL REFERENCES library (id) ON DELETE CASCADE,
  -- Relative to the models folder; '' is the whole of it.
  path          text NOT NULL,
  kind          text NOT NULL CHECK (kind IN ('file', 'folder')),
  -- The model's page, when the person gave one, and whether its details are fetched from it.
  page_url      text,
  fetch_details boolean NOT NULL DEFAULT false,
  created_at    timestamptz NOT NULL DEFAULT now(),
  UNIQUE (library_id, path)
);

-- Pictures a person chose while adding, given to every part found under the source.
CREATE TABLE library_source_image (
  source_id  uuid NOT NULL REFERENCES library_source (id) ON DELETE CASCADE,
  position   integer NOT NULL,
  image_webp bytea NOT NULL,
  PRIMARY KEY (source_id, position)
);

-- Whether a file added with a page link fetches that page's details unless the person says otherwise:
-- the Import dialog's starting answer, set in Settings.
ALTER TABLE app_user ADD COLUMN fetch_details boolean NOT NULL DEFAULT false;

-- A library that had its own folder keeps it, as its one source, and its parts' paths become paths
-- under the models folder, which is what a scan now produces for them. '' was the whole mount and
-- becomes nothing: a library holds what was added to it.
INSERT INTO library_source (id, library_id, path, kind)
  SELECT gen_random_uuid(), id, btrim(root, '/'), 'folder' FROM library WHERE btrim(root, '/') <> '';
UPDATE part p SET source_path = btrim(l.root, '/') || '/' || p.source_path
  FROM library l WHERE p.library_id = l.id AND btrim(l.root, '/') <> '';
ALTER TABLE library DROP COLUMN root;
