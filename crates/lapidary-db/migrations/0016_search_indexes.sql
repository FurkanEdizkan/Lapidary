-- The indexes search needs, which `0001` and `0002` prepared for and neither created.
--
-- `0002` made `part.search` a **STORED** generated tsvector and `schema.rs` has guarded that
-- ever since, with a comment saying a virtual column "would make Phase 2's search silently
-- unindexable". The guard was right and the index was never written — so every search until
-- this migration would have been a sequential scan over `part`, which looks fine on the 156
-- parts this was developed against and stops looking fine somewhere around a corpus.
--
-- **Three indexes, because one predicate cannot do this job.** Verified against this
-- deployment's own PostgreSQL 18.6 before writing any of it:
--
--   to_tsvector('simple', 'A1234-56-B')  →  '-56':2 'a1234':1 'b':3
--
-- The fragment `1234` is not a lexeme, so no `tsquery` finds it — and Phase 2's exit
-- criterion is exactly "searching a part number like A1234-56-B by the fragment 1234
-- returns it at position one". `similarity('A1234-56-B', '1234')` is 0.231, under
-- `pg_trgm`'s 0.3 default, so the `%` operator does not find it either. `ILIKE '%1234%'`
-- does, and `gin_trgm_ops` is what turns that from a scan into a lookup.

-- The tsvector half: multi-word AND over part numbers (weight A) and names (weight B).
-- What it buys that substring matching cannot is "bracket mount" finding *Mount bracket,
-- left* — two words, either order, neither adjacent.
create index part_search_gin on part using gin (search);

-- The trigram half, which is what actually answers a fragment.
--
-- `gin_trgm_ops` and not `gist_trgm_ops`: GiST buys KNN distance ordering (`<->`) that
-- nothing here asks for — ranking is tiers over match kinds, not a distance — and pays for
-- it with slower lookups. GIN's slower writes are the right trade for a table written once
-- per ingested file and read on every keystroke.
create index part_number_trgm on part using gin (part_number gin_trgm_ops);
create index part_name_trgm on part using gin (name gin_trgm_ops);

-- Two notes worth carrying, both about what this migration deliberately does not do.
--
-- **`part_number_trgm` indexes a column that is entirely NULL today** — 156 of 156 rows in
-- the library this was measured against, because nothing writes it: `handler.rs` passes
-- `None` and the only non-NULL in the tree is a test fixture. Phase 2's metadata extractor
-- is what fills it. The index costs nothing to maintain while the column is empty, and the
-- alternative is remembering to add it in a slice that will be about something else.
--
-- **Not `CONCURRENTLY`.** sqlx supports it through a `-- no-transaction` directive, and a
-- customer with an existing hundred-thousand-part library is who would want it. But a
-- failed concurrent build leaves an INVALID index behind, and a half-applied non-atomic
-- migration in an air-gapped deployment is a worse failure than a table lock measured in
-- milliseconds at this size. If that customer arrives, the change is this file with
-- `-- no-transaction` at the top and `CONCURRENTLY` on each statement.
--
-- No partial `where deleted_at is null` either: search reads the removed list too, through
-- the same `Shows` predicate the grid uses, and a partial index would quietly stop serving
-- it.
