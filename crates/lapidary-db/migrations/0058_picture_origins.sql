-- Two ways a part gets a picture without a person choosing a file (F1, 2026-10-16): the thumbnail a
-- slicer saved inside a 3MF (`embedded`), and the view somebody had in the browser's 3D viewer when
-- they pressed "use this view" (`snapshot`). Nothing renders one.
--
-- Widened, not rewritten: `og_fetched` and `rendered` have no writer, but an upgraded install may
-- hold rows with them, and a check those rows fail would stop this migration.
ALTER TABLE part_image DROP CONSTRAINT part_image_known_origin;
ALTER TABLE part_image ADD CONSTRAINT part_image_known_origin CHECK (
  origin IN ('uploaded', 'url_supplied', 'og_fetched', 'rendered', 'embedded', 'snapshot')
);
