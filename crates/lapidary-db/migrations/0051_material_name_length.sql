-- A density for a material named as a CAD file names it (L3, item 5).
--
-- `material_density.material` is keyed by the material exactly as `part.materials` holds it, and that
-- column is unbounded `text[]` — whatever the file stated. 64 characters was the limit a *person*
-- typing a tag gets, and applying it here meant a STEP file naming
-- "Stainless steel, AISI 316L, annealed, cold drawn bar to ASTM A276/A276M" — 71 characters, and an
-- ordinary thing for a supplier's export to say — could never be given a density, so that part could
-- never have a mass at all.
--
-- 200, which covers a grade with its standard and its condition spelled out, and still leaves the
-- primary key far inside btree's ~2704-byte entry limit. Past it the API refuses saying the number,
-- and the person can shorten the part's own material to match: `PUT /api/parts/{id}/materials`
-- takes the same 200.
--
-- Declared inline in `0034`, so the name Postgres gave it is what gets dropped.
ALTER TABLE material_density DROP CONSTRAINT material_density_material_check;
ALTER TABLE material_density ADD CONSTRAINT material_density_material_check
    CHECK (material = btrim(material) AND char_length(material) BETWEEN 1 AND 200);
