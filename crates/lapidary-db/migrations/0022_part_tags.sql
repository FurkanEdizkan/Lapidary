-- The tags a person gives a part, and the tag facet's column. Shaped like `materials` (`0021`) for
-- the same reasons: a typed array rather than a path into `metadata_json`, `not null default '{}'`
-- so an untagged part is an empty array, and GIN because the queries test `tags @> array[$n]`.
alter table part add column tags text[] not null default '{}';

create index part_tags on part using gin (tags);

-- Search reads tags and materials too, at weight C: below a part number (A) and a name (B), so a
-- word that is also a part's name still ranks that part first.
--
-- A generated column accepts only IMMUTABLE functions and `array_to_string` is only STABLE, so it
-- is wrapped in one that says what is true of joining text with a space: nothing but its input
-- decides the result.
create function lapidary_words(text[]) returns text language sql immutable parallel safe
  as $$ select array_to_string($1, ' ') $$;

-- Rewrites the table to recompute every row's vector. The column stays STORED, which `SET
-- EXPRESSION` keeps and `the_search_column_is_stored_not_virtual` checks.
alter table part alter column search set expression as (
  setweight(to_tsvector('simple', coalesce(part_number, '')), 'A') ||
  setweight(to_tsvector('simple', name), 'B') ||
  setweight(to_tsvector('simple', lapidary_words(tags || materials)), 'C')
);
