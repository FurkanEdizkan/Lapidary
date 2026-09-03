//! What the loop does with a handler's answer. A pure function, so the policy that
//! decides whether a user's file is retried or abandoned is testable without a database,
//! a worker, or a clock.

use crate::HandlerError;
use lapidary_core::Outcome;
use std::time::Duration;

/// Fixed, not exponential-with-jitter: with `max_attempts = 3` only the third value is
/// ever reached, and a table is easier to reason about than a formula.
pub const BACKOFF: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(30),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Complete(Outcome),
    Fail { reason: String },
    Retry { reason: String, backoff: Duration },
}

pub fn next_state(result: Result<Outcome, HandlerError>, attempts: i32, max_attempts: i32) -> Next {
    match result {
        Ok(outcome) => Next::Complete(outcome),
        Err(HandlerError::Permanent { message }) => Next::Fail { reason: message },
        Err(HandlerError::Transient { message }) => {
            if attempts >= max_attempts {
                Next::Fail {
                    reason: format!("{message} Gave up after {attempts} attempts."),
                }
            } else {
                let index = (attempts.max(1) as usize - 1).min(BACKOFF.len() - 1);
                Next::Retry {
                    reason: message,
                    backoff: BACKOFF[index],
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permanent_failure_is_terminal_on_the_first_attempt() {
        // The bytes are content-addressed and immutable: attempt two would parse the
        // same bytes and produce the same error, so there is no attempt two.
        let message = "Could not read this STL - it declares 24 facets but the file ends \
                       after 11. Re-export from your CAD tool and retry."
            .to_owned();
        let next = next_state(
            Err(HandlerError::Permanent {
                message: message.clone(),
            }),
            1,
            3,
        );
        // Asserting the full value, not just the variant: a `Next::Fail` with the
        // message dropped or replaced would still match `matches!(next, Next::Fail
        // { .. })`, but `JobFailure.reason` is this exact string reaching a person
        // through the batch status API -- CLAUDE.md requires errors to say what broke
        // and what to do, so a regression that mangles the message must fail this test.
        assert_eq!(next, Next::Fail { reason: message });
    }

    #[test]
    fn a_transient_failure_retries_with_a_growing_backoff() {
        let reason = HandlerError::Transient {
            message: "The database was unreachable.".to_owned(),
        };
        assert_eq!(
            next_state(Err(reason.clone()), 1, 3),
            Next::Retry {
                reason: "The database was unreachable.".to_owned(),
                backoff: Duration::from_secs(2),
            }
        );
        assert_eq!(
            next_state(Err(reason), 2, 3),
            Next::Retry {
                reason: "The database was unreachable.".to_owned(),
                backoff: Duration::from_secs(8),
            }
        );
    }

    #[test]
    fn a_transient_failure_on_the_last_attempt_is_terminal() {
        let next = next_state(
            Err(HandlerError::Transient {
                message: "The database was unreachable.".to_owned(),
            }),
            3,
            3,
        );
        match next {
            Next::Fail { reason } => assert!(
                reason.contains("Gave up after 3 attempts"),
                "the message must say why it stopped trying, got: {reason}"
            ),
            other => panic!("expected a terminal failure, got {other:?}"),
        }
    }

    #[test]
    fn a_success_carries_its_outcome_through() {
        assert_eq!(
            next_state(Ok(Outcome::Skipped), 1, 3),
            Next::Complete(Outcome::Skipped)
        );
    }

    // The four tests above exercise every arm of `next_state`'s match, but with
    // `max_attempts` fixed at 3 the backoff index never climbs past 1 (`BACKOFF[1]`,
    // the 8s entry) before the cap kicks in and routes to `Fail`. `BACKOFF[2]`, the 30s
    // entry, and the `.min(BACKOFF.len() - 1)` clamp that reaches it, are exercised only
    // when `max_attempts` exceeds the table's length -- so that boundary is added here
    // rather than assumed from the brief's four tests.
    #[test]
    fn a_transient_failure_backoff_clamps_to_the_last_table_entry() {
        let reason = HandlerError::Transient {
            message: "The blob store timed out.".to_owned(),
        };
        // attempts = 3 with max_attempts = 5 is still under the cap, and the naive index
        // (attempts - 1 = 2) lands exactly on BACKOFF[2] -- no clamp needed here, but it
        // confirms the third table entry is reachable at all.
        assert_eq!(
            next_state(Err(reason.clone()), 3, 5),
            Next::Retry {
                reason: "The blob store timed out.".to_owned(),
                backoff: Duration::from_secs(30),
            }
        );
        // attempts = 4 with max_attempts = 5 is where the clamp actually does work: the
        // naive index (3) would run off the end of a 3-element table, so `.min(2)` must
        // hold it at BACKOFF[2] instead of panicking on out-of-bounds access.
        assert_eq!(
            next_state(Err(reason), 4, 5),
            Next::Retry {
                reason: "The blob store timed out.".to_owned(),
                backoff: Duration::from_secs(30),
            }
        );
    }
}
