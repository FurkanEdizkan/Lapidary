-- Phase 4 slice 1: a check-out lock on a part. See
-- docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md §5.
--
-- A lock is on a part, not on a revision: it is taken before the next revision exists,
-- which is also why the two unused columns 0002 put on `revision` go.
create table part_lock (
    id           uuid primary key,
    -- Purge removes the part and everything that was about it; a lock is not user data.
    part_id      uuid        not null references part(id) on delete cascade,
    -- Free text (`$USER@$HOSTNAME` from the agent): there are no users yet, and the spec says
    -- so rather than inventing an identity nothing checks.
    holder       text        not null check (btrim(holder) <> '' and length(holder) <= 200),
    taken_at     timestamptz not null default now(),
    released_at  timestamptz,
    released_by  text,
    -- Released by somebody other than the holder checking it in.
    forced       boolean     not null default false,
    -- A release says when and who together, or neither.
    constraint part_lock_release_is_whole check ((released_at is null) = (released_by is null))
);

-- One active lock per part, enforced by the index rather than trusted to the writer.
create unique index part_lock_one_active_per_part on part_lock (part_id)
    where released_at is null;

alter table revision drop column locked_by, drop column locked_at;
