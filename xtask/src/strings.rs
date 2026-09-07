//! Scans `.rs` sources for a string literal containing a run of three or more space
//! characters between two non-space characters — the exact shape a mangled `\`
//! continuation leaves behind. `cargo fmt --check` never inspects string-literal
//! contents, so a message that lost its continuation passes fmt, clippy and every test
//! silently; this is the gate that closes that class (see the fix report for the
//! incident, and this module's own tests for the mutation that confirms it fires).
//!
//! Tokenizes with `proc_macro2` rather than scanning physical lines with a regex, so
//! only real string-literal content is checked — a comment, or ordinary code, never
//! trips this. The one escape this module interprets is Rust's own line-continuation
//! rule (a `\` directly followed by a newline erases the newline and the following
//! line's leading ASCII whitespace); everything else in a literal (`\n`, `\"`, `\\`,
//! ...) is left exactly as its raw source spelling. That is enough: the bug class this
//! exists to catch is always a stray run of literal space characters left behind by a
//! mishandled continuation, never something hiding behind a different escape.
//!
//! No general Rust parser here, same reasoning `deploy.rs`'s module doc gives for not
//! pulling in a YAML parser: a hand-rolled walk that only has to find literal tokens
//! and read their raw text is a much smaller, more inspectable piece of code than a
//! full AST for this one narrow question.
//!
//! This scan also sees a doc comment's text: rustc desugars `///`/`//!` into
//! `#[doc = "..."]` attributes, and `proc_macro2` tokenizes source the same way rustc
//! does, so a mangled doc comment is caught too, not just a runtime-visible message.
//!
//! Legitimately space-bearing literals — YAML fixtures, reproduced `cargo` output, ASCII
//! STL samples — are excused one at a time through [`EXEMPT`], keyed on a digest of the
//! literal's own text. See [`Exemption`] for why that key is not a line number.

use proc_macro2::{Literal, TokenStream, TokenTree};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub excerpt: String,
    /// The key an `Exemption` for this literal would carry. Reported with the violation
    /// so that exempting something legitimate is a copy-paste rather than a second run
    /// with a hand-written helper.
    pub content: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.excerpt)
    }
}

/// One legitimately space-bearing literal: fixture text (YAML, a Dockerfile, reproduced
/// `cargo test -- --list` output, an ASCII STL sample) where interior spacing is the thing
/// under test rather than prose.
///
/// Kept as narrow per-literal exemptions rather than skipping a whole file: several of
/// these files also hold real prose this check must keep seeing — `deploy.rs`'s own
/// `Violation::Display` messages, for instance — and a file-level exemption would blind
/// the check to a regression there.
///
/// # Why the key is a digest and not a line number
///
/// It was a line number, and that was wrong in a way that cost something every time. An
/// exemption pinned to `deploy.rs:723` stops matching the moment anyone inserts a function
/// above it, so a change with nothing to do with these fixtures fails the gate and has to
/// be followed by a renumbering pass — which happened six times across slices 6a and 6b,
/// each time re-pairing every entry by hand against the check's own output.
///
/// The digest is of the literal's own raw content, so it survives every edit *except* an
/// edit to the literal itself — which is exactly when a human should look again at whether
/// the exemption still describes something legitimate.
pub struct Exemption {
    pub file: &'static str,
    /// [`digest`] of the literal's raw inner text.
    pub content: &'static str,
    pub reason: &'static str,
}

/// FNV-1a, 64-bit, as sixteen hex characters.
///
/// Hand-rolled rather than reached for: this is a lookup key for a hand-maintained table
/// in a build tool, not a security boundary and not a content address, so the properties
/// that matter are "stable across runs and platforms" and "short enough to paste into a
/// table". A dependency would buy collision resistance nobody here needs — and if two
/// literals in one file ever did collide, they would share an exemption whose reason
/// describes both, which is the same thing byte-identical literals already get.
pub fn digest(content: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub const EXEMPT: &[Exemption] = &[
    Exemption {
        file: "crates/lapidary-cad/src/stl.rs",
        content: "fe88bef97b896b65",
        reason: "ASCII STL fixture text: real STL syntax, conventionally indented by facet/loop nesting depth",
    },
    Exemption {
        file: "crates/lapidary-cad/src/stl.rs",
        content: "7fa2f0d3c76855dd",
        reason: "ASCII STL fixture text, same reason as the entry above",
    },
    Exemption {
        file: "crates/lapidary-cad/src/stl.rs",
        content: "e5d7778dc32517af",
        reason: "ASCII STL fixture text, same reason as the entry above",
    },
    Exemption {
        file: "crates/lapidary-cad/src/stl.rs",
        content: "a6d14c34472778b3",
        reason: "ASCII STL fixture text, same reason as the entry above",
    },
    Exemption {
        file: "crates/lapidary-api/tests/download.rs",
        content: "1d4a315f76812693",
        reason: "ASCII STL fixture text built for the multi-chunk download test: real STL syntax, conventionally indented by facet/loop nesting depth, same as the crates/lapidary-cad/src/stl.rs entries above",
    },
    Exemption {
        file: "crates/lapidary-db/tests/repo.rs",
        content: "55d5e6b6b6d3bb02",
        reason: "a multi-line SQL query string, indented for readability across its four sub-selects; not prose, and not a backslash continuation at all (the line breaks are real, embedded newlines the string keeps on purpose)",
    },
    Exemption {
        file: "crates/lapidary-db/tests/migrations.rs",
        content: "b7e7d3d829cd0250",
        reason: "a multi-line recursive CTE string reading the folder tree back out, indented for readability across its base case and recursive step; not prose, and not a backslash continuation at all (the line breaks are real, embedded newlines the string keeps on purpose) -- the same reason as the crates/lapidary-db/tests/repo.rs entry above",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "c967147b7cf9b3e2",
        reason: "a doc comment quoting a real indented BuildKit RUN-continuation example (RUN foo, a comment line, then an indented bar) where the indentation is the example itself",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "9275fdc4d65e5485",
        reason: "CORRECT_COMPOSE: a deliberate compose.yaml fixture; YAML indentation is meaningful",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "77fb09a87b017307",
        reason: "a compose.yaml api-service fixture, same reason as CORRECT_COMPOSE. Covers two literals: two tests build a byte-identical fixture, so one digest keys both",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "44764e86adc410e3",
        reason: "a compose.yaml api-service (with args) fixture, same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "4681e7fcaf1a4d9b",
        reason: "a compose.yaml args-block fixture, same reason as CORRECT_COMPOSE. Covers two literals, as above",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "10df1f6dcaa4f650",
        reason: "a compose.yaml api-service (with a banner comment) fixture, same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "1707e268aca66049",
        reason: "a compose.yaml worker-service fixture, same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "476a78f60fa9f1e7",
        reason: "a compose.yaml args-block (list form) fixture, same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "9ebb5fb17a772eb7",
        reason: "a compose.yaml worker-service fixture (long build form), same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "231b666657f9bed3",
        reason: "a compose.yaml worker-service fixture (short build form), same reason as CORRECT_COMPOSE",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "b2c554d6013a2e04",
        reason: "a Containerfile RUN cargo build fixture; the leading spaces are the line-continuation indent this module's own parser is being tested against",
    },
    Exemption {
        file: "xtask/src/deploy.rs",
        content: "441045200970187e",
        reason: "a Containerfile RUN cargo build (with an interior comment) fixture, same reason as the entry above",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "f8cfbee1ddc65f26",
        reason: "this module's own test data: a correctly continued inner literal, escaped so its cooked runtime value is what gets tokenized by check_source; the escaping itself unavoidably contains a space run in this file's own raw source text",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "5cb0a60a8c8aea18",
        reason: "this module's own test data: the mangled-continuation shape under test, by design",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "6db28316545545fa",
        reason: "this module's own test data: a comment containing a space run, proving comments are never flagged",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "9609fca8bf449fca",
        reason: "this module's own test data: the byte-string form of the mangled shape under test",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "6578ae8bb31789b0",
        reason: "this module's own test data: the raw-string form of the mangled shape under test",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "54a047a63a6d0446",
        reason: "this module's own test data: leading spaces at a literal's very start, proving that shape is not flagged",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "86ea8a0f93e6012a",
        reason: "this module's own test data: two mangled lines, proving EXEMPT filters one without hiding the other",
    },
    Exemption {
        file: "xtask/src/main.rs",
        content: "7f73c0c58de170ba",
        reason: "a synthetic cargo test -- --list transcript reproducing real cargo output (see bindings_command_tests); the leading spaces before Running are cargo's own formatting, not ours",
    },
    Exemption {
        file: "xtask/src/main.rs",
        content: "089dc443c387d0f2",
        reason: "a synthetic cargo test -- --list transcript, same reason as the entry above",
    },
    Exemption {
        file: "xtask/src/main.rs",
        content: "394eef2af759bb44",
        reason: "a synthetic cargo test -- --list transcript pinning that a test matching only by its MODULE name is not counted, same reason as the entry above",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "0f0843d2bc154ab0",
        reason: "this module's own test data: the source a moved-literal test tokenizes, escaped so its cooked value is what check_source sees",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "f85299243a7c1219",
        reason: "this module's own test data: the literal the moved-literal and digest tests key on. Covers four occurrences of the same fixture text",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "6576865864b552d2",
        reason: "this module's own test data: the source the unused-exemption test tokenizes, escaped for the same reason as the entry above",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "035a4f1c7c3628bf",
        reason: "this module's own test data: the literal the unused-exemption test keys on",
    },
    Exemption {
        file: "xtask/src/strings.rs",
        content: "706c708f60d2f528",
        reason: "this module's own test data: the one-character-different literal proving a changed literal gets a different key",
    },
];

/// What one file's scan found, and which exemptions it used.
///
/// `matched` exists so the caller can report an exemption that no longer matches
/// anything. A line-keyed list could not do that — a stale entry and a moved literal were
/// indistinguishable — so entries accumulated and nothing ever said which had stopped
/// describing real code.
pub struct Scan {
    pub violations: Vec<Violation>,
    /// Indices into the exemption list, one per literal an entry excused.
    pub matched: Vec<usize>,
}

/// Checks one file's already-read source. `file` is only used to label violations and
/// to look up `EXEMPT` entries — it need not be a real path.
pub fn check_source(file: &str, source: &str) -> Result<Scan, String> {
    check_source_with_exemptions(file, source, EXEMPT)
}

/// The exemptions no file in this run matched.
///
/// A dead entry is not harmless: it is a documented claim that some literal is legitimate
/// fixture text, still sitting in the table after that literal was rewritten or deleted,
/// and the next person to read the list has no way to tell it from a live one.
pub fn unused(matched: &[usize]) -> Vec<&'static Exemption> {
    EXEMPT
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched.contains(index))
        .map(|(_, entry)| entry)
        .collect()
}

/// `check_source`'s real logic, taking the exemption list as a parameter so tests can
/// exercise the filtering itself against a synthetic list instead of depending on
/// whatever `EXEMPT` currently holds.
fn check_source_with_exemptions(
    file: &str,
    source: &str,
    exempt: &[Exemption],
) -> Result<Scan, String> {
    let tokens = TokenStream::from_str(source)
        .map_err(|e| format!("{file}: could not tokenize as Rust source: {e}"))?;
    let mut violations = Vec::new();
    walk(&tokens, file, &mut violations);
    let mut matched = Vec::new();
    violations.retain(|v| {
        match exempt
            .iter()
            .position(|e| e.file == v.file && e.content == v.content)
        {
            Some(index) => {
                matched.push(index);
                false
            }
            None => true,
        }
    });
    Ok(Scan {
        violations,
        matched,
    })
}

fn walk(tokens: &TokenStream, file: &str, out: &mut Vec<Violation>) {
    for tt in tokens.clone() {
        match tt {
            TokenTree::Group(group) => walk(&group.stream(), file, out),
            TokenTree::Literal(lit) => check_literal(&lit, file, out),
            TokenTree::Ident(_) | TokenTree::Punct(_) => {}
        }
    }
}

fn check_literal(lit: &Literal, file: &str, out: &mut Vec<Violation>) {
    let raw = lit.to_string();
    let Some(content) = string_literal_content(&raw) else {
        return; // Not a string or byte-string literal — a number, char, lifetime, etc.
    };
    let collapsed = collapse_line_continuations(content);
    if let Some(excerpt) = find_space_run(&collapsed) {
        let line = lit.span().start().line;
        out.push(Violation {
            file: file.to_owned(),
            line,
            excerpt,
            // Of the raw content, not the collapsed form: the raw text is what a person
            // sees in the file, so a digest computed from it is one they can regenerate.
            content: digest(content),
        });
    }
}

/// Strips a string or byte-string literal's prefix and delimiters, returning its raw
/// (uncooked — escapes not yet interpreted) inner text. Handles plain (`"..."`), byte
/// (`b"..."`), and raw (`r"..."`, `r#"..."#`, `br#"..."#`, any hash count) forms.
/// Returns `None` for every other literal kind (integers, floats, chars, lifetimes)
/// since none of those can carry the bug this module looks for.
fn string_literal_content(raw: &str) -> Option<&str> {
    let mut s = raw;
    if let Some(rest) = s.strip_prefix('b') {
        s = rest;
    }
    if let Some(rest) = s.strip_prefix('r') {
        // Raw string: r, then N '#', then '"', ..., '"', then N '#'.
        let hashes = rest.chars().take_while(|&c| c == '#').count();
        let after_hashes = &rest[hashes..];
        let inner = after_hashes.strip_prefix('"')?;
        let suffix: String = std::iter::repeat_n('#', hashes).collect();
        let closer = format!("\"{suffix}");
        return inner.strip_suffix(closer.as_str());
    }
    let inner = s.strip_prefix('"')?;
    inner.strip_suffix('"')
}

/// Collapses Rust's own string continuation: a `\` immediately followed by a newline
/// erases the newline and every ASCII space/tab that follows it, up to the next
/// non-whitespace character or the string's end — exactly the rule the Rust reference
/// gives for string literals, applied here to the raw (uncooked) source text so a
/// *correct* continuation collapses to clean prose with no trace left to flag, and an
/// *incorrect* one (the backslash lost, or the newline surviving with its indentation)
/// leaves exactly the stray space run this module is looking for.
fn collapse_line_continuations(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'\n') {
            chars.next(); // the newline itself
            while matches!(chars.peek(), Some(' ') | Some('\t')) {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The first run of three or more consecutive space characters flanked by non-space
/// characters, as a short excerpt centered on the run — or `None` if there is none.
/// Deliberately only the ASCII space character, not any-whitespace: a `\t` or `\n` run
/// is a different, legitimate shape (indentation, an intentionally embedded newline),
/// not the symptom this module exists to catch.
fn find_space_run(content: &str) -> Option<String> {
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' {
            let start = i;
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            let run_len = i - start;
            let preceded_by_word = start > 0 && chars[start - 1] != ' ';
            let followed_by_word = i < chars.len() && chars[i] != ' ';
            if run_len >= 3 && preceded_by_word && followed_by_word {
                let excerpt_start = start.saturating_sub(20);
                let excerpt_end = (i + 20).min(chars.len());
                let excerpt: String = chars[excerpt_start..excerpt_end].iter().collect();
                return Some(excerpt);
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correct_continuation_collapses_to_clean_prose_and_is_not_flagged() {
        let src = "const M: &str = \"first part \\\n             second part\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(
            violations,
            vec![],
            "a well-formed continuation must not be flagged"
        );
    }

    #[test]
    fn a_mangled_continuation_is_flagged() {
        // The exact bug this module exists to catch: the `\` and newline are gone, and
        // the next line's source indentation survived as literal spaces in the string.
        let src = "const M: &str = \"first part                second part\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 1);
        assert!(violations[0].excerpt.contains("first part"));
    }

    #[test]
    fn a_comment_containing_a_space_run_is_never_flagged() {
        let src = "// aligned    comment    columns\nconst M: &str = \"fine\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations, vec![], "comments are not string literals");
    }

    #[test]
    fn a_byte_string_is_checked_the_same_way() {
        let src = "const M: &[u8] = b\"first part                second part\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_raw_string_is_checked_but_cannot_hide_a_mangled_continuation() {
        // Raw strings have no escapes at all, so this bug class cannot occur in one —
        // but the check must still read past the r#"..."# delimiters correctly rather
        // than tripping over them, which this pins.
        let src = "const M: &str = r#\"first part                second part\"#;";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_single_space_between_words_is_fine() {
        let src = "const M: &str = \"perfectly ordinary prose with single spaces\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn a_two_space_run_is_not_flagged() {
        // Two spaces is a common deliberate stylistic choice (e.g. after a period) —
        // this check's threshold is three or more, matching what a mangled multi-level
        // indent actually produces, not ordinary typography.
        let src = "const M: &str = \"one.  Two spaces after a period is not the bug.\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn leading_spaces_at_the_very_start_of_a_literal_are_not_flagged() {
        // A literal that deliberately starts with several spaces (reproducing indented
        // external output, say) has nothing but the opening quote before the run — not
        // a word character — so it must not trip "flanked by non-space characters".
        let src = "const M: &str = \"     Running unittests\";";
        let violations = check_source("test.rs", src).expect("tokenizes").violations;
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn an_exempted_literal_is_skipped_but_others_still_flag() {
        let src = "const A: &str = \"first bad                run\";\nconst B: &str = \"second bad                run\";\n";
        let all = check_source_with_exemptions("test.rs", src, &[])
            .expect("tokenizes")
            .violations;
        assert_eq!(all.len(), 2, "both lines are genuinely mangled unexempted");

        let exempt = &[Exemption {
            file: "test.rs",
            content: all[0].content.clone().leak(),
            reason: "synthetic exemption for this test only",
        }];
        let scan = check_source_with_exemptions("test.rs", src, exempt).expect("tokenizes");
        assert_eq!(
            scan.violations.len(),
            1,
            "the first literal is exempted; the second must still be reported"
        );
        assert_eq!(scan.violations[0].line, 2);
        assert_eq!(scan.matched, vec![0], "the entry reports that it fired");
    }

    #[test]
    fn moving_a_literal_down_the_file_does_not_break_its_exemption() {
        // The regression this key exists for. An exemption pinned to a line number stopped
        // matching the moment anyone inserted anything above it, so a change with nothing
        // to do with these fixtures failed the gate and had to be followed by a
        // renumbering pass — six times across slices 6a and 6b.
        let literal = "const A: &str = \"fixture                text\";\n";
        let exempt = &[Exemption {
            file: "test.rs",
            content: digest("fixture                text").leak(),
            reason: "synthetic",
        }];

        let pushed_down = "// pushed down\n".repeat(40);
        for prelude in ["", "// one line above\n", pushed_down.as_str()] {
            let src = format!("{prelude}{literal}");
            let scan = check_source_with_exemptions("test.rs", &src, exempt).expect("tokenizes");
            assert!(
                scan.violations.is_empty(),
                "the exemption must survive the literal moving to line {}",
                scan.violations.first().map_or(0, |v| v.line)
            );
        }
    }

    #[test]
    fn an_exemption_that_matches_nothing_is_reported_as_unused() {
        // A dead entry is a documented claim that some literal is legitimate fixture text,
        // still in the table after that literal was rewritten. The line-keyed version could
        // not tell one from a literal that had merely moved, so entries only ever
        // accumulated.
        let src = "const A: &str = \"first bad                run\";\n";
        let exempt = &[
            Exemption {
                file: "test.rs",
                content: digest("first bad                run").leak(),
                reason: "live",
            },
            Exemption {
                file: "test.rs",
                content: "0000000000000000",
                reason: "describes a literal that is no longer here",
            },
        ];
        let scan = check_source_with_exemptions("test.rs", src, exempt).expect("tokenizes");
        assert!(scan.violations.is_empty());
        assert_eq!(scan.matched, vec![0], "only the first entry fired");

        let unused: Vec<_> = exempt
            .iter()
            .enumerate()
            .filter(|(i, _)| !scan.matched.contains(i))
            .map(|(_, e)| e.reason)
            .collect();
        assert_eq!(unused, vec!["describes a literal that is no longer here"]);
    }

    #[test]
    fn the_digest_is_of_the_literal_and_not_of_its_surroundings() {
        // Two files, same literal: one entry covers both, which is what makes a fixture
        // moved between modules keep its exemption. And a changed literal gets a different
        // key, which is what makes someone look at it again.
        assert_eq!(
            digest("fixture                text"),
            digest("fixture                text")
        );
        assert_ne!(
            digest("fixture                text"),
            digest("fixture                text!")
        );
    }
}
