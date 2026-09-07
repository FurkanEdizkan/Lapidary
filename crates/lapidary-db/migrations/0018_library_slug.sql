-- A library's directory becomes a column, for the reason a category's already is.
--
-- `library_slug` derived it: `slugify(name).to_lowercase()`. That was safe while `0002`
-- seeded the only library there could be, and stopped being safe the moment `0017` let
-- somebody create one — because two *distinct names* can share one *directory*:
--
--   `Tabletop terrain` and `tabletop terrain`  → both `tabletop terrain`
--   `Rocks?` and `Rocks*`                      → both `Rocks-`
--
-- `0017`'s unique index is on the name and lets both pairs through. Their models then land
-- in one directory on disk, `model_dir_for` disambiguates the second `vee-block` into
-- `vee-block_a1b2c3`, and two libraries are interleaved in a folder the owner is invited to
-- open and read. Nothing is lost — `file.storage_path` records where everything actually
-- went — but the store stops being the legible thing `DATA.md` §1.1 builds everything on.
--
-- **This is `folder.slug`'s design, one level up, and deliberately so.** The slug is the
-- library's *address*, allocated once at creation; the name is its *label*. `DATA.md` §1.1
-- already says that about categories and gives the reasoning: a directory that follows a
-- name splits one thing across two directories the first time anyone renames it. There is
-- no library rename today, and when there is, this column is what makes it free.

alter table library add column slug text;

-- Backfilled with what `library_slug` computed, so an existing store's directories keep
-- pointing at the bytes already in them. `regexp_replace` mirrors `slug::slugify`'s
-- filesystem-safety rule closely enough for the names a deployment can currently hold — the
-- seeded `Default`, and anything created since `0017`, which is at most hours old.
-- `slugify` itself stays the authority for every row written from now on.
update library set slug = lower(regexp_replace(name, '[/\\:*?"<>|[:cntrl:]]', '-', 'g'));

alter table library alter column slug set not null;

-- The constraint that `0017`'s name index could not be. Two libraries may not share a
-- directory, whatever their names look like on screen.
create unique index library_slug_unique on library (slug);
