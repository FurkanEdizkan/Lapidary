-- Saved filters are ordered by hand (the local product spec §3).
--
-- Every existing filter takes its place in the order it was made, per library. A new filter goes last,
-- and a move swaps a filter with its neighbour under the library's row lock, renumbering first so that
-- two filters sharing a position can still be told apart. There is no unique (library_id, position):
-- a swap passes through a moment where two rows hold one position.
alter table saved_filter add column position integer;

update saved_filter s set position = ranked.n
  from (select id, (row_number() over (partition by library_id order by created_at, id) - 1)::integer as n
          from saved_filter) ranked
 where ranked.id = s.id;

alter table saved_filter alter column position set not null;
