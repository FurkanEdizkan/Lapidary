# What the Node prototype knew

The prototype (Fastify + SQLite, ~1,000 LOC of services) is deleted. It remains on
`main` and in git history. This records what it established, so the knowledge outlives
the code.

## Domain shape that survived contact with real files

`model.service.ts` settled on a row shape worth carrying into `lapidary-core`:
identity (`id`, `name`, `creator`), classification (`type`, `mesh_kind`, `format`),
geometry (`bbox_x/y/z`, `triangle_count`, `file_size_bytes`), provenance
(`created_date`, `added_date`, `original_path`), and derivative presence flags
(`thumbnail_path`, `lod_path` → `hasThumbnail`, `hasLod`, `hasOriginal`).

The three many-to-many attachments — tags, groups, printer types — were each a join
table resolved to a sorted name list. Lapidary keeps that shape; the join resolution
moves behind repository traits in `lapidary-db`.

**Note the bug we are not carrying forward:** `listModels` did `SELECT * FROM models`
and filtered in TypeScript. Phase 1 filters and paginates in SQL with keyset
pagination.

## Search behaviour worth reproducing

`search.service.ts` returned three result classes from one query — matching models,
creators with counts, and tags drawn from matched models — plus a header that changes
between `POPULAR TAGS` (empty query) and `TAGS & SUGGESTIONS` (non-empty). Caps were
5 models, 4 creators, 8 tags, 9 popular tags, 12 rail tags.

Tag counts were computed by a full `GROUP BY` on every keystroke. `lapidary-index`
replaces this with `tsvector` + `pg_trgm` and the 10k exact-count threshold from
`docs/DATA.md`, but the *shape* of the suggestion payload is right.

## LOD approach

`rust-mesh` was dependency-free so it compiled offline in the container build — a
constraint worth preserving. It computed an exact bounding box in mm and a triangle
count, and generated LOD by **vertex clustering on a 48³ grid**, writing binary STL.

Vertex clustering is the right first LOD algorithm for `lapidary-cad`: single pass,
no topology required, degrades gracefully on the malformed meshes real libraries
contain. The 48 constant was tuned by eye and should become an L0/L1/L2 ladder.

## Explicitly not carried forward

- **`cache.service.ts`** — Redis. The cache and the job queue are both Postgres.
- **Per-model procedural sample shapes** in `seed.ts`. Phase 1 seeds one real
  licence-clean example part.
- **`npm run dev` as the primary run path.** `podman compose up` is the entry point.
