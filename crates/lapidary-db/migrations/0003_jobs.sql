-- Slice 2's queue. One row per file; batch_id groups the rows one scan created and is
-- deliberately NOT a table of its own -- see the design doc, section 3.1. Nothing here
-- is denormalized, so nothing here can go stale.
create table job (
    id               uuid primary key,
    batch_id         uuid        not null,
    library_id       uuid        not null references library(id),
    kind             text        not null,
    payload          jsonb       not null,
    state            text        not null default 'pending',
    outcome          text,
    attempts         int         not null default 0,
    max_attempts     int         not null default 3,
    run_after        timestamptz not null default now(),
    leased_by        text,
    lease_expires_at timestamptz,
    last_error       text,
    created_at       timestamptz not null default now(),
    updated_at       timestamptz not null default now(),

    constraint job_state_known
        check (state in ('pending', 'running', 'done', 'failed')),
    constraint job_outcome_known
        check (outcome is null or outcome in ('ingested', 'skipped')),
    -- A finished job says how it finished, and only a finished job may: an outcome on a
    -- non-terminal row is incoherent either way, so both directions of this one are
    -- meaningful.
    constraint job_done_has_outcome
        check ((state = 'done') = (outcome is not null)),
    -- A failed job says why. This is an implication, not an equivalence, on purpose: a
    -- job that is retrying after a transient failure keeps its last error, so an operator
    -- can see why a batch is slow without waiting for it to exhaust its attempts. Only
    -- the 'failed' direction is mandatory.
    constraint job_failed_has_reason
        check (state <> 'failed' or last_error is not null)
);

-- The dequeue index. Partial: only pending rows are ever ordered by run_after, and this
-- table is expected to accumulate 'done' rows without bound (design doc, section 11).
create index job_dequeue_idx on job (run_after) where state = 'pending';

-- Reclaiming an expired lease -- the other arm of the dequeue's WHERE.
create index job_expired_lease_idx on job (lease_expires_at) where state = 'running';

-- BatchStatus' GROUP BY.
create index job_batch_idx on job (batch_id);

-- At-least-once delivery means two workers can genuinely race the same file after a
-- lease expiry, and library_holds is not atomic with the insert -- so no amount of
-- checking harder prevents the duplicate. This constraint is what makes the race safe;
-- the handler maps its violation to Skipped.
--
-- The key must agree with PgBlobs::library_holds, which deliberately does NOT filter
-- deleted_at (slice 1, ledger item S11: a re-scan reports a soft-deleted part skipped
-- rather than resurrecting it). If one of the two starts filtering and the other does
-- not, library_holds returns false and this constraint throws on a path with no reason
-- to expect it.
alter table part add constraint part_name_unique_per_library unique (library_id, name);

-- Rider from slice 1's ledger, item S3: a revision has at most one derivative of each
-- kind. Scheduled there for "slice 2's first migration"; this is it.
alter table derivative add constraint derivative_kind_unique_per_revision
    unique (revision_id, kind);
