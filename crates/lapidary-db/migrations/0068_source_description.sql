-- A15: what a model's page says about it, kept on the row that names the page. Free text, as the site wrote it;
-- Fetch details fills it and a fetch that finds none keeps the one before.
ALTER TABLE part_source ADD COLUMN description text;
