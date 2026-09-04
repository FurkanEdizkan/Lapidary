-- Slice 3's schema change: two constraints `derivative` should always have had. No new
-- tables and no new columns -- three tessellations are three rows of the existing shape.

-- A derivative is stored inline (`thumb_bytes`) or by hash (`blake3`), never both and
-- never neither. Both columns have been nullable and independent since 0002, so a row with
-- neither has been legal all along -- and a row with neither is a derivative that cannot be
-- served. The LOD ladder is the first thing able to produce one, because it is the first
-- thing to write `blake3` at all.
--
-- `<>` on two booleans is XOR: exactly one of the two is null. Written this way rather than
-- as a pair of ORs because the pair is easy to get subtly wrong, and `or` in particular
-- accepts the both-null case that this exists to refuse.
alter table derivative add constraint derivative_storage_is_exclusive
    check ((blake3 is null) <> (thumb_bytes is null));

-- `file.blake3` has referenced `blob(blake3)` since 0002; `derivative.blake3` never has.
-- Nothing noticed because nothing wrote the column. The ladder writes it, so the ladder is
-- what could write a dangling one.
--
-- This is also what makes `ref_count` cover derivatives: a rung needs a `blob` row, and a
-- rung whose bytes another revision already stored costs a `ref_count` bump rather than a
-- second file. Three identical rungs on a small part -- the ordinary case for anything
-- under the L0 budget -- are one blob with `ref_count` 3.
alter table derivative add constraint derivative_blake3_references_blob
    foreign key (blake3) references blob(blake3);
