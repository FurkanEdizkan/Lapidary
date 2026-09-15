-- A part's materials, editable (goal 5, stage 1). `true` once a person has typed them, so what a file
-- states fills `materials` only while nobody has. Setting an empty list makes it `false` again.
ALTER TABLE part ADD COLUMN materials_typed boolean NOT NULL DEFAULT false;
