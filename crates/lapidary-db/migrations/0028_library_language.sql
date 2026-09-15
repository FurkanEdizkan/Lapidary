-- A library's search language (DATA §3.3; the local product spec §2).
--
-- `simple` or `turkish`, chosen when the library is made. A text search configuration is fixed when
-- a row is indexed, so it belongs to the library rather than to a global setting.
alter table library add column language text not null default 'simple'
  constraint library_language_known check (language in ('simple', 'turkish'));

-- Each part's configuration, copied from its library in the insert that makes the part. A generated
-- column may read another column of its own row and not the library's, and `to_tsvector(regconfig,
-- text)` is immutable, so the search column stays STORED and generated rather than kept by a trigger.
-- Moves stay within a library, so nothing ever needs to write this again.
alter table part add column search_config regconfig not null default 'simple';

-- Rewrites the table to recompute every row's vector, as `0022` did. Every existing part is `simple`,
-- so each vector comes out as it was.
alter table part alter column search set expression as (
  setweight(to_tsvector(search_config, coalesce(part_number, '')), 'A') ||
  setweight(to_tsvector(search_config, name), 'B') ||
  setweight(to_tsvector(search_config, lapidary_words(tags || materials)), 'C')
);
