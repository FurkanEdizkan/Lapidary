-- Phase 4 slice 1: a changed file becomes a revision instead of vanishing as "already
-- here". See docs/superpowers/specs/2026-09-14-phase-4-slice-1-revisions-design.md.

-- Two new ways for a job to finish. `revised`: a controlled library kept the change as a new
-- revision. `unkept`: a hobby library saw the change and, keeping no revisions, did not
-- store it -- a notice rather than a failure, because nothing broke and a retry cannot
-- change the answer. Drop and add, as 0005 and 0010 did: a CHECK cannot be altered.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in
           ('ingested', 'skipped', 'rendered', 'scanned', 'migrated', 'revised', 'unkept'));

-- A label names one revision of one part. The next label is read under the part's row lock,
-- so two writers cannot both pick it; this is what says so if that ever stops being true.
-- Every part written before this migration holds exactly one revision, labelled '1'.
alter table revision
    add constraint revision_label_unique_per_part unique (part_id, rev_label);
