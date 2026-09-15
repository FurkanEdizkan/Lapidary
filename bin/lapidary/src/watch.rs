//! When a checked-out file has been saved, as a pure state machine (Phase 4 slice 1 spec §6).
//!
//! Polling, not OS events: the agent reads one file's size and mtime every [`POLL`], and this
//! decides when a change has sat still long enough to be worth hashing. The clock is a
//! parameter, so the tests step time instead of sleeping through it.

use std::time::{Duration, Instant, SystemTime};

/// How often the agent looks: `docs/DATA.md` §6.2's debounce.
pub const POLL: Duration = Duration::from_millis(500);

/// How long a file must sit unchanged before it is hashed: §6.2's write-settle.
pub const SETTLE: Duration = Duration::from_secs(2);

/// One look at the file. The agent passes `None` when the file is not there, which is the
/// middle of an editor's write-a-temporary-then-rename save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub size: u64,
    pub modified: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing new, or a change still settling.
    Wait,
    /// The file changed and has now been still for [`SETTLE`]: hash it.
    Hash,
}

#[derive(Debug, Clone)]
pub struct Watch {
    last: Option<Seen>,
    changed_at: Option<Instant>,
}

impl Watch {
    /// Starts from what the file looks like now, so a file that was already there is not
    /// hashed for having been noticed.
    pub fn new(seen: Option<Seen>) -> Self {
        Self {
            last: seen,
            changed_at: None,
        }
    }

    pub fn poll(&mut self, now: Instant, seen: Option<Seen>) -> Verdict {
        if seen != self.last {
            // Any movement restarts the wait: a save still being written is not a save yet.
            self.last = seen;
            self.changed_at = Some(now);
            return Verdict::Wait;
        }
        match self.changed_at {
            Some(at) if self.last.is_some() && now.duration_since(at) >= SETTLE => {
                self.changed_at = None;
                Verdict::Hash
            }
            _ => Verdict::Wait,
        }
    }
}

/// Hash before believing anything (§6.2): the new BLAKE3 when the bytes differ from the
/// revision the checkout is of, and `None` when an editor only touched the file.
pub fn changed(known: &str, bytes: &[u8]) -> Option<String> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    (hash != known).then_some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(size: u64, modified: u64) -> Option<Seen> {
        Some(Seen {
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(modified),
        })
    }

    #[test]
    fn a_file_nobody_touches_is_never_hashed() {
        let start = Instant::now();
        let mut watch = Watch::new(seen(184_342, 100));
        for tick in 0..20 {
            assert_eq!(
                watch.poll(start + POLL * tick, seen(184_342, 100)),
                Verdict::Wait
            );
        }
    }

    #[test]
    fn a_save_is_hashed_once_after_it_has_settled() {
        let start = Instant::now();
        let mut watch = Watch::new(seen(184_342, 100));
        assert_eq!(
            watch.poll(start, seen(202_776, 160)),
            Verdict::Wait,
            "just changed"
        );
        assert_eq!(
            watch.poll(start + Duration::from_millis(1_500), seen(202_776, 160)),
            Verdict::Wait,
            "not settled yet"
        );
        assert_eq!(
            watch.poll(start + SETTLE, seen(202_776, 160)),
            Verdict::Hash
        );
        assert_eq!(
            watch.poll(start + SETTLE + POLL, seen(202_776, 160)),
            Verdict::Wait,
            "and only once"
        );
    }

    #[test]
    fn a_write_still_going_restarts_the_wait() {
        let start = Instant::now();
        let mut watch = Watch::new(seen(0, 100));
        assert_eq!(
            watch.poll(start, seen(8_192, 101)),
            Verdict::Wait,
            "a write just seen is not hashed"
        );
        let still_growing = start + Duration::from_millis(1_500);
        assert_eq!(
            watch.poll(still_growing, seen(65_536, 102)),
            Verdict::Wait,
            "nor is one still growing"
        );
        assert_eq!(
            watch.poll(start + SETTLE, seen(65_536, 102)),
            Verdict::Wait,
            "two seconds from the first change is not two seconds of stillness"
        );
        assert_eq!(
            watch.poll(still_growing + SETTLE, seen(65_536, 102)),
            Verdict::Hash
        );
    }

    #[test]
    fn a_file_mid_rename_is_hashed_only_once_it_is_back_and_still() {
        let start = Instant::now();
        let mut watch = Watch::new(seen(184_342, 100));
        assert_eq!(
            watch.poll(start, None),
            Verdict::Wait,
            "the file just went away"
        );
        assert_eq!(
            watch.poll(start + SETTLE, None),
            Verdict::Wait,
            "nothing to hash while it is gone"
        );
        let back = start + SETTLE + POLL;
        watch.poll(back, seen(202_776, 160));
        assert_eq!(watch.poll(back + SETTLE, seen(202_776, 160)), Verdict::Hash);
    }

    #[test]
    fn a_touch_that_changes_no_bytes_is_not_a_save() {
        let bytes = b"solid flange\nendsolid flange\n";
        let known = blake3::hash(bytes).to_hex().to_string();
        assert_eq!(changed(&known, bytes), None);
        assert_eq!(
            changed(&known, b"solid flange, 10% wider\nendsolid flange\n").map(|hash| hash.len()),
            Some(64)
        );
    }
}
