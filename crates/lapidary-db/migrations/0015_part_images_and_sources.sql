-- Where a part came from, and what it looks like when a render is not the right picture.
--
-- Both tables are sketched in `docs/DATA.md` §4 and were deferred twice: slice 4's design
-- worked out the decisions and sent the execution to "beside the detail view and the upload
-- control", and slice 5 did not reach them. This is that slice.
--
-- **One owner decision constrains the shape, taken 2026-09-05 and binding:** a part carries
-- an *ordered gallery* — user images first, generated views appended after them, never
-- replacing them, and both freely added and deleted. So there is no `unique (part_id)`, and
-- `position` below is the column that decision asked for.

create table part_source (
  id           uuid primary key,
  part_id      uuid not null references part(id),
  -- Where this model came from. Nullable because the other fields are worth recording for
  -- a part somebody bought at a trade show or modelled themselves, and a row with a vendor
  -- and a price and no URL is a real answer.
  url          text,
  vendor       text,
  -- The seller's own identifier: an SKU, a Thingiverse id, a part number in their
  -- catalogue. Named `external_id` and not `sku` because it is whatever they call it.
  external_id  text,
  title        text,
  -- **Not bureaucracy** (`DATA.md` §4): half of hobbyist STL libraries are non-commercial,
  -- and somebody selling prints needs to see that before they print. Free text rather than
  -- an enum — the licences in the wild do not fit one, and refusing to record `CC-BY-NC-SA
  -- 4.0` because it is not in our list would make the field useless for the case it exists
  -- for.
  license      text,
  -- Minor units, so 12.50 EUR is 1250. Never a float: a price is money, and money in binary
  -- floating point is a rounding error waiting for a total to be taken of it.
  price_minor  bigint,
  -- ISO 4217, three letters. Meaningless without `price_minor` and vice versa, which the
  -- CHECK below makes the database's opinion rather than a convention.
  currency     text,
  retrieved_at timestamptz,
  created_at   timestamptz not null default now(),
  -- One row per source per part. A part genuinely can have two — the model from one place
  -- and the hardware from another — and the same URL twice is a duplicate, not a second.
  unique (part_id, url),
  constraint part_source_price_has_currency check (
    (price_minor is null) = (currency is null)
  )
);

create index part_source_part_id_idx on part_source (part_id);

create table part_image (
  id         uuid primary key,
  part_id    uuid not null references part(id),
  -- **Inline below 64 KB, a blob above it** — the same split `derivative` already makes
  -- (`DATA.md` §1.5), and made the same way so there is one rule in the store rather than
  -- two. A thumbnail-sized image travels with the row and costs a grid page no extra
  -- request; a photograph goes to the content-addressed store, where two parts sharing one
  -- picture share one file.
  --
  -- Exactly one of them, enforced below rather than left to the writer. A row with both
  -- would have two answers to "what does this look like", and a row with neither is an
  -- image that is not there.
  blake3     text references blob(blake3),
  image_webp bytea,
  -- `uploaded`, `url_supplied`, `og_fetched` or `rendered`. Text and not an enum, matching
  -- every other discriminator in this schema: adding a value must not need a migration.
  origin     text not null,
  -- Where it was fetched from, for an image that was. Kept so a broken picture can be
  -- re-fetched and so a person can see they did not choose it themselves — never used to
  -- load the image, which is always served from our own copy. Hotlinking leaks a referrer
  -- on every grid scroll and breaks whenever the host rotates a URL.
  source_url text,
  -- Position in the gallery, ascending. The owner's decision above is what needs it: user
  -- images sort before generated views, and both are reorderable, which a boolean
  -- `is_primary` could not express.
  position   integer not null default 0,
  created_at timestamptz not null default now(),
  constraint part_image_inline_or_blob check (
    (blake3 is null) != (image_webp is null)
  ),
  constraint part_image_known_origin check (
    origin in ('uploaded', 'url_supplied', 'og_fetched', 'rendered')
  )
);

-- The gallery read: every image for one part, in order. `position` then `id` so that two
-- images at the same position have a stable order rather than whatever the heap returns.
create index part_image_part_id_position_idx on part_image (part_id, position, id);

-- What makes an image blob reachable, and the reason this index exists rather than being
-- left to a sequential scan: `GET /api/blob/{blake3}` asks "does anything reference these
-- bytes" on every request, and answering it by scanning `part_image` would put a scan on
-- the open path. `CLAUDE.md`: content addressing is not authorization.
create index part_image_blake3_idx on part_image (blake3) where blake3 is not null;
