//! Static checks over `deploy/compose.yaml` and `deploy/Containerfile`.
//!
//! Protects the same product rule `layers.rs`'s `FORBIDDEN_PAIRS` protects, from the
//! deploy side rather than the dependency-graph side: the open path (opening a part for
//! viewing) lives in `lapidary-api` and must never invoke the CAD kernel. `layers.rs`
//! stops `lapidary-api` from depending on `lapidary-cad` at compile time; this module
//! stops the `api` container image from *linking* the kernel at build time, by checking
//! that `SERVER_FEATURES` — the build arg that pulls the kernel in — is set only for
//! kernel-linked services (today just `worker`), and that `deploy/Containerfile` still
//! routes that arg into the build rather than hardcoding a feature flag.
//!
//! This check is **static**: it verifies the text of the two files, not the images built
//! from them. A green `check-deploy` means the configuration is internally consistent, not
//! that the last-built `api` image actually lacks the kernel — that would require
//! inspecting a built artifact, which this does not do.
//!
//! No YAML parser: `serde_yaml` is unmaintained, and pulling a parser to read two service
//! blocks from a file we control is not the boring option. Parsing is line-wise instead —
//! see the module doc on the failure mode below for why that is safe.
//!
//! Pure functions over `&str` file contents, unit-testable without touching the
//! filesystem — `main.rs` reads the files and calls these, mirroring the `layers.rs` /
//! `main.rs` split.
//!
//! ## The failure mode
//!
//! A line-wise parser that cannot find what it expects must fail loudly, not pass
//! silently. If `deploy/compose.yaml` has no `services:` key, or no service blocks parse
//! under it; if `deploy/Containerfile` has no `cargo build` line, or no `ARG
//! SERVER_FEATURES` declaration at all — each is its own [`Violation`], distinct from an
//! ordinary rule violation, whose message says the check's *parsing* is stale, not the
//! configuration, and names the function to update. Collapsing "found nothing to check"
//! into "found no violations" is exactly the bug this module exists to avoid: an earlier
//! round shipped an assertion whose two halves could both degrade to an empty string,
//! compare equal, and pass having verified nothing (see the rustc-version check in
//! `deploy/Containerfile` for the fix pattern this module follows).

/// Services allowed to set `SERVER_FEATURES` in `deploy/compose.yaml` — i.e. services that
/// legitimately link the CAD kernel. In the same spirit as `FORBIDDEN_PAIRS` in
/// `layers.rs`: a named list edited only to *permit* a new kernel-linked service, never to
/// make an ordinary edit pass. Checked generically over every service block rather than
/// naming `api`, so a future `api-replica` is covered without anyone remembering to add a
/// check for it.
const KERNEL_LINKED_SERVICES: &[&str] = &["worker"];

/// The `build: dockerfile:` value that identifies a `deploy/compose.yaml` service as one
/// that builds and runs `lapidary-server` — as opposed to `db` (`db/Containerfile`, runs
/// Postgres) or `web` (`deploy/web/Containerfile`, a different binary entirely). Matching
/// on the dockerfile path rather than naming `api`/`worker` in a list means a future
/// `lapidary-server`-based service is covered by the `LAPIDARY_ROLE` rule below without
/// anyone remembering to add its name anywhere.
const LAPIDARY_SERVER_DOCKERFILE: &str = "deploy/Containerfile";

/// The exact expansion `deploy/Containerfile`'s `cargo build` line must contain so the
/// `SERVER_FEATURES` arg actually reaches the build. Kept as one constant so the check and
/// its own doc comment can't drift apart.
const ARG_EXPANSION: &str = "${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}";

#[derive(Debug, PartialEq, Eq)]
pub enum Violation {
    /// A `deploy/compose.yaml` service sets `SERVER_FEATURES` but is not in
    /// `KERNEL_LINKED_SERVICES`.
    UnexpectedKernelLink { service: String },
    /// A service in `KERNEL_LINKED_SERVICES` does not set `SERVER_FEATURES`.
    MissingKernelLink { service: String },
    /// A `deploy/compose.yaml` service builds `LAPIDARY_SERVER_DOCKERFILE` (i.e. runs
    /// `lapidary-server`) but does not set `LAPIDARY_ROLE`.
    MissingRole { service: String },
    /// `deploy/Containerfile`'s `cargo build` line does not contain the
    /// `${SERVER_FEATURES:+...}` expansion.
    BuildLineMissingArgExpansion,
    /// `deploy/Containerfile`'s `cargo build` line contains a second, hardcoded
    /// `--features` flag alongside the expansion.
    BuildLineHardcodedFeatures,
    /// `ARG SERVER_FEATURES` is declared somewhere the `cargo build` line that uses it
    /// cannot see it — after that line, or in a different build stage (a `FROM` sits
    /// between the `ARG` and the `RUN cargo build` line, and `ARG` scope does not cross a
    /// `FROM`).
    ArgNotVisibleToBuildLine,
    /// Parse-stale: `deploy/compose.yaml` has no top-level `services:` key this parser
    /// recognizes.
    ComposeMissingServicesKey,
    /// Parse-stale: a `services:` key was found but no service blocks parsed under it.
    ComposeNoServicesParsed,
    /// Parse-stale: no `cargo build` line found in `deploy/Containerfile`.
    ContainerfileMissingBuildLine,
    /// Parse-stale: no `ARG SERVER_FEATURES` declaration at all in
    /// `deploy/Containerfile`.
    ContainerfileMissingArgDeclaration,
    /// A file under `crates/lapidary-api/src/`, other than one listed in
    /// `OPEN_PATH_BOUNDARY_EXEMPTIONS`, names `SourceStore`. The open path must never
    /// touch a source file — only derivatives.
    OpenPathNamesSourceStore { path: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::UnexpectedKernelLink { service } => write!(
                f,
                "deploy/compose.yaml service '{service}' sets SERVER_FEATURES but is not in \
                 KERNEL_LINKED_SERVICES (xtask/src/deploy.rs). The open path must never \
                 invoke the CAD kernel — if {service} genuinely needs it, add it to \
                 KERNEL_LINKED_SERVICES; otherwise remove the SERVER_FEATURES build arg from \
                 {service} in deploy/compose.yaml."
            ),
            Violation::MissingKernelLink { service } => write!(
                f,
                "deploy/compose.yaml service '{service}' is listed in KERNEL_LINKED_SERVICES \
                 (xtask/src/deploy.rs) but does not set SERVER_FEATURES — or is absent from \
                 the file entirely — so its image would build without the CAD kernel it \
                 needs. Add `args: SERVER_FEATURES: \
                 mock-kernel` (or the current feature name) under {service} in \
                 deploy/compose.yaml, or remove {service} from KERNEL_LINKED_SERVICES if it \
                 no longer needs the kernel."
            ),
            Violation::MissingRole { service } => write!(
                f,
                "deploy/compose.yaml service '{service}' builds {LAPIDARY_SERVER_DOCKERFILE} \
                 (runs lapidary-server) but does not set LAPIDARY_ROLE. \
                 bin/lapidary-server/src/main.rs requires it — there is no default, because \
                 the two roles are not interchangeable and a missing value on the worker \
                 service fails silently (the container starts, binds, and passes its \
                 healthcheck, but never mounts /scan). Add `LAPIDARY_ROLE: api` (serves the \
                 grid and the open path) or `LAPIDARY_ROLE: worker` (runs ingest) under \
                 {service}'s environment: block in deploy/compose.yaml."
            ),
            Violation::BuildLineMissingArgExpansion => write!(
                f,
                "deploy/Containerfile's `cargo build` line does not contain the \
                 {ARG_EXPANSION} expansion, so the SERVER_FEATURES arg never reaches the \
                 build — every image would build with the same fixed feature set regardless \
                 of what deploy/compose.yaml sets per service. Restore the expansion in the \
                 RUN cargo build line."
            ),
            Violation::BuildLineHardcodedFeatures => write!(
                f,
                "deploy/Containerfile's `cargo build` line contains a second, hardcoded \
                 --features flag alongside the {ARG_EXPANSION} expansion. A hardcoded flag \
                 applies to every image regardless of SERVER_FEATURES, which could link the \
                 CAD kernel into the api image — the open path must never invoke the kernel. \
                 Remove the hardcoded --features flag and let SERVER_FEATURES alone decide \
                 which image gets the kernel."
            ),
            Violation::ArgNotVisibleToBuildLine => write!(
                f,
                "deploy/Containerfile declares ARG SERVER_FEATURES somewhere the `cargo \
                 build` line that uses it cannot see it — either it comes after that line, \
                 or a FROM starting a later build stage sits between the ARG and the RUN \
                 cargo build line, and ARG scope does not cross a FROM. So the \
                 {ARG_EXPANSION} expansion would silently see an empty value and the worker \
                 image would ship without the CAD kernel it needs. Move ARG SERVER_FEATURES \
                 to a line after the FROM that starts the stage containing `RUN cargo \
                 build`, and before that RUN line, with no other FROM between them."
            ),
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
            Violation::ContainerfileMissingBuildLine => write!(
                f,
                "deploy/Containerfile has no line containing `cargo build`. This check's \
                 parsing is stale, not the config — update check_containerfile in \
                 xtask/src/deploy.rs to find wherever the build command now lives."
            ),
            Violation::ContainerfileMissingArgDeclaration => write!(
                f,
                "deploy/Containerfile has no `ARG SERVER_FEATURES` declaration this parser \
                 recognizes. This check's parsing is stale, not the config — update \
                 check_containerfile in xtask/src/deploy.rs to find it, whatever form it now \
                 takes."
            ),
            Violation::OpenPathNamesSourceStore { path } => write!(
                f,
                "{path} names SourceStore. lapidary-api serves the open path, which must \
                 never touch a source file — it reads metadata and derivatives only. Use \
                 DerivativeStore, or move the work into crates/lapidary-api/src/scan.rs \
                 (the one handler file OPEN_PATH_BOUNDARY_EXEMPTIONS allows, because \
                 router() mounts it only under Role::Worker)."
            ),
        }
    }
}

/// One `deploy/compose.yaml` service block, as far as these checks need it.
struct ServiceBlock {
    name: String,
    /// Does the service set `SERVER_FEATURES` (any nesting depth)?
    sets_features: bool,
    /// The `dockerfile:` value under `build:`, if any — used to recognize a service that
    /// builds `deploy/Containerfile` and therefore runs `lapidary-server`, without having
    /// to name every such service (`api`, `worker`, and any future one) in a list.
    dockerfile: Option<String>,
    /// Does the service set `LAPIDARY_ROLE` (any nesting depth)?
    sets_role: bool,
}

/// Parse `deploy/compose.yaml`'s `services:` block into one [`ServiceBlock`] per service.
///
/// Line-wise, not a YAML parser: a service name is a line indented by exactly two spaces
/// ending in `:` (`  worker:`); everything more deeply indented until the next such line —
/// or the next unindented top-level key — belongs to that service's body. Within a body,
/// any non-comment line starting with `SERVER_FEATURES:` or `LAPIDARY_ROLE:` (after its own
/// leading whitespace) marks that service as setting it, regardless of nesting depth (each
/// normally lives under a specific key — `build: args:`, `environment:` — but this does not
/// require that exact path — a boring, tolerant check); a `dockerfile:` line records its
/// value.
fn parse_services(contents: &str) -> Result<Vec<ServiceBlock>, Violation> {
    let lines: Vec<&str> = contents.lines().collect();
    let Some(start) = lines.iter().position(|l| *l == "services:") else {
        return Err(Violation::ComposeMissingServicesKey);
    };

    let mut services: Vec<ServiceBlock> = Vec::new();
    let mut current: Option<ServiceBlock> = None;

    for line in &lines[start + 1..] {
        // A non-blank, unindented line ends the services: block (e.g. a top-level
        // `volumes:` key) — unless it's a comment. A column-0 comment (a section banner
        // like `# ---- application services ----`) is ordinary YAML style *inside* a
        // mapping and must not be mistaken for the next top-level key, or every service
        // below the banner — kernel-linked or not — becomes invisible to this parser
        // without raising ComposeNoServicesParsed, because the services found above the
        // banner are still enough to make the file parse as "some services found".
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            if line.trim_start().starts_with('#') {
                continue;
            }
            break;
        }

        // A two-space-indented `name:` line (no deeper indent, no trailing content after
        // the colon) starts a new service.
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
                    sets_features: false,
                    dockerfile: None,
                    sets_role: false,
                });
                continue;
            }
        }

        if let Some(block) = current.as_mut() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                // Mapping form (`SERVER_FEATURES: mock-kernel`, the form deploy/compose.yaml
                // actually uses) and list form (`- SERVER_FEATURES=mock-kernel`, equally
                // valid Compose syntax for `args:`) both count. Recognizing only the mapping
                // form would make a list-form worker report MissingKernelLink — telling the
                // reader their image ships kernel-less when it would not.
                let list_item = |key: &str| {
                    trimmed
                        .strip_prefix('-')
                        .map(str::trim_start)
                        .is_some_and(|rest| rest.starts_with(key))
                };
                if trimmed.starts_with("SERVER_FEATURES:") || list_item("SERVER_FEATURES=") {
                    block.sets_features = true;
                }
                if trimmed.starts_with("LAPIDARY_ROLE:") || list_item("LAPIDARY_ROLE=") {
                    block.sets_role = true;
                }
                if let Some(value) = trimmed.strip_prefix("dockerfile:") {
                    block.dockerfile = Some(value.trim().to_owned());
                }
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

/// Rule 1: exactly the services in `KERNEL_LINKED_SERVICES` set `SERVER_FEATURES` in
/// `deploy/compose.yaml` — no more, no fewer.
///
/// Rule 5: every service that builds `LAPIDARY_SERVER_DOCKERFILE` (i.e. runs
/// `lapidary-server`) sets `LAPIDARY_ROLE` explicitly. `bin/lapidary-server/src/main.rs`
/// deliberately has no default for it — the two roles are not interchangeable, and a
/// missing `api` value looks fine (the process just stays `api`) while a missing `worker`
/// value fails silently (the container starts, binds, passes its healthcheck, and never
/// mounts `/scan`). This rule is the CI-time half of closing that hole: it catches
/// `deploy/compose.yaml` losing the variable before anyone runs the container.
pub fn check_compose(contents: &str) -> Vec<Violation> {
    let services = match parse_services(contents) {
        Ok(services) => services,
        Err(parse_violation) => return vec![parse_violation],
    };

    let mut violations = Vec::new();
    for service in &services {
        let is_kernel_linked = KERNEL_LINKED_SERVICES.contains(&service.name.as_str());
        match (service.sets_features, is_kernel_linked) {
            (true, false) => violations.push(Violation::UnexpectedKernelLink {
                service: service.name.clone(),
            }),
            (false, true) => violations.push(Violation::MissingKernelLink {
                service: service.name.clone(),
            }),
            _ => {}
        }

        let runs_lapidary_server =
            service.dockerfile.as_deref() == Some(LAPIDARY_SERVER_DOCKERFILE);
        if runs_lapidary_server && !service.sets_role {
            violations.push(Violation::MissingRole {
                service: service.name.clone(),
            });
        }
    }

    // The loop above only walks services the parser actually found, so a
    // KERNEL_LINKED_SERVICES entry that never parsed at all — the service block was
    // deleted, renamed, or hidden below a banner comment this parser used to choke on —
    // raises nothing there. Check the reverse direction too: every name this module
    // expects to exist must have been found among the parsed services.
    for &expected in KERNEL_LINKED_SERVICES {
        if !services.iter().any(|s| s.name == expected) {
            violations.push(Violation::MissingKernelLink {
                service: expected.to_owned(),
            });
        }
    }

    violations
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

/// Rules 2 and 3: `deploy/Containerfile` still routes `SERVER_FEATURES` through the arg
/// expansion in its `cargo build` line, and `ARG SERVER_FEATURES` is declared where that
/// line can actually see it.
///
/// "Visible" is not "after the first FROM" — a multi-stage Containerfile can declare the
/// arg in one stage and use `cargo build` in a later one, or (the bug this replaced) place
/// the arg after some *other* FROM while `cargo build` runs in an earlier stage. `ARG`
/// scope does not cross a `FROM`, so the real requirement is: the arg's declaration comes
/// before the `cargo build` line, and no `FROM` sits between them. That is checked against
/// every `FROM` in the file, not just the first, so it stays correct if a third stage is
/// ever added.
pub fn check_containerfile(contents: &str) -> Vec<Violation> {
    let lines = logical_lines(contents);
    let mut violations = Vec::new();

    // Case-insensitive: Dockerfile instructions are case-insensitive, and a lowercase
    // `from` between the ARG and the build line would hide a stage boundary from a
    // case-sensitive match — the exact silent-pass failure mode this module exists to
    // avoid, just less likely than the compose one. (`ARG`/`RUN` don't need the same
    // treatment: a lowercase `arg` or `run` just fails to match at all, which degrades
    // loudly to a parse-stale violation rather than passing silently.)
    let from_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let trimmed = l.trim_start();
            trimmed
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("from "))
        })
        .map(|(i, _)| i)
        .collect();

    let arg_decl = lines
        .iter()
        .position(|l| l.trim_start().starts_with("ARG SERVER_FEATURES"));
    if arg_decl.is_none() {
        violations.push(Violation::ContainerfileMissingArgDeclaration);
    }

    // Anchored on `RUN cargo build`, not the bare substring "cargo build": a comment
    // mentioning "cargo build" above the real RUN line must not be mistaken for it. Excludes
    // comment lines the same way parse_services does for deploy/compose.yaml. Uses the
    // first match: deploy/Containerfile has exactly one `cargo build` invocation today, so
    // there is nothing to disambiguate. A second RUN cargo build line appearing in the
    // future is a sign this check needs a design update, not a case whose first match is
    // authoritative — no such fixture exists here and none should be added to paper over
    // it.
    let build_line = lines.iter().enumerate().find(|(_, l)| {
        let trimmed = l.trim_start();
        !trimmed.starts_with('#') && trimmed.starts_with("RUN cargo build")
    });

    match build_line {
        None => violations.push(Violation::ContainerfileMissingBuildLine),
        Some((build_i, build_content)) => {
            if !build_content.contains(ARG_EXPANSION) {
                violations.push(Violation::BuildLineMissingArgExpansion);
            }
            if build_content.matches("--features").count() > 1 {
                violations.push(Violation::BuildLineHardcodedFeatures);
            }
            if let Some(arg_i) = arg_decl {
                let visible = arg_i < build_i
                    && !from_indices
                        .iter()
                        .any(|&from_i| from_i > arg_i && from_i < build_i);
                if !visible {
                    violations.push(Violation::ArgNotVisibleToBuildLine);
                }
            }
            // If arg_decl is None, ContainerfileMissingArgDeclaration was already pushed
            // above — nothing more useful to say about where it's visible from.
        }
    }

    violations
}

/// Files, matched by base name under `crates/lapidary-api/src/`, exempted from Rule 4
/// below. A named allow-list, in the same spirit as `KERNEL_LINKED_SERVICES` above:
/// edited only to permit a new worker-only handler file that legitimately needs
/// `SourceStore`, never to make an ordinary open-path change pass.
///
/// `scan.rs` is the one file today. It is the ingest handler `router()` (`lib.rs`) mounts
/// only under `Role::Worker` — see the module doc there — so naming `SourceStore` in it
/// does not put a source file within reach of the open path; it puts it within reach of
/// exactly the process role that is supposed to reach one.
const OPEN_PATH_BOUNDARY_EXEMPTIONS: &[&str] = &["scan.rs"];

/// Rule 4: no file in `lapidary-api` may name `SourceStore`, except the worker-only
/// handler files listed in `OPEN_PATH_BOUNDARY_EXEMPTIONS`. The type needs a `WorkerRole`
/// token to construct, so the compiler already prevents obtaining one from a file that
/// never proved it is running as the worker — this catches the earlier mistake of
/// importing it at all, which is the first move someone makes before discovering they
/// cannot build one, and the point at which to stop them, in every file except the one
/// that is allowed to make that move.
///
/// A dependency-graph rule cannot express this: `lapidary-api` legitimately depends on
/// `lapidary-storage` for `DerivativeStore` (both roles hold one), and, since Task 9,
/// legitimately contains one handler that legitimately holds a `SourceStore` too — so the
/// boundary is *which file*, not whether the crate may name the type at all. `main.rs`
/// walks `crates/lapidary-api/src/**/*.rs` and passes `(path, contents)` pairs here.
///
/// This is a lint against accidental misuse, not a security boundary: it scans literal
/// source text for the string `SourceStore`, so a third crate re-exporting it under another
/// name — `pub use lapidary_storage::SourceStore as BlobStore` from somewhere `lapidary-api`
/// then imports — would slip past, and the exemption matches on file *name*, not on proof
/// that the file's routes only ever mount under `Role::Worker` — a second file also named
/// `scan.rs` in a nested module would slip past too. That is an acceptable trade for a
/// cheap static check against the mistake this actually catches (importing the type
/// directly into an open-path file, the first thing someone does before discovering the
/// compiler won't let them build one there); a future reader should not treat a green run
/// here as proof no source bytes are reachable outside `scan.rs`, only as proof nothing
/// named `SourceStore` directly outside the files this list names.
pub fn check_open_path_boundary(api_sources: &[(String, String)]) -> Vec<Violation> {
    api_sources
        .iter()
        .filter(|(_, body)| body.contains("SourceStore"))
        .filter(|(path, _)| {
            let exempt = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|name| OPEN_PATH_BOUNDARY_EXEMPTIONS.contains(&name));
            !exempt
        })
        .map(|(path, _)| Violation::OpenPathNamesSourceStore { path: path.clone() })
        .collect()
}

/// Run every rule over both files and collect the violations, in the order `main.rs`
/// should report them.
pub fn check(compose_contents: &str, containerfile_contents: &str) -> Vec<Violation> {
    let mut violations = check_compose(compose_contents);
    violations.extend(check_containerfile(containerfile_contents));
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like the real deploy/compose.yaml, trimmed to what these checks read.
    const CORRECT_COMPOSE: &str = "\
name: lapidary

services:
  db:
    build:
      context: .
      dockerfile: db/Containerfile
    environment:
      POSTGRES_USER: ${POSTGRES_USER:-lapidary}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?set POSTGRES_PASSWORD in .env}

  api:
    build:
      context: ..
      dockerfile: deploy/Containerfile
    environment:
      DATABASE_URL: postgres://${POSTGRES_USER:-lapidary}:${POSTGRES_PASSWORD}@db:5432/lapidary
      LAPIDARY_BIND: 0.0.0.0:8080
      LAPIDARY_ROLE: api
    ports:
      - \"8080:8080\"

  worker:
    build:
      context: ..
      dockerfile: deploy/Containerfile
      args:
        # Only the worker links the CAD kernel — the open path (api) never invokes it.
        SERVER_FEATURES: mock-kernel
    environment:
      DATABASE_URL: postgres://${POSTGRES_USER:-lapidary}:${POSTGRES_PASSWORD}@db:5432/lapidary
      LAPIDARY_ROLE: worker

  web:
    build:
      context: ..
      dockerfile: deploy/web/Containerfile
    depends_on:
      - api

volumes:
  lapidary-db:
";

    // Shaped like the real deploy/Containerfile, trimmed to what these checks read.
    const CORRECT_CONTAINERFILE: &str = "\
# syntax=docker/dockerfile:1
FROM docker.io/library/rust:1.95-trixie@sha256:443dd9a3260cf23c22fc05051dd5661dd7b4028d3d25dbaffab6563b63c3539c AS build
ARG SERVER_FEATURES=
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}

FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f
COPY --from=build /src/target/release/lapidary-server /usr/local/bin/lapidary-server
EXPOSE 8080 8081
ENTRYPOINT [\"/usr/local/bin/lapidary-server\"]
";

    #[test]
    fn correct_configuration_passes() {
        assert_eq!(check(CORRECT_COMPOSE, CORRECT_CONTAINERFILE), vec![]);
    }

    #[test]
    fn server_features_on_a_non_listed_service_fails_and_names_it() {
        let bad = CORRECT_COMPOSE.replacen(
            "  api:\n    build:\n      context: ..\n      dockerfile: deploy/Containerfile\n",
            "  api:\n    build:\n      context: ..\n      dockerfile: deploy/Containerfile\n      args:\n        SERVER_FEATURES: mock-kernel\n",
            1,
        );
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::UnexpectedKernelLink {
                service: "api".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("api"));
    }

    #[test]
    fn worker_missing_server_features_fails() {
        let bad = CORRECT_COMPOSE.replacen(
            "      args:\n        # Only the worker links the CAD kernel — the open path (api) never invokes it.\n        SERVER_FEATURES: mock-kernel\n",
            "",
            1,
        );
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::MissingKernelLink {
                service: "worker".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("worker"));
    }

    #[test]
    fn column_zero_comment_banner_does_not_hide_services_below_it() {
        // The bug this replaced: a section banner comment at column 0 (ordinary YAML style
        // inside a mapping — today's file merely happens to indent its comments) satisfied
        // the old "non-blank, unindented line ends the block" terminator, so every service
        // below it — api included — became invisible and its kernel-linked SERVER_FEATURES
        // raised nothing.
        let bad = CORRECT_COMPOSE.replacen(
            "  api:\n    build:\n      context: ..\n      dockerfile: deploy/Containerfile\n",
            "# ---- application services ----\n  api:\n    build:\n      context: ..\n      dockerfile: deploy/Containerfile\n      args:\n        SERVER_FEATURES: mock-kernel\n",
            1,
        );
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::UnexpectedKernelLink {
                service: "api".to_owned()
            }]
        );
    }

    #[test]
    fn kernel_linked_service_absent_from_the_file_entirely_fails_and_names_it() {
        // Deleting the worker service outright used to give a green run: check_compose only
        // ever walked the services the parser found, so a KERNEL_LINKED_SERVICES entry that
        // never parsed at all raised nothing. This pins the reverse check.
        let bad = CORRECT_COMPOSE.replacen(
            "  worker:\n    build:\n      context: ..\n      dockerfile: deploy/Containerfile\n      args:\n        # Only the worker links the CAD kernel — the open path (api) never invokes it.\n        SERVER_FEATURES: mock-kernel\n    environment:\n      DATABASE_URL: postgres://${POSTGRES_USER:-lapidary}:${POSTGRES_PASSWORD}@db:5432/lapidary\n      LAPIDARY_ROLE: worker\n\n",
            "",
            1,
        );
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::MissingKernelLink {
                service: "worker".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("worker"));
    }

    #[test]
    fn list_form_args_setting_server_features_is_recognized() {
        // Compose accepts `args:` as a list (`- SERVER_FEATURES=mock-kernel`) as well as a
        // mapping. Recognizing only the mapping form would make this fixture report
        // MissingKernelLink — telling the reader their worker ships kernel-less when it
        // would not, since the arg genuinely reaches the build either way.
        let list_form = CORRECT_COMPOSE.replacen(
            "      args:\n        # Only the worker links the CAD kernel — the open path (api) never invokes it.\n        SERVER_FEATURES: mock-kernel\n",
            "      args:\n        - SERVER_FEATURES=mock-kernel\n",
            1,
        );
        assert_eq!(check_compose(&list_form), vec![]);
    }

    #[test]
    fn worker_missing_lapidary_role_fails_and_names_it() {
        // The worker case matters most: losing LAPIDARY_ROLE here is the silent failure
        // this rule exists to catch — the container would still start, bind, and pass its
        // healthcheck, and simply never mount /scan.
        let bad = CORRECT_COMPOSE.replacen("      LAPIDARY_ROLE: worker\n", "", 1);
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::MissingRole {
                service: "worker".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("worker"));
        assert!(violations[0].to_string().contains("LAPIDARY_ROLE"));
    }

    #[test]
    fn api_missing_lapidary_role_fails_and_names_it() {
        // The rule is generic over every service that builds deploy/Containerfile, not
        // special-cased to worker — pin that api is checked too.
        let bad = CORRECT_COMPOSE.replacen("      LAPIDARY_ROLE: api\n", "", 1);
        let violations = check_compose(&bad);
        assert_eq!(
            violations,
            vec![Violation::MissingRole {
                service: "api".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("api"));
    }

    #[test]
    fn services_not_running_lapidary_server_need_no_lapidary_role() {
        // CORRECT_COMPOSE's db (db/Containerfile) and web (deploy/web/Containerfile)
        // services never set LAPIDARY_ROLE and never should — neither builds
        // deploy/Containerfile, so neither runs lapidary-server. Pinned directly (not just
        // via correct_configuration_passes returning an empty vec) so a future reader can
        // see this was checked on purpose.
        let violations = check_compose(CORRECT_COMPOSE);
        assert!(
            !violations.iter().any(|v| matches!(
                v,
                Violation::MissingRole { service } if service == "db" || service == "web"
            )),
            "db and web must never be asked for LAPIDARY_ROLE: {violations:?}"
        );
    }

    #[test]
    fn hardcoded_features_flag_in_build_line_fails() {
        let bad = CORRECT_CONTAINERFILE.replace(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "RUN cargo build --release --locked -p lapidary-server --features mock-kernel ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
        );
        let violations = check_containerfile(&bad);
        assert_eq!(violations, vec![Violation::BuildLineHardcodedFeatures]);
    }

    #[test]
    fn arg_before_first_from_fails() {
        // Pins the first-ARG-wins semantics of `arg_decl = lines.iter().position(...)`:
        // this fixture prepends an invalid top-level ARG in front of CORRECT_CONTAINERFILE,
        // which still carries its own correctly-placed `ARG SERVER_FEATURES=` inside the
        // build stage. A real `docker build` would see the in-stage re-declaration and
        // succeed — but `.position()` locks onto the *first* occurrence (the prepended,
        // invalid one) and flags a violation anyway. That's over-flagging a file that would
        // actually build fine, not under-flagging a broken one — the safe direction for a
        // static check — so it's left as-is rather than taught to consider every ARG
        // occurrence. A future fixer: this is intended behavior, not a bug this test missed.
        let bad = format!("ARG SERVER_FEATURES=\n{CORRECT_CONTAINERFILE}");
        let violations = check_containerfile(&bad);
        assert_eq!(violations, vec![Violation::ArgNotVisibleToBuildLine]);
    }

    #[test]
    fn arg_after_second_from_fails_even_though_it_is_after_the_first() {
        // The bug this replaced: checking only `arg_i < first_from_i` accepts this,
        // because the ARG is indeed after the *first* FROM — it just isn't in the stage
        // that runs `cargo build`, so the expansion sees nothing.
        let bad = CORRECT_CONTAINERFILE
            .replacen("ARG SERVER_FEATURES=\n", "", 1)
            .replacen(
                "FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f\n",
                "FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f\nARG SERVER_FEATURES=\n",
                1,
            );
        let violations = check_containerfile(&bad);
        assert_eq!(violations, vec![Violation::ArgNotVisibleToBuildLine]);
        let msg = violations[0].to_string();
        assert!(
            msg.contains("FROM") && msg.contains("stage"),
            "message must describe the actual problem (wrong stage), not just \
             'before the first FROM' — got {msg:?}"
        );
    }

    #[test]
    fn lowercase_from_between_arg_and_build_line_hides_the_stage_boundary() {
        // Dockerfile instructions are case-insensitive, so a lowercase `from` starts a
        // real stage exactly like `FROM` does. Before from_indices matched
        // case-insensitively, this `from` would not register as a stage boundary at all —
        // ARG SERVER_FEATURES would look visible to the build line (no FROM detected
        // between them), and the check would report OK on a Containerfile where the arg
        // silently does not reach the build. Asserting the full vector, not just
        // `contains`, also catches a case-insensitive match firing twice (once via a
        // hypothetical fallback) and producing a spurious second violation.
        let bad = CORRECT_CONTAINERFILE.replacen(
            "ARG SERVER_FEATURES=\nWORKDIR /src\n",
            "ARG SERVER_FEATURES=\nfrom docker.io/library/rust:1.95-trixie@sha256:443dd9a3260cf23c22fc05051dd5661dd7b4028d3d25dbaffab6563b63c3539c AS extra\nWORKDIR /src\n",
            1,
        );
        let violations = check_containerfile(&bad);
        assert_eq!(violations, vec![Violation::ArgNotVisibleToBuildLine]);
    }

    #[test]
    fn arg_correctly_placed_in_a_three_stage_file_passes() {
        let three_stage = "\
# syntax=docker/dockerfile:1
FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f AS deps
RUN apt-get update

FROM docker.io/library/rust:1.95-trixie@sha256:443dd9a3260cf23c22fc05051dd5661dd7b4028d3d25dbaffab6563b63c3539c AS build
ARG SERVER_FEATURES=
WORKDIR /src
RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}

FROM docker.io/library/debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f
COPY --from=build /src/target/release/lapidary-server /usr/local/bin/lapidary-server
EXPOSE 8080 8081
ENTRYPOINT [\"/usr/local/bin/lapidary-server\"]
";
        assert_eq!(check_containerfile(three_stage), vec![]);
    }

    #[test]
    fn comment_mentioning_cargo_build_above_the_real_run_line_does_not_misfire() {
        let with_comment = CORRECT_CONTAINERFILE.replacen(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "# Remember: cargo build needs SERVER_FEATURES threaded through the ARG below.\nRUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            1,
        );
        assert_eq!(check_containerfile(&with_comment), vec![]);
    }

    #[test]
    fn wrapped_run_cargo_build_line_with_expansion_on_the_continuation_passes() {
        let wrapped = CORRECT_CONTAINERFILE.replacen(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "RUN cargo build --release --locked -p lapidary-server \\\n    ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            1,
        );
        assert_eq!(check_containerfile(&wrapped), vec![]);
    }

    #[test]
    fn backslash_terminated_comment_above_the_run_line_does_not_absorb_it() {
        // A comment ending in `\` is still just a comment — Docker does not extend
        // comments across a trailing backslash. Joining it into the RUN line below would
        // make the whole thing one `#`-prefixed logical line, which the comment-excluding
        // search then skips, misreporting ContainerfileMissingBuildLine on a valid file.
        let with_comment = CORRECT_CONTAINERFILE.replacen(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "# feature list comes from the SERVER_FEATURES build arg \\\nRUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            1,
        );
        assert_eq!(check_containerfile(&with_comment), vec![]);
    }

    #[test]
    fn backslash_terminated_comment_above_the_arg_declaration_does_not_absorb_it() {
        // Symmetric case: a `\`-terminated comment directly above ARG SERVER_FEATURES=
        // must not swallow it into a comment line either, or the check would misreport
        // ContainerfileMissingArgDeclaration on a file that declares the arg correctly.
        let with_comment = CORRECT_CONTAINERFILE.replacen(
            "ARG SERVER_FEATURES=\n",
            "# threaded in by deploy/compose.yaml \\\nARG SERVER_FEATURES=\n",
            1,
        );
        assert_eq!(check_containerfile(&with_comment), vec![]);
    }

    #[test]
    fn comment_inside_an_open_continuation_is_skipped_not_spliced_in() {
        // Pins the deliberate choice documented on logical_lines: a comment appearing
        // between two backslash-continued physical lines is dropped from the joined
        // content (matching BuildKit, which ignores it) rather than ending the
        // continuation early or being spliced into the instruction text.
        let with_mid_comment = CORRECT_CONTAINERFILE.replacen(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "RUN cargo build --release --locked -p lapidary-server \\\n    # SERVER_FEATURES flows through the ARG above\n    ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            1,
        );
        assert_eq!(check_containerfile(&with_mid_comment), vec![]);
    }

    #[test]
    fn compose_with_no_services_key_fails_as_parse_stale_not_as_clean() {
        let no_services = "name: lapidary\n\nvolumes:\n  lapidary-db:\n";
        let violations = check_compose(no_services);
        assert_eq!(violations, vec![Violation::ComposeMissingServicesKey]);
        // Must not be mistaken for "no violations found".
        assert_ne!(violations, Vec::<Violation>::new());
    }

    #[test]
    fn containerfile_with_no_cargo_build_line_fails_as_parse_stale() {
        let no_build = "FROM docker.io/library/rust:1.95-trixie AS build\nARG SERVER_FEATURES=\nWORKDIR /src\n";
        let violations = check_containerfile(no_build);
        assert_eq!(violations, vec![Violation::ContainerfileMissingBuildLine]);
    }

    #[test]
    fn missing_arg_declaration_entirely_fails_as_parse_stale() {
        let no_arg = "FROM docker.io/library/rust:1.95-trixie AS build\nWORKDIR /src\nRUN cargo build --release -p lapidary-server\n";
        let violations = check_containerfile(no_arg);
        assert!(violations.contains(&Violation::ContainerfileMissingArgDeclaration));
    }

    #[test]
    fn compose_with_services_key_but_no_blocks_fails_as_parse_stale() {
        // services: present but with no two-space-indented name: lines under it.
        let odd = "services:\n# nothing recognizable here\nvolumes:\n  lapidary-db:\n";
        let violations = check_compose(odd);
        assert_eq!(violations, vec![Violation::ComposeNoServicesParsed]);
    }

    #[test]
    fn missing_arg_expansion_fails() {
        let bad = CORRECT_CONTAINERFILE.replace(
            "RUN cargo build --release --locked -p lapidary-server ${SERVER_FEATURES:+--features \"$SERVER_FEATURES\"}",
            "RUN cargo build --release --locked -p lapidary-server",
        );
        let violations = check_containerfile(&bad);
        assert_eq!(violations, vec![Violation::BuildLineMissingArgExpansion]);
    }

    #[test]
    fn violation_messages_point_at_the_check_not_the_config_when_parsing_is_stale() {
        let no_services = "name: lapidary\n\nvolumes:\n  lapidary-db:\n";
        let msg = check_compose(no_services)[0].to_string();
        assert!(msg.contains("This check's parsing is stale, not the config"));
        assert!(msg.contains("deploy.rs"));
    }

    #[test]
    fn a_file_naming_source_store_fails_and_names_the_file() {
        let sources = vec![
            (
                "crates/lapidary-api/src/handlers/open.rs".to_owned(),
                "use lapidary_storage::SourceStore;\n".to_owned(),
            ),
            (
                "crates/lapidary-api/src/handlers/thumbnail.rs".to_owned(),
                "use lapidary_storage::DerivativeStore;\n".to_owned(),
            ),
        ];
        let violations = check_open_path_boundary(&sources);
        assert_eq!(
            violations,
            vec![Violation::OpenPathNamesSourceStore {
                path: "crates/lapidary-api/src/handlers/open.rs".to_owned()
            }]
        );
        assert!(violations[0].to_string().contains("handlers/open.rs"));
    }

    #[test]
    fn files_naming_only_derivative_store_pass() {
        let sources = vec![(
            "crates/lapidary-api/src/handlers/thumbnail.rs".to_owned(),
            "use lapidary_storage::DerivativeStore;\n".to_owned(),
        )];
        assert_eq!(check_open_path_boundary(&sources), vec![]);
    }

    #[test]
    fn scan_rs_is_exempted_because_it_only_mounts_under_the_worker_role() {
        // Task 9's ingest handler legitimately names SourceStore — it is the one route
        // router() (crates/lapidary-api/src/lib.rs) mounts only under Role::Worker.
        let sources = vec![(
            "crates/lapidary-api/src/scan.rs".to_owned(),
            "use lapidary_storage::SourceStore;\n".to_owned(),
        )];
        assert_eq!(check_open_path_boundary(&sources), vec![]);
    }

    #[test]
    fn the_exemption_is_scoped_to_scan_rs_by_name_not_to_every_file() {
        // Naming SourceStore in a different file — including a hypothetical open-path
        // handler sitting beside scan.rs — must still fail. This is what pins the
        // exemption to `scan.rs` specifically rather than the whole crate silently
        // regaining a blanket pass.
        let sources = vec![
            (
                "crates/lapidary-api/src/scan.rs".to_owned(),
                "use lapidary_storage::SourceStore;\n".to_owned(),
            ),
            (
                "crates/lapidary-api/src/parts.rs".to_owned(),
                "use lapidary_storage::SourceStore;\n".to_owned(),
            ),
        ];
        assert_eq!(
            check_open_path_boundary(&sources),
            vec![Violation::OpenPathNamesSourceStore {
                path: "crates/lapidary-api/src/parts.rs".to_owned()
            }]
        );
    }
}
