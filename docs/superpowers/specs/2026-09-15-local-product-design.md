# Custom fields, Turkish search, saved filters finished, a capped cut, watched folders

**Goal:** `docs/superpowers/plans/2026-09-15-local-product-goal.md`
**Closes:** the Phase 4 and Phase 5 features this Linux machine can build and check, with the mock
kernel and no image build.

---

## 0. The gaps, precisely

- **Custom fields.** DATA §3.5 describes a table, but no such table exists.
  - `part.metadata_json` holds only `cad`.
  - `PgParts::set_metadata` writes it when a part is created, and replaces the whole object.
- **Search.**
  - Every `tsquery` is `plainto_tsquery('simple', …)`: in `search`, `format_facet`, `material_facet`
    and `tag_facet`, all in `repo.rs`.
  - `part.search` is a STORED column built with `'simple'` (`0022`).
  - `library` has no language.
- **Saved filters.**
  - There is no rename and no order.
  - A filter whose category was deleted opens an empty grid saying nothing is filed there yet.
- **The section cut is open.** The viewer's renderer is created without a stencil buffer, which three
  0.186 does not give by default.
- **Watched folders.** The agent watches only the one checked-out file, and nothing watches a folder.

## 1. Custom fields

### 1.1 Schema

```sql
CREATE TABLE custom_field (
  id           uuid PRIMARY KEY,
  library_id   uuid NOT NULL REFERENCES library(id) ON DELETE CASCADE,
  key          text NOT NULL CHECK (key ~ '^[a-z0-9_]{1,40}$'),
  label        text NOT NULL CHECK (label = btrim(label) AND length(label) BETWEEN 1 AND 80),
  type         text NOT NULL CHECK (type IN ('text', 'number', 'choice')),
  options_json jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(options_json) = 'array'),
  indexed      boolean NOT NULL DEFAULT false,
  created_at   timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT custom_field_key_unique_per_library UNIQUE (library_id, key)
);
CREATE INDEX part_custom_gin ON part USING gin ((metadata_json->'custom') jsonb_path_ops);
```

- **Where values live:** `part.metadata_json->'custom'->'<key>'`.
  - A JSON string for `text` and `choice`, and a JSON number for `number`.
  - Beside `cad`, never inside it.
- **`set_metadata` writes `cad` alone,** with `jsonb_set`, so an ingest and a field edit never
  overwrite each other's key.

### 1.2 Rules

- **Key.**
  - `[a-z0-9_]{1,40}`, unique per library, and never renamed.
  - The dialog proposes one from the label, and the API validates what it is sent.
  - The label can change.
- **Types.**
  - `text`: a string, trimmed, 1 to 512 characters.
  - `number`: a finite JSON number.
  - `choice`: one of `options_json`. That is 1 to 50 options, each 1 to 80 characters, no repeats.
- **Caps.**
  - 32 fields per library, of which at most 8 are `indexed`.
  - Both are checked in the transaction that writes the field, under the library row's lock, so two
    requests cannot both take the eighth place.
- **Indexing amends DATA §3.5,** as the goal's default, on the owner's behalf.
  - One GIN index over `(metadata_json->'custom') jsonb_path_ops`, with filters written as `@>`.
  - One expression index per field would be DDL built from a key a user typed.
  - `indexed` now means "offered as a grid filter". The cap of 8 stays, as the grid's limit rather
    than a write cost.
- **Removing a field removes its definition only.**
  - Its values stay in `metadata_json`.
  - The part page lists them read-only, under their keys, as no longer defined in this library.
  - Defining a field with the same key again shows them as values again.
- **Removing a choice option that parts still hold is refused.** The refusal names how many parts
  hold it, so they can be changed first. Adding options and relabelling are free.
- **A value of the wrong type is refused when written.**
  - A number field refuses "twelve", and the message names the field.
  - A value stored before a field was defined again with another type is shown as it is, and refused
    only when somebody edits it.

### 1.3 Routes

| Route | Does |
|---|---|
| `GET /api/libraries/{id}/fields` | The definitions, oldest first |
| `POST /api/libraries/{id}/fields` | Takes `{key, label, type, options?, indexed?}`. Refuses with 409 `keyTaken`, `tooManyFields` or `tooManyIndexed`, or 400 `badKey` or `badOptions` |
| `PATCH /api/libraries/{id}/fields/{key}` | Takes `{label?, options?, indexed?}`, never the key or the type. Refuses with 409 `optionInUse` or `tooManyIndexed` |
| `DELETE /api/libraries/{id}/fields/{key}` | Removes the definition, and nothing else |
| `PUT /api/parts/{id}/fields/{key}` | Takes `{value}`, where `null` clears this part's value. Refuses with 400 `wrongType` or 404 `noSuchField` |

- **The detail.** `GET /api/parts/{id}` carries the part's `custom` object as it is stored. The page
  joins it with the library's definitions.
- **The grid filter.**
  - The parts route and the facets route take `field` and `fieldValue`.
  - `field` must name an `indexed` field of that library, or the route answers 400 `notAFilter`.
  - `fieldValue` is parsed by that field's type.
  - The route binds `{"<key>": value}` as one JSON parameter. The predicate is
    `($n::jsonb IS NULL OR p.metadata_json->'custom' @> $n::jsonb)`, the shape the `material` and
    `tag` filters already have.
  - One field filter at a time, just as there is one material and one tag.
- **Saved filters** accept `field` and `fieldValue`, and refuse a `field` that is not an indexed
  field of their library.
- **The grid's URL:** `validateSearch` gains `field` and `fieldValue`.

### 1.4 UI

- **The library menu** gains a **Fields** dialog. It lists the fields and lets a person:
  - add one, with a key proposed from its label;
  - relabel one, and edit its options;
  - offer one as a filter;
  - remove one.
- **The part page** gains a **Fields** section under the tags.
  - Editable where the tags are (`recordable`), and read-only in the quick look.
  - The values of removed fields are listed below it, read-only.
- **The facets beside the grid** gain a filter for each indexed field. A choice lists its options;
  text and number take a typed value.
- Every string goes through `strings.ts`.

### 1.5 Not in this goal

- **Ranges on a number field.** `@>` is equality only, and a range needs a typed expression index,
  which is DDL built from a key again.
- **`metadata.json`.** A value edited on the part page reaches it only when the manifest is next
  rewritten, by an ingest or a revision. The api cannot write into the worker's store.
- **Facet counts** per field value.

## 2. Turkish search

### 2.1 Checked on `postgres:18` (18.6), 2026-09-15

- **What the column needs holds.**
  - The `turkish` config is installed.
  - `to_tsvector(regconfig, text)` and `websearch_to_tsquery(regconfig, text)` are immutable, so a
    STORED generated column may read a `regconfig` column of its own row.
  - A GIN index over that column builds.
- **Stemming, measured.** 22 real inflection pairs, each with the inflected form in the name and the
  base word as the query:

| Query built with | Matched |
|---|---|
| `simple`, today's `tsvector` path | 0 of 22 |
| `turkish` | 12 of 22 |
| `simple`, with a prefix (`q:*`) | 19 of 22 |
| `turkish`, with a prefix | 20 of 22 |

- **What `turkish` adds.** Today's search already matches names, part numbers and paths with
  `ILIKE '%q%'`, which reaches as far as the prefix rows do for a one-word query. What `turkish` adds
  is what a substring cannot find:
  - a softened root, so *kapak* finds *kapağı* and *yatak* finds *yatağı*;
  - inflected tags and materials;
  - a query of several inflected words.
- **So there is no prefix matching.** `plainto_tsquery` stays, with the library's config, beside the
  `ILIKE` it already has.
- **Capital I, recorded, not fixed.** The database's `en_US.utf8` lowercases capital I the English
  way: *IŞIK* is indexed as `işik`, and `ILIKE` folds it the same way. So a name written in capitals
  is not found by a query typed with `ı`.
  - Folding ı and i together, in both the name and the query, would fix it.
  - So would a database created with a Turkish ctype.

### 2.2 Schema and queries

- **`library.language`:** `text NOT NULL DEFAULT 'simple' CHECK (language IN ('simple', 'turkish'))`,
  chosen at creation.
- **`part.search_config`:** `regconfig NOT NULL DEFAULT 'simple'`. The insert that creates a part sets
  it from the library.
- **`part.search`** is dropped and added again as `to_tsvector(search_config, …)`, with the same three
  weights and `lapidary_words`. `part_search_gin` is dropped and rebuilt with it.
  - The migration rewrites every part row once.
- **Queries** use `plainto_tsquery((SELECT language FROM library WHERE id = $1)::regconfig, $q)`, in
  `search` and in the three facets.
  - `text::regconfig` is only stable, which a query may use.
  - The subquery runs once per query, so the GIN index still serves the match.
- **Moves** stay within a library, so a part's config never needs recomputing.
- **Changing a library's language later is out of scope.** It would rewrite every part in that
  library.

### 2.3 UI

The create-library dialog gains a search language: "Any language" (`simple`) or "Turkish".

### 2.4 Tests

- In a `turkish` library, "Şaft yatağı kapağı" is found by "yatak kapak".
  - The name holds ğ, ş and ı, and neither query word is a substring of it.
  - In a `simple` library the same query finds nothing, so a pass means the library's config was used.
- A `simple` library answers exactly as before: the existing search tests run unchanged.
- A part moved to another category keeps its `search_config`.

## 3. Saved filters, finished

- **Rename:** `PATCH /api/libraries/{library}/filters/{filter}` with `{name}`. The name rules, and the
  409 `nameTaken`, are the ones create has.
- **Order.**
  - `saved_filter.position int NOT NULL`, backfilled in `created_at` order. A new filter goes last,
    and the list is ordered by `position`.
  - `POST …/filters/{filter}/move` with `{direction: "up" | "down"}` swaps a filter with its neighbour,
    in one transaction, under the library row's lock.
  - At either end, a move answers 200 and changes nothing.
- **A filter whose category was deleted.**
  - The list route adds `folderGone: boolean` to each filter: true when it names a category that is
    not live. It is read on every list, so restoring the category clears it.
  - The list marks such a filter.
  - When the grid's URL names a category the live tree does not hold, the grid does not show the
    empty "Nothing filed here yet". It says the category was deleted, and offers the same filters
    without it.
  - It says nothing while the tree is still loading.
  - This check is the grid's and not the list's, because the grid also catches an old link.
- **Tests:**
  - a rename, and a rename to a name already taken;
  - a move up and a move down, including at either end;
  - `folderGone` before and after a delete;
  - the grid's notice.

## 4. A capped section cut

- **A stencil buffer.** The viewer's renderer is created with `stencil: true`.
- **The cap uses three's stencil technique, over the current rung only:**
  1. Clipped by the section plane, the model's back faces increment the stencil and its front faces
     decrement it. Neither pass writes colour or depth.
  2. A plane lying on the section and covering the part's bounds is drawn where the stencil is not
     zero, then clears the stencil.
  3. The part is drawn as it is today.
- **Why that finds solid material.** The stencil counts the surfaces a ray from the eye crosses behind
  the cut. For a closed mesh it is non-zero exactly inside material, and zero through a bore.
- **Colour:** `CAP = 0xc4665a`, flat and unlit. It is distinct from the part's lit grey (`0xb8bcc4`)
  and from the ghost's amber.
- **Picking.** Neither the cap nor the stencil passes is in the group that picks intersect, and their
  `raycast` does nothing. Wall thickness and picks behave exactly as today.
- **The ghost is not capped.** It is a see-through overlay, and a cap would hide the current rung
  behind it.
- **Closed meshes only.**
  - A part whose `isWatertight` is not `true` gets no cap.
  - The bar says the mesh is open, so the cut shows no filled face.
  - A guessed cap is never drawn.
- **A pure helper** places the cap plane from the section and the bounds, in `viewer-math.ts`. It is
  tested alongside `sectionPlane`.
- **The check,** on the native stack with SwiftShader:
  1. Cut the flange at Z = 8 mm.
  2. Sample pixels inside the section: cap colour. Sample through the bore: not cap colour.
  3. Time the first cut's compile over CDP, counting `renderer.info.programs`.
  4. Record that turning the cut off and on again adds no program.

## 5. Watched-folder ingest through the agent

`lapidary watch <folder> --library <id>`, on Linux, like the rest of the agent.

### 5.1 One list of model formats

`MESH_EXTENSIONS` (`lapidary-ingest/src/scan.rs`) and `CAD_FORMATS` (`handler.rs`) move to
`lapidary-core`, together with the candidate check. The scan and the agent then never disagree about
what a model file is.

### 5.2 A poll

- **Every 2 s,** the folder is listed recursively without following symlinks, and each candidate
  file's size and mtime are read.
- **Skipped** (DATA §6.2's list, whole, since every file under the folder is watched):
  - names starting with `.` (which covers `.DS_Store`) or with `~$`;
  - names ending in `.bak`, `.tmp`, `.lck` or `.autosave`, in any case. `.3dm.bak` is among them;
  - `Thumbs.db`;
  - anything §5.1 does not call a model file.
- **Each file keeps a `watch::Watch`.** A change waits for `SETTLE`, then is hashed once. A hash equal
  to the one the state file holds is not a change.
- **A pure planner** turns the previous state and one listing into actions: upload these files, forget
  these deleted ones. The tests run against it, with no filesystem and no server.

### 5.3 An upload

- **Files that settle in the same poll go up together.** It is the existing probe, 8 MiB chunks and
  commit, taken out of `send_back` so that check-in and watch share it. Watch sends no lock.
- **The source path** is the file's path relative to the watched folder, with `/` separators. It is
  refused if it would escape the folder (`reject_escaping_path`).
- **The server decides the outcome,** as for any upload: ingested, skipped, revised or unkept. The
  agent follows the batch, and prints its counts and failures.
- **Origin is `upload`.** A part checked out to somebody is refused by the worker, and the agent prints
  that refusal as it arrived.

### 5.4 A deletion

- Printed once: "`<path>` was deleted here; nothing changes in the library".
- Forgotten from the state.
- Nothing is sent, since user data is never deleted implicitly.

### 5.5 State

- **Where:** `$XDG_STATE_HOME/lapidary/watch-<library>.json`, falling back to `~/.local/state`.
- **What:** for each relative path, the size, the mtime and the BLAKE3 last uploaded.
- **When:** after a commit is accepted, written to a temp file and renamed into place.
- **Nothing is written inside the watched folder.**
- **On start,** a file is treated as changed if its size or mtime differs from the state, or if the
  state does not know it. A first run therefore uploads the whole folder, and the probe skips the
  bytes the server already holds.

### 5.6 The ceiling

- **Measured on 2026-09-15**, over the STL corpus's tree, listing and `lstat` only: 2,778 files and 546
  directories.
  - 109 ms cold.
  - 12 to 13 ms warm.
- **At one poll every 2 s,** that is under 1% of a core.
- **Marked `ponytail:`.** A tree about a hundred times larger costs about a second per poll. When it
  does, `notify` (inotify), which ARCHITECTURE names, replaces the poll.

### 5.7 Tests and check

- **Unit tests:**
  - the ignore list;
  - relative source paths: nested, and never escaping;
  - a deletion that sends nothing;
  - after a restart, an unchanged file sends nothing.
- **The check,** on the native stack, watching a scratch copy of `example/parts` in `target/`:
  1. Add a file: it is ingested.
  2. Change a file in a controlled library: a revision.
  3. Write a `.tmp` file: nothing.
  4. Delete a file: nothing on the server.

## 6. Not in this goal

- **Bundles:** already shipped in Phase 4 slice 2.
- **Changing a library's language** after it is created.
- **Custom fields:** number ranges, and facet counts.
- **The macOS and Windows watchers.**

## 7. Decided without the owner

1. **Custom field indexing** is one GIN index with `@>` filters, amending DATA §3.5, as the goal's
   default. `indexed` means "offered as a filter", still capped at 8.
2. **A library holds at most 32 fields,** beside the goal's 8 indexed.
3. **The key is typed, from a proposal.** The dialog proposes a key from the label, and the API
   validates what it is sent. A key is never derived server-side from a label that may not be ASCII.
4. **A removed field's values show read-only on the part page,** under their keys. They become values
   again if the key is defined again.
5. **Removing a choice option still in use is refused,** naming the count.
6. **`set_metadata` writes `cad` alone,** so an ingest and a field edit never overwrite each other.
7. **One field filter at a time,** as there is one material and one tag.
8. **No prefix matching for Turkish.** Stemming matched 12 of 22 pairs, and the `ILIKE` substring
   search already there covers the prefix cases (§2.1).
9. **The goal's example test is replaced.** "bağlantı finds bağlantılar" already passes today, through
   `ILIKE`, so it cannot fail first. "yatak kapak" finding "Şaft yatağı kapağı" can.
10. **Capital I under `en_US.utf8`** is recorded, not fixed.
11. **A deleted category is the grid's check,** made against the live tree, so an old link is covered
    too. The list's `folderGone` mark sits beside it.
12. **A move at either end answers 200 and changes nothing.**
13. **The cap is `0xc4665a`,** and the ghost is never capped.
14. **Model formats move to `lapidary-core`,** so the agent and the scan share one list.
15. **A first watch uploads the whole folder,** and trusts the probe to skip bytes the server already
    has.
