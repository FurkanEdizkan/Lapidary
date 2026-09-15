-- A part's `metadata.json` written again from its rows (the goal 3 review's open points): after a custom
-- field's value changes, which only the rows held. The worker writes into model directories, so the api
-- queues a `describe_part` job, and a finished one says `described`.
alter table job drop constraint job_outcome_known;
alter table job add constraint job_outcome_known
    check (outcome is null or outcome in ('ingested', 'skipped', 'rendered', 'scanned', 'migrated', 'revised', 'unkept', 'described'));
