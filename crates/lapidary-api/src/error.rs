//! Errors raised while building or configuring this crate's router, as opposed to errors
//! raised by a handler at request time (those stay local to their handler module, e.g.
//! `health::healthz` builds its own JSON body).
//!
//! One thing about request-time errors does live here, because every handler module makes the same
//! decision about it: [`log_db_error`], the level a database error deserves.

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(
        "`{got}` is not a role. Set LAPIDARY_ROLE to `api` (serves the grid and the open \
         path) or `worker` (runs ingest). deploy/compose.yaml sets it per service."
    )]
    UnknownRole { got: String },
}

/// One line for a database error, at the level it deserves, naming the route or widget that asked.
///
/// **A statement the server cancelled is a warning, not an error.** `lapidary_db::INTERACTIVE` is
/// what stopped it, on purpose; nothing was written, the caller is told to try again, and an
/// operator whose alerting watches ERROR should not be woken by contention. It still gets a line —
/// silence is what kept the pinned connection G4 found invisible until somebody locked a table on
/// purpose, and the `what` each caller passes is what says which route or widget it was.
///
/// Every `internal_error` in this crate logs through here, which is why it is one function in one
/// place: the seven of them each keep their own response shape and share this decision. Several
/// handlers log a `DbError` outside that family (`tags`, `filters`, `part_number`, `moves`'s rename,
/// `blob`'s reachability sweep); those still log at error, and a cancelled statement reaching one of
/// them is over-loud rather than silent.
pub(crate) fn log_db_error(err: &lapidary_db::DbError, what: &str) {
    if err.gave_up() {
        tracing::warn!(error = %err, "{what}");
    } else {
        tracing::error!(error = %err, "{what}");
    }
}

#[cfg(test)]
mod tests {
    use super::log_db_error;
    use std::borrow::Cow;
    use std::sync::{Arc, Mutex};

    /// A `lock_timeout` cancellation, which is what the api sees when a widget's read is held up by
    /// a lock. sqlx will not let a `PgDatabaseError` be built outside its own crate, so this is the
    /// same stand-in `lapidary-db`'s own tests use, down to `code()` being the only field the code
    /// under test reads.
    #[derive(Debug)]
    struct Cancelled;

    impl std::fmt::Display for Cancelled {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("canceling statement due to lock timeout")
        }
    }

    impl std::error::Error for Cancelled {}

    impl sqlx::error::DatabaseError for Cancelled {
        fn message(&self) -> &str {
            "canceling statement due to lock timeout"
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("55P03"))
        }
        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
    }

    /// Captures every event's level and fields, the shape `lapidary-peer`'s refusal test uses.
    struct Logged(Arc<Mutex<Vec<(tracing::Level, String)>>>);

    impl tracing::Subscriber for Logged {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut line = String::new();
            event.record(&mut Fields(&mut line));
            self.0
                .lock()
                .expect("the log")
                .push((*event.metadata().level(), line));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    struct Fields<'a>(&'a mut String);

    impl tracing::field::Visit for Fields<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!("{}={value:?} ", field.name()));
        }
    }

    /// One line, at warn, naming what asked — and an ordinary failure still at error, because the
    /// point is the difference between them. Without the second half this would pass on a helper
    /// that logged everything at warn, which would lose the level a real fault needs.
    #[test]
    fn a_cancelled_statement_is_one_warning_naming_the_widget_and_a_real_fault_is_still_an_error() {
        let logged = Arc::new(Mutex::new(Vec::new()));
        {
            let _capturing = tracing::subscriber::set_default(Logged(logged.clone()));
            log_db_error(
                &lapidary_db::DbError::Query(sqlx::Error::Database(Box::new(Cancelled))),
                "dashboard queue widget read failed",
            );
            log_db_error(
                &lapidary_db::DbError::Query(sqlx::Error::PoolClosed),
                "dashboard queue widget read failed",
            );
        }

        let lines = logged.lock().expect("the log").clone();
        assert_eq!(lines.len(), 2, "one line each and no more: {lines:?}");
        assert_eq!(lines[0].0, tracing::Level::WARN, "{lines:?}");
        assert!(
            lines[0].1.contains("dashboard queue widget"),
            "the widget that asked is not in the line: {:?}",
            lines[0].1
        );
        assert_eq!(
            lines[1].0,
            tracing::Level::ERROR,
            "a query that failed for any other reason is still an error: {lines:?}"
        );
    }
}
