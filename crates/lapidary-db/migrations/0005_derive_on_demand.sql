-- Slice 4. Two facts: a library may decline to render, and a job may report that it
-- rendered something. No table is created and no row is removed -- `part_image` lands in
-- slice 5, beside the routes that write it.

alter table library add column auto_thumbnail boolean not null default true;

-- PostgreSQL cannot modify a CHECK in place, so this is a drop and an add rather than an
-- alteration. The name is reused deliberately: one constraint, one meaning.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in ('ingested', 'skipped', 'rendered'));
