-- Search is `simple` again, for every library (DATA §3.3). The owner does not need Turkish search
-- (2026-09-15), and `0028`'s `regconfig` column was a cost as well as a feature: `pg_upgrade` refuses a
-- cluster with a `reg*` type in a user table, and compose promises the next major version is a
-- `pg_upgrade --link`. A library made `turkish` searches as written from here on.

-- Rewrites the table to recompute every row's vector, as `0022` and `0028` did. The column stays STORED
-- and its index is rebuilt. The expression is `0022`'s, so the configuration is a literal again and a
-- query's `plainto_tsquery('simple', …)` folds to a constant.
alter table part alter column search set expression as (
  setweight(to_tsvector('simple', coalesce(part_number, '')), 'A') ||
  setweight(to_tsvector('simple', name), 'B') ||
  setweight(to_tsvector('simple', lapidary_words(tags || materials)), 'C')
);

-- Nothing depends on either column once the expression no longer reads `search_config`.
alter table part drop column search_config;
alter table library drop column language;
