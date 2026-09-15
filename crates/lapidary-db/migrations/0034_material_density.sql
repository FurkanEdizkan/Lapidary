-- A density per material, per library (goal 5, stage 2): what a part's mass is worked out from.
--
-- Keyed by the material exactly as parts hold it, capitals and all, as the materials facet keeps it.
-- Typed by a person and never measured, so mass worked out from it is never exact. Stored in kg/m³
-- whatever the page shows; the API says the bounds in words, and this check refuses the rest.
CREATE TABLE material_density (
    library_id uuid NOT NULL REFERENCES library (id) ON DELETE CASCADE,
    material text NOT NULL CHECK (material = btrim(material) AND char_length(material) BETWEEN 1 AND 64),
    density_kg_m3 numeric NOT NULL CHECK (density_kg_m3 > 0 AND density_kg_m3 < 25000),
    PRIMARY KEY (library_id, material)
);
