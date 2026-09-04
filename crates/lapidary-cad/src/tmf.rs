//! 3MF: a mesh in XML inside an OPC package, which is a ZIP.
//!
//! Unlike `stl.rs` and `obj.rs` this reader is not hand-rolled. The container is a
//! security boundary — zip64, data descriptors, local-versus-central header mismatch —
//! and a bug here is a vulnerability rather than a wrong mesh. See spec §3.2.

// Not called outside this module's tests yet: task 4 wires `read_capped`/`Caps` into
// the archive reader. Delete this line then.
#![allow(dead_code)]

use crate::kernel::CadError;
use std::io::Read;

pub(crate) const FORMAT: &str = "3MF";

/// Limits on what an archive may expand to. `DATA.md` §5.4 requires all three.
///
/// A struct rather than three constants so tests can inject small values: proving the
/// 2 GiB cap fires would otherwise need a 2 GiB fixture in the repository. The mechanism
/// is the part that can break, and small caps exercise the same mechanism.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Caps {
    pub(crate) max_decompressed: u64,
    pub(crate) max_entries: usize,
    pub(crate) max_ratio: u64,
}

impl Caps {
    /// Sized in spec §3.4 against `DATA.md`'s 1 MB – 2 GB source range. Compiled in and
    /// deliberately not configurable: a security control with an environment override is
    /// one an operator can switch off by accident.
    pub(crate) const DEFAULT: Caps = Caps {
        max_decompressed: 2 << 30,
        max_entries: 1024,
        max_ratio: 200,
    };
}

/// Read at most `cap` bytes, refusing rather than allocating when the stream is longer.
///
/// `DATA.md` §5.4 says to abort "on breach, not after", and that is this function's whole
/// reason to exist: `Read::take(cap + 1)` means a hostile entry costs `cap + 1` bytes of
/// memory regardless of what it claims to expand to. Reading first and measuring second
/// is the bomb working exactly as designed.
pub(crate) fn read_capped<R: Read>(reader: R, cap: u64) -> Result<Vec<u8>, CadError> {
    let mut out = Vec::new();
    // cap + 1: reading exactly `cap` cannot distinguish "ended at the cap" from
    // "continues past it", and a file of exactly the documented maximum size is legal.
    reader
        .take(cap.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|source| CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("the archive could not be read: {source}"),
        })?;
    if out.len() as u64 > cap {
        return Err(CadError::ArchiveRefused {
            format: FORMAT.to_owned(),
            detail: format!("one entry expands past the {cap}-byte limit"),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bounded source that records how many bytes were actually pulled from it.
    ///
    /// Bounded on purpose. An infinite reader proves the same point more elegantly, but
    /// the naive implementation this test exists to catch calls `read_to_end` on it and
    /// allocates until the machine dies — an OOM kill, not a test failure, and it takes
    /// the developer's editor down with it. That happened twice while this slice was
    /// being written. A finite source plus a byte counter gives a deterministic red test
    /// for 64 KiB.
    ///
    /// Also deliberately not `std::io::repeat`: std specialises `Repeat::read_to_end` to
    /// fail with `OutOfMemory` immediately, so the naive version would return an error
    /// too and the test would pass against the exact bug it exists to catch.
    struct Counted<'a> {
        remaining: usize,
        pulled: &'a std::cell::Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.remaining);
            buf[..n].fill(0);
            self.remaining -= n;
            self.pulled.set(self.pulled.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn reading_stops_at_the_cap_rather_than_after_it() {
        // `DATA.md` §5.4 says abort "on breach, not after", and the byte count is what
        // tells those two apart. The source holds 64 KiB against a 1 KiB cap: capping as
        // bytes arrive pulls about 1 KiB and stops, while reading everything and
        // measuring afterwards pulls all 64 KiB. Both return an error, so asserting on
        // the error alone would certify nothing.
        let pulled = std::cell::Cell::new(0);
        let source = Counted {
            remaining: 64 * 1024,
            pulled: &pulled,
        };
        let err = read_capped(source, 1024).expect_err("must refuse");
        assert!(matches!(err, CadError::ArchiveRefused { .. }), "{err}");
        assert!(
            pulled.get() <= 1024 + 4096,
            "pulled {} bytes for a 1024-byte cap: the cap must bound the read, not just \
             the result",
            pulled.get()
        );
    }

    #[test]
    fn a_stream_inside_the_cap_is_returned_whole() {
        let bytes = read_capped(&b"3MF"[..], 1024).expect("reads");
        assert_eq!(bytes, b"3MF");
    }

    #[test]
    fn a_stream_exactly_at_the_cap_is_allowed() {
        // Off-by-one guard: the cap is a maximum, not a strict bound. A 1024-byte entry
        // under a 1024-byte cap is legal, and a parser that refused it would reject
        // files for being exactly the documented size.
        let bytes = read_capped(&[7u8; 1024][..], 1024).expect("reads");
        assert_eq!(bytes.len(), 1024);
    }

    #[test]
    fn the_default_caps_are_the_documented_ones() {
        // Spec §3.4. These are a security control; a silent edit should fail a test.
        assert_eq!(Caps::DEFAULT.max_decompressed, 2 << 30);
        assert_eq!(Caps::DEFAULT.max_entries, 1024);
        assert_eq!(Caps::DEFAULT.max_ratio, 200);
    }
}
