//! What the app-wide event stream says (`GET /api/events`, Phase 6; design in `docs/goals/phase-6.md`).
//!
//! Deliberately little: which library changed, never what changed in it. The browser asks again for
//! whatever it shows, so the server keeps no state per tab, and an event that is lost or merged with
//! another costs one extra question rather than a wrong page.

use crate::LibraryId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One event on the stream, sent as the JSON of an SSE `data:` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AppEvent {
    /// Something in this library changed: a part, or a job finishing. Ask again for what you show of it.
    Changed { library: LibraryId },
    /// The stream may have missed something — it reconnected to the database, or this subscriber fell
    /// behind. Ask again for everything.
    Resync,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_as_the_web_will_read_them() {
        let library = LibraryId::from_uuid(
            "01931b6e-0000-7000-8000-000000000001"
                .parse()
                .expect("a uuid"),
        );
        assert_eq!(
            serde_json::to_string(&AppEvent::Changed { library }).expect("serialises"),
            r#"{"type":"changed","library":"01931b6e-0000-7000-8000-000000000001"}"#
        );
        assert_eq!(
            serde_json::to_string(&AppEvent::Resync).expect("serialises"),
            r#"{"type":"resync"}"#
        );
    }
}
