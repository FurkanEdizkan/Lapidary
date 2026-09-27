-- Phase 6: where the app-wide event stream's events come from (design: docs/goals/phase-6.md
-- § App-wide events). One channel, `lapidary_events`; the payload is a library id and nothing else,
-- because the browser asks again for whatever it shows (`AppEvent` in `lapidary-core`).
--
-- **This is the repository's first trigger, and the reason is fan-out, not convenience.** Every other
-- notification here is issued by the statement that caused it -- `PgJobs` calls `pg_notify` beside each
-- enqueue and each completion, `PgSharing` beside each roster change -- and that is right for them,
-- because each of those channels has exactly one writer and the notification is part of what that one
-- writer means. "Something in this library changed" has many writers: ingest, upload, a folder move, a
-- rename, remove, restore, purge, a revision, a fold, and every job that ends. A `pg_notify` call per
-- writer is a line each of them has to remember, and the first one that forgets is a dashboard that
-- silently stops updating for that one kind of change -- the worst shape of bug this application can
-- have, because it looks like nothing at all. A trigger is the one place that cannot be forgotten.
--
-- It also gets two things right for free that a hand-written call would have to earn:
--
--   * It fires inside the writing transaction, so a change that rolls back notifies nobody.
--   * PostgreSQL delivers one notification per distinct (channel, payload) per transaction, so a
--     transaction that touches four hundred parts of one library wakes a page once, not four hundred
--     times. The api groups across transactions as well (`crates/lapidary-api/src/events.rs`); this is
--     the half the database does by itself.
--
-- The cost is what any trigger costs: an extra function call per row written. That is paid on the write
-- path, never on the open path, and `pg_notify` with no listener is queued and discarded at commit.

CREATE FUNCTION lapidary_changed() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- OLD on a delete, NEW on anything else. A purge deletes `part` rows, and a part leaving is as
    -- much a change to the grid as a part arriving.
    PERFORM pg_notify(
        'lapidary_events',
        (CASE WHEN TG_OP = 'DELETE' THEN OLD.library_id ELSE NEW.library_id END)::text
    );
    RETURN NULL;
END;
$$;

-- FOR EACH ROW, not per statement: a statement-level trigger has no NEW to read a library from, and one
-- `UPDATE part ... WHERE library_id IN (...)` can touch two libraries, both of which have to hear about
-- it. Per-row is the cost the paragraph above accounts for.
CREATE TRIGGER part_changed
    AFTER INSERT OR UPDATE OR DELETE ON part
    FOR EACH ROW EXECUTE FUNCTION lapidary_changed();

-- A job is only worth waking a browser for when it ends. `pending` to `running` changes nothing anybody
-- can see; `done` or `failed` is the moment a part has its thumbnail, its rungs and its shape profile
-- (goal G2 profiles in line inside the ingest job, so a job reaching `done` has already written the
-- profile), or the moment a file's failure is worth showing. `UPDATE OF state` narrows this to the
-- statements that write that column at all, and the WHEN clause to the two states that matter.
--
-- No constraint on `job` is restated here. `0047` added `profiled` to `job_outcome_known` and this
-- migration does not touch it; a later migration that restates it keeps `profiled` and every value
-- before it.
CREATE TRIGGER job_finished
    AFTER UPDATE OF state ON job
    FOR EACH ROW WHEN (NEW.state IN ('done', 'failed'))
    EXECUTE FUNCTION lapidary_changed();
