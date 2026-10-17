//! Static checks over `deploy/`, `deploy/Containerfile` and the source of the crate that serves the browser.
//!
//! Until S1 (2026-10-16) most of this file held the api/worker split in place: which compose service
//! linked the kernel, which role each set, which build target each named. One process now runs both,
//! so those rules went with the split. What is left guards two things that are still true:
//!
//! - **A model file is written once and moved, never deleted** (A2, owner 2026-10-17). In
//!   `lapidary-api`, `InboxWriter` (which writes an upload) is named only in the upload route,
//!   `move_to_removed` (which moves a removed model's file into `_removed/`) only in `retire.rs`,
//!   and `remove_file` / `remove_dir` only in the upload route, for its own staging files. Textual,
//!   so a lint against the mistake rather than a seal.
//! - **Named volumes are directories the image creates for `lapidary`**, or a fresh volume comes up
//!   owned by root and the server cannot write to it.
//!
//! No YAML parser: `serde_yaml` is unmaintained, and pulling a parser to read service blocks from a
//! file we control is not the boring option. A line-wise parser that cannot find what it expects
//! fails loudly with a parse-stale [`Violation`] rather than passing having verified nothing.

/// The `build: dockerfile:` value that identifies a compose service as one that runs
/// `lapidary-server`, as opposed to `db`, which runs the stock Postgres image as its own user.
const LAPIDARY_SERVER_DOCKERFILE: &str = "deploy/Containerfile";

/// The writing handles `lapidary-api` may name, each with the one file it may be named in — a
/// path and not a file name, because the rule is about this exact file and not about whatever
/// anyone calls their next module.
///
/// One entry since M4. `InboxWriter` writes an upload into the inbox, once, and `upload.rs` is
/// the route a user hands bytes to. The three handles this table used to hold went with the store
/// Lapidary filed sources into: the download route reads through `LibraryReader`, which writes
/// nothing and needs no exemption, and nothing renames a model's directory any more.
///
/// A2 added the mover and the two unlinks: a removed model's file is moved into `_removed/` by
/// `retire.rs` and by nothing else, and the only file this crate ever deletes is an upload's own
/// staging copy, in `upload.rs`.
const SOURCE_HANDLE_EXEMPTIONS: &[(&str, &str)] = &[
    ("InboxWriter", "crates/lapidary-api/src/upload.rs"),
    ("move_to_removed", "crates/lapidary-api/src/retire.rs"),
    ("remove_file", "crates/lapidary-api/src/upload.rs"),
    ("remove_dir", "crates/lapidary-api/src/upload.rs"),
];

#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    /// Parse-stale: a compose file has no top-level `services:` key this parser recognizes.
    ComposeMissingServicesKey,
    /// Parse-stale: a `services:` key was found but no service blocks parsed under it.
    ComposeNoServicesParsed,
    /// A service that runs `lapidary-server` mounts a named volume at a path `deploy/Containerfile` does not create
    /// owned by `lapidary`, so the volume comes up owned by root and the service cannot write to it.
    NamedVolumeNotOwned { service: String, target: String },
    /// A file under `crates/lapidary-api/src/` names one of the narrow source-bytes
    /// handles somewhere other than the single route that handle exists for.
    OpenPathNamesSourceHandleOutsideItsModule {
        path: String,
        handle: &'static str,
        allowed: &'static str,
    },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::ComposeMissingServicesKey => write!(
                f,
                "deploy/compose.yaml has no top-level `services:` key this parser \
                 recognizes. This check's parsing is stale, not the config — \
                 deploy/compose.yaml's shape has changed; update parse_services in \
                 xtask/src/deploy.rs to find it, rather than trusting a check that found \
                 nothing to verify."
            ),
            Violation::ComposeNoServicesParsed => write!(
                f,
                "deploy/compose.yaml has a `services:` key but this parser found no service \
                 blocks under it. This check's parsing is stale, not the config — update \
                 parse_services in xtask/src/deploy.rs (it expects two-space-indented \
                 `name:` lines) to match the file's actual shape."
            ),
            Violation::NamedVolumeNotOwned { service, target } => write!(
                f,
                "service '{service}' mounts a named volume at {target}, but deploy/Containerfile does not create \
                 that directory owned by `lapidary`. Docker and Podman seed a new named volume from the image's \
                 directory at the mount point, ownership included; with none there it comes up owned by root, \
                 and the service, which runs as `lapidary`, cannot write to it. Add {target} to the runtime \
                 stage's `install -d -o lapidary -g lapidary` line."
            ),
            Violation::OpenPathNamesSourceHandleOutsideItsModule {
                path,
                handle,
                allowed,
            } => write!(
                f,
                "{path} names {handle}. That handle writes a source file and exists for exactly \
                 one file — {allowed}, the one route a user hands bytes to. A second file in \
                 lapidary-api writing source files is how the rule that Lapidary never writes \
                 into the owner's folders gets lost. Move the work into {allowed} — that path \
                 exactly, so splitting the route into a directory or naming a second module \
                 after it does not widen the exemption — or read through LibraryReader."
            ),
        }
    }
}

/// One compose service block, as far as these checks need it.
struct ServiceBlock {
    name: String,
    /// The `dockerfile:` value under `build:`, if any.
    dockerfile: Option<String>,
    /// The container paths of the named volumes the service mounts, from `volumes:`' short syntax
    /// (`name:/path[:options]`). A source with a `/`, a `.`, a `~` or a `$` is a host path, and is not
    /// the image's to create.
    named_volumes: Vec<String>,
    /// Every mount's container path and options, from the same short syntax, bind mounts
    /// included: `(target, options)`.
    mounts: Vec<(String, String)>,
}

/// Strip one layer of matching YAML quotes from a scalar, plus surrounding whitespace.
/// `dockerfile: "deploy/Containerfile"` is the same value as the unquoted form to every
/// Compose implementation, and it must be the same value here — quoting both `dockerfile:`
/// values and deleting `LAPIDARY_ROLE` used to report OK, because the exact-string match
/// failed and `runs_lapidary_server` silently became false. That reproduces the very
/// silent-failure shape these rules exist to close, moved into the parser.
fn scalar(value: &str) -> String {
    let trimmed = value.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = trimmed.strip_prefix(quote)
            && let Some(inner) = inner.strip_suffix(quote)
        {
            return inner.to_owned();
        }
    }
    trimmed.to_owned()
}

/// Parse a compose file's `services:` block into one [`ServiceBlock`] per service.
///
/// Line-wise, not a YAML parser: a service name is a line indented by exactly two spaces
/// ending in `:` (`  app:`); everything more deeply indented until the next such line —
/// or the next unindented top-level key — belongs to that service's body.
fn parse_services(contents: &str) -> Result<Vec<ServiceBlock>, Violation> {
    let lines: Vec<&str> = contents.lines().collect();
    let Some(start) = lines.iter().position(|l| *l == "services:") else {
        return Err(Violation::ComposeMissingServicesKey);
    };

    let mut services: Vec<ServiceBlock> = Vec::new();
    let mut current: Option<ServiceBlock> = None;
    let mut in_volumes = false;

    for line in &lines[start + 1..] {
        // A non-blank, unindented line ends the services: block (e.g. a top-level
        // `volumes:` key) — unless it's a comment. A column-0 comment (a section banner)
        // is ordinary YAML style *inside* a mapping and must not be mistaken for the next
        // top-level key, or every service below the banner becomes invisible.
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            if line.trim_start().starts_with('#') {
                continue;
            }
            break;
        }

        if let Some(rest) = line.strip_prefix("  ")
            && !rest.starts_with(' ')
            && !rest.starts_with('\t')
            && !rest.is_empty()
        {
            let trimmed = rest.trim_end();
            if let Some(name) = trimmed.strip_suffix(':')
                && !name.is_empty()
                && !name.starts_with('#')
            {
                if let Some(finished) = current.take() {
                    services.push(finished);
                }
                current = Some(ServiceBlock {
                    name: name.to_owned(),
                    dockerfile: None,
                    named_volumes: Vec::new(),
                    mounts: Vec::new(),
                });
                in_volumes = false;
                continue;
            }
        }

        if let Some(block) = current.as_mut() {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if indent == 4 && !trimmed.starts_with('#') && !trimmed.is_empty() {
                in_volumes = trimmed.starts_with("volumes:");
            }
            if in_volumes
                && indent == 6
                && let Some(mount) = trimmed.strip_prefix('-').map(scalar)
                && let Some((source, rest)) = split_source(&mount)
                && !source.is_empty()
            {
                let (target, options) = rest.split_once(':').unwrap_or((rest, ""));
                block.mounts.push((target.to_owned(), options.to_owned()));
                if !source.contains(['/', '.', '~', '$']) {
                    block.named_volumes.push(target.to_owned());
                }
            }
            if !trimmed.starts_with('#')
                && let Some(value) = trimmed.strip_prefix("dockerfile:")
            {
                block.dockerfile = Some(scalar(value));
            }
        }
    }
    if let Some(finished) = current.take() {
        services.push(finished);
    }

    if services.is_empty() {
        return Err(Violation::ComposeNoServicesParsed);
    }
    Ok(services)
}

/// A short-syntax mount split into its source and the rest, where the source may be a compose
/// substitution with its own `:-` fallback (`${LAPIDARY_INGEST_DIR:-../example/parts}:/ingest:ro`):
/// the first `:` outside `${…}` is the separator.
fn split_source(mount: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (at, c) in mount.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some((&mount[..at], &mount[at + 1..])),
            _ => {}
        }
    }
    None
}

/// Join backslash line-continuations into logical lines, so a `RUN` instruction wrapped
/// across multiple physical lines (`RUN cargo build ... \` followed by an indented
/// continuation) is inspected as one line rather than several. Docker's own parser joins
/// continuations the same way before evaluating an instruction; without this, a wrapped
/// `cargo build` line with the expansion on the continuation would make
/// `BuildLineMissingArgExpansion` fire on a perfectly correct file, because the substring
/// search would only ever see the first physical line.
///
/// Comments get deliberate, separate handling, because Docker treats them specially with
/// respect to continuation and a uniform join breaks the comment exclusion the search
/// functions below rely on:
///
/// - **A comment line never continues, even if it ends in `\`.** A trailing backslash in
///   a comment is just text. Joining it into the next line anyway would let a `\`-
///   terminated comment directly above `RUN cargo build` or `ARG SERVER_FEATURES=` absorb
///   that instruction into one `#`-prefixed logical line — which the comment-excluding
///   searches below then skip entirely, misreporting a parse-stale violation ("no ARG /
///   no cargo build line") on a file that is completely valid.
/// - **A comment line inside an already-open continuation is skipped, not spliced in and
///   not treated as ending the continuation.** This matches BuildKit's own behavior — `RUN
///   foo && \` / `# explains the next line` / `    bar` is one `RUN` command with the
///   comment ignored, not two instructions and not a comment containing `bar`. Ending the
///   continuation at the comment instead would truncate a real multi-line instruction
///   right where someone added an explanation, which is the opposite of what a comment is
///   for.
fn logical_lines(contents: &str) -> Vec<String> {
    let mut logical = Vec::new();
    let mut buffer = String::new();
    let mut continuing = false;

    for raw in contents.lines() {
        let is_comment = raw.trim_start().starts_with('#');

        if continuing {
            if is_comment {
                // Skip: a comment inside an open continuation is neither part of the
                // instruction text nor the end of it — see the module doc above.
                continue;
            }
            buffer.push(' ');
            buffer.push_str(raw.trim_start());
        } else if is_comment {
            // Comments never continue, regardless of a trailing backslash — always their
            // own logical line.
            logical.push(raw.to_owned());
            continue;
        } else {
            buffer.push_str(raw);
        }

        if let Some(stripped) = buffer.trim_end().strip_suffix('\\') {
            buffer = stripped.trim_end().to_owned();
            continuing = true;
        } else {
            continuing = false;
            logical.push(std::mem::take(&mut buffer));
        }
    }
    // A trailing continuation with nothing following it: keep whatever was gathered
    // rather than silently dropping the tail of the file.
    if continuing {
        logical.push(buffer);
    }
    logical
}

/// `lapidary-api` may name each handle in [`SOURCE_HANDLE_EXEMPTIONS`] only in the one file that
/// table pairs it with. `main.rs` walks `crates/lapidary-api/src/**/*.rs` and passes
/// `(path, contents)` pairs here.
///
/// A lint against accidental misuse, not a security boundary: it scans literal source text, so a
/// re-export under another name slips past. A green run proves only that nothing under
/// `crates/lapidary-api/src/` but `upload.rs` names `InboxWriter` directly.
pub fn check_open_path_boundary(api_sources: &[(String, String)]) -> Vec<Violation> {
    // Per file, then per handle, so one file naming two handles reports in the order a
    // reader meets them rather than grouped by handle.
    api_sources
        .iter()
        .flat_map(|(path, body)| {
            SOURCE_HANDLE_EXEMPTIONS
                .iter()
                .filter(move |(handle, allowed)| body.contains(handle) && !is_module(path, allowed))
                .map(move |(handle, allowed)| {
                    Violation::OpenPathNamesSourceHandleOutsideItsModule {
                        path: path.clone(),
                        handle,
                        allowed,
                    }
                })
        })
        .collect()
}

/// Is this the single file a handle is allowed in? `Path::ends_with` is
/// *component-wise*, not a string suffix, which is the whole reason it is the right
/// primitive here: it rejects `bulk_download.rs` (a different final component, not a
/// suffix of one) and equally rejects `handlers/download.rs` and `download/mod.rs` (right
/// name, wrong parents). Comparing `file_name()` instead would exempt every `download.rs`
/// at every depth, which is precisely the copy-paste spread rule 5 exists to stop.
/// `main.rs` hands us absolute paths; a component-wise suffix match is indifferent to the
/// prefix.
fn is_module(path: &str, module: &str) -> bool {
    std::path::Path::new(path).ends_with(module)
}

/// Every named volume a `lapidary-server` service mounts, in any of `compose_files`, is a directory the Containerfile
/// creates owned by `lapidary` (`install -d -o lapidary …`): the image is what gives a fresh named volume its owner. Bind
/// mounts are the operator's, and a service that does not build the Containerfile (`db`) runs as its own image's user.
///
/// Textual, as the rules above are: it reads `install` lines that name `-o lapidary`, in any stage.
pub fn check_volume_ownership(compose_files: &[&str], containerfile: &str) -> Vec<Violation> {
    let owned: Vec<String> = logical_lines(containerfile)
        .iter()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split("&&").map(str::to_owned).collect::<Vec<_>>())
        .filter(|command| command.contains("install") && command.contains("-o lapidary"))
        .flat_map(|command| {
            command
                .split_whitespace()
                .filter(|word| word.starts_with('/'))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut violations = Vec::new();
    let mut services = Vec::new();
    for contents in compose_files {
        // A file this parser cannot read is reported, not skipped: a check that found nothing to
        // verify must not look like one that verified everything.
        match parse_services(contents) {
            Ok(parsed) => services.extend(parsed),
            Err(stale) => violations.push(stale),
        }
    }
    let unowned = services
        .iter()
        .filter(|service| service.dockerfile.as_deref() == Some(LAPIDARY_SERVER_DOCKERFILE))
        .flat_map(|service| {
            let owned = &owned;
            service
                .named_volumes
                .iter()
                .filter(move |target| !owned.contains(target))
                .map(move |target| Violation::NamedVolumeNotOwned {
                    service: service.name.clone(),
                    target: target.clone(),
                })
                .collect::<Vec<_>>()
        });
    violations.extend(unowned);
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writing handle is allowed in the upload route and nowhere else: not another file, not a
    /// file whose name ends the same way, not the same name at another depth.
    #[test]
    fn the_inbox_writer_is_allowed_in_upload_rs_and_nowhere_else() {
        let sources = vec![
            (
                "crates/lapidary-api/src/upload.rs".to_owned(),
                "use lapidary_storage::InboxWriter;\n".to_owned(),
            ),
            // main.rs walks from an absolute workspace root, so the exempt path arrives with a
            // prefix.
            (
                "/home/runner/work/lapidary/lapidary/crates/lapidary-api/src/upload.rs".to_owned(),
                "use lapidary_storage::InboxWriter;\n".to_owned(),
            ),
            (
                "crates/lapidary-api/src/moves.rs".to_owned(),
                "fn place(w: &InboxWriter) {}\n".to_owned(),
            ),
            (
                "crates/lapidary-api/src/bulk_upload.rs".to_owned(),
                "use lapidary_storage::InboxWriter;\n".to_owned(),
            ),
            (
                "crates/lapidary-api/src/upload/mod.rs".to_owned(),
                "use lapidary_storage::InboxWriter;\n".to_owned(),
            ),
            // Reading needs no exemption: `LibraryReader` cannot write.
            (
                "crates/lapidary-api/src/download.rs".to_owned(),
                "use lapidary_storage::{DerivativeStore, LibraryReader, Roots};\n".to_owned(),
            ),
        ];
        let violation = |path: &str| Violation::OpenPathNamesSourceHandleOutsideItsModule {
            path: path.to_owned(),
            handle: "InboxWriter",
            allowed: "crates/lapidary-api/src/upload.rs",
        };
        let violations = check_open_path_boundary(&sources);
        assert_eq!(
            violations,
            vec![
                violation("crates/lapidary-api/src/moves.rs"),
                violation("crates/lapidary-api/src/bulk_upload.rs"),
                violation("crates/lapidary-api/src/upload/mod.rs"),
            ]
        );
        assert!(
            violations[2]
                .to_string()
                .contains("crates/lapidary-api/src/upload.rs")
        );
    }

    /// Docker and Podman seed a new named volume from the image's directory at its mount point, ownership included. Two
    /// volumes once had no such directory, so they came up owned by root and the server, running as `lapidary`, could
    /// not write to them: found by goal 8 running the images, not by any check.
    #[test]
    fn a_named_volume_the_image_does_not_create_for_lapidary_is_refused() {
        let compose = "\
services:
  app:
    build:
      context: ..
      dockerfile: deploy/Containerfile
    volumes:
      - ${LAPIDARY_STORAGE_ROOT:-../storage}:/var/lib/lapidary:z
      - ../example/parts:/ingest:ro,z
      - \"lapidary-uploads:/var/lib/lapidary-uploads:Z\"
      - lapidary-staging:/var/lib/lapidary-staging:Z
    ports:
      - \"3000:8080\"
  db:
    image: docker.io/library/postgres:18
    volumes:
      - lapidary-db:/var/lib/postgresql:Z
";
        let containerfile = "\
FROM debian AS runtime
RUN install -d -o lapidary -g lapidary -m 0755 /var/lib/lapidary \\
    && install -d -o lapidary -g lapidary -m 0755 /var/lib/lapidary-uploads
RUN install -d -m 0755 /var/lib/lapidary-staging
";
        assert_eq!(
            check_volume_ownership(&[compose], containerfile),
            vec![Violation::NamedVolumeNotOwned {
                service: "app".to_owned(),
                target: "/var/lib/lapidary-staging".to_owned(),
            }],
            "bind mounts and the db's own volume are not the image's to create, a port is not a volume, and a directory \
             the image makes for root is not one it makes for lapidary"
        );
    }

    #[test]
    fn every_named_volume_in_the_real_deploy_files_is_created_for_lapidary() {
        assert_eq!(
            check_volume_ownership(
                &[include_str!("../../deploy/compose.yaml")],
                include_str!("../../deploy/Containerfile"),
            ),
            vec![]
        );
    }

    #[test]
    fn a_compose_file_this_parser_cannot_read_is_reported_not_passed() {
        let containerfile = "FROM debian AS runtime\n";
        assert_eq!(
            check_volume_ownership(
                &["name: lapidary\n\nvolumes:\n  lapidary-db:\n"],
                containerfile
            ),
            vec![Violation::ComposeMissingServicesKey]
        );
        let odd = "services:\n# nothing recognizable here\nvolumes:\n  lapidary-db:\n";
        assert_eq!(
            check_volume_ownership(&[odd], containerfile),
            vec![Violation::ComposeNoServicesParsed]
        );
        let message = Violation::ComposeMissingServicesKey.to_string();
        assert!(
            message.contains("This check's parsing is stale, not the config"),
            "{message}"
        );
    }

    #[test]
    fn a_column_zero_comment_banner_does_not_hide_the_services_below_it() {
        let compose = "\
services:
# ---- the application ----
  app:
    build:
      dockerfile: deploy/Containerfile
    volumes:
      - lapidary-uploads:/var/lib/lapidary-uploads:Z
";
        assert_eq!(
            check_volume_ownership(&[compose], "FROM debian AS runtime\n"),
            vec![Violation::NamedVolumeNotOwned {
                service: "app".to_owned(),
                target: "/var/lib/lapidary-uploads".to_owned(),
            }]
        );
    }
}
