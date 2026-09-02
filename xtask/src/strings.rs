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

use proc_macro2::{Literal, TokenStream, TokenTree};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub excerpt: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.excerpt)
    }
}

/// A (file, line, reason) triple whose flagged literal is legitimately space-bearing —
/// fixture text (YAML, a Dockerfile, reproduced cargo test -- --list output, an ASCII
/// STL sample) where interior spacing is the thing under test, not prose. Kept as
/// narrow per-entry exemptions rather than skipping a whole file: several of these
/// files also hold real prose this check must keep seeing (deploy.rs's own
/// Violation::Display messages, for instance), and a file-level exemption would blind
/// the check to a regression there. `line` is the literal's *starting* line — the same
/// number check_source reports for a real violation, so updating this list after a
/// genuine edit is a matter of re-running the check and copying the number it prints.
pub const EXEMPT: &[(&str, usize, &str)] = &[
    (
        "crates/lapidary-cad/src/stl.rs",
        259,
        "ASCII STL fixture text: real STL syntax, conventionally indented by facet/loop nesting depth",
    ),
    (
        "crates/lapidary-cad/src/stl.rs",
        339,
        "ASCII STL fixture text, same reason as line 259",
    ),
    (
        "crates/lapidary-cad/src/stl.rs",
        363,
        "ASCII STL fixture text, same reason as line 259",
    ),
    (
        "crates/lapidary-cad/src/stl.rs",
        389,
        "ASCII STL fixture text, same reason as line 259",
    ),
    (
        "crates/lapidary-db/tests/repo.rs",
        57,
        "a multi-line SQL query string, indented for readability across its four sub-selects; not prose, and not a backslash continuation at all (the line breaks are real, embedded newlines the string keeps on purpose)",
    ),
    (
        "xtask/src/deploy.rs",
        375,
        "a doc comment quoting a real indented BuildKit RUN-continuation example (RUN foo, a comment line, then an indented bar) where the indentation is the example itself",
    ),
    (
        "xtask/src/deploy.rs",
        539,
        "CORRECT_COMPOSE: a deliberate compose.yaml fixture; YAML indentation is meaningful",
    ),
    (
        "xtask/src/deploy.rs",
        607,
        "a compose.yaml api-service fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        608,
        "a compose.yaml api-service (with args) fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        624,
        "a compose.yaml args-block fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        646,
        "a compose.yaml api-service fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        647,
        "a compose.yaml api-service (with a banner comment) fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        665,
        "a compose.yaml worker-service fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        686,
        "a compose.yaml args-block fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        687,
        "a compose.yaml args-block (list form) fixture, same reason as CORRECT_COMPOSE",
    ),
    (
        "xtask/src/deploy.rs",
        843,
        "a Containerfile RUN cargo build fixture; the leading spaces are the line-continuation indent this module's own parser is being tested against",
    ),
    (
        "xtask/src/deploy.rs",
        884,
        "a Containerfile RUN cargo build (with an interior comment) fixture, same reason as line 843",
    ),
    (
        "xtask/src/strings.rs",
        318,
        "this module's own test data: a correctly continued inner literal, escaped so its cooked runtime value is what gets tokenized by check_source; the escaping itself unavoidably contains a space run in this file's own raw source text",
    ),
    (
        "xtask/src/strings.rs",
        331,
        "this module's own test data: the mangled-continuation shape under test, by design",
    ),
    (
        "xtask/src/strings.rs",
        340,
        "this module's own test data: a comment containing a space run, proving comments are never flagged",
    ),
    (
        "xtask/src/strings.rs",
        347,
        "this module's own test data: the byte-string form of the mangled shape under test",
    ),
    (
        "xtask/src/strings.rs",
        357,
        "this module's own test data: the raw-string form of the mangled shape under test",
    ),
    (
        "xtask/src/strings.rs",
        384,
        "this module's own test data: leading spaces at a literal's very start, proving that shape is not flagged",
    ),
    (
        "xtask/src/strings.rs",
        391,
        "this module's own test data: two mangled lines, proving EXEMPT filters one without hiding the other",
    ),
    (
        "xtask/src/main.rs",
        400,
        "a synthetic cargo test -- --list transcript reproducing real cargo output (see export_bindings_tests); the leading spaces before Running are cargo's own formatting, not ours",
    ),
    (
        "xtask/src/main.rs",
        412,
        "a synthetic cargo test -- --list transcript, same reason as line 400",
    ),
];

/// Checks one file's already-read source. `file` is only used to label violations and
/// to look up `EXEMPT` entries — it need not be a real path.
pub fn check_source(file: &str, source: &str) -> Result<Vec<Violation>, String> {
    check_source_with_exemptions(file, source, EXEMPT)
}

/// `check_source`'s real logic, taking the exemption list as a parameter so tests can
/// exercise the filtering itself against a synthetic list instead of depending on
/// whatever `EXEMPT` currently holds.
fn check_source_with_exemptions(
    file: &str,
    source: &str,
    exempt: &[(&str, usize, &str)],
) -> Result<Vec<Violation>, String> {
    let tokens = TokenStream::from_str(source)
        .map_err(|e| format!("{file}: could not tokenize as Rust source: {e}"))?;
    let mut violations = Vec::new();
    walk(&tokens, file, &mut violations);
    violations.retain(|v| {
        !exempt
            .iter()
            .any(|(f, line, _)| *f == v.file && *line == v.line)
    });
    Ok(violations)
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
        let violations = check_source("test.rs", src).expect("tokenizes");
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
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 1);
        assert!(violations[0].excerpt.contains("first part"));
    }

    #[test]
    fn a_comment_containing_a_space_run_is_never_flagged() {
        let src = "// aligned    comment    columns\nconst M: &str = \"fine\";";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations, vec![], "comments are not string literals");
    }

    #[test]
    fn a_byte_string_is_checked_the_same_way() {
        let src = "const M: &[u8] = b\"first part                second part\";";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_raw_string_is_checked_but_cannot_hide_a_mangled_continuation() {
        // Raw strings have no escapes at all, so this bug class cannot occur in one —
        // but the check must still read past the r#"..."# delimiters correctly rather
        // than tripping over them, which this pins.
        let src = "const M: &str = r#\"first part                second part\"#;";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_single_space_between_words_is_fine() {
        let src = "const M: &str = \"perfectly ordinary prose with single spaces\";";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn a_two_space_run_is_not_flagged() {
        // Two spaces is a common deliberate stylistic choice (e.g. after a period) —
        // this check's threshold is three or more, matching what a mangled multi-level
        // indent actually produces, not ordinary typography.
        let src = "const M: &str = \"one.  Two spaces after a period is not the bug.\";";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn leading_spaces_at_the_very_start_of_a_literal_are_not_flagged() {
        // A literal that deliberately starts with several spaces (reproducing indented
        // external output, say) has nothing but the opening quote before the run — not
        // a word character — so it must not trip "flanked by non-space characters".
        let src = "const M: &str = \"     Running unittests\";";
        let violations = check_source("test.rs", src).expect("tokenizes");
        assert_eq!(violations, vec![]);
    }

    #[test]
    fn an_exempted_file_and_line_is_skipped_but_others_still_flag() {
        let src = "const A: &str = \"first bad                run\";\nconst B: &str = \"second bad                run\";\n";
        let all = check_source_with_exemptions("test.rs", src, &[]).expect("tokenizes");
        assert_eq!(all.len(), 2, "both lines are genuinely mangled unexempted");

        let exempt = &[("test.rs", 1usize, "synthetic exemption for this test only")];
        let filtered = check_source_with_exemptions("test.rs", src, exempt).expect("tokenizes");
        assert_eq!(
            filtered.len(),
            1,
            "line 1 is exempted; line 2 must still be reported"
        );
        assert_eq!(filtered[0].line, 2);
    }
}
