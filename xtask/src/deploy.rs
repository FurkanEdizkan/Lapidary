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
                 (xtask/src/deploy.rs) but does not set SERVER_FEATURES, so its image would \
                 build without the CAD kernel it needs. Add `args: SERVER_FEATURES: \
                 mock-kernel` (or the current feature name) under {service} in \
                 deploy/compose.yaml, or remove {service} from KERNEL_LINKED_SERVICES if it \
                 no longer needs the kernel."
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
        }
    }
}

/// Parse `deploy/compose.yaml`'s `services:` block into `(service name, sets
/// SERVER_FEATURES)` pairs.
///
/// Line-wise, not a YAML parser: a service name is a line indented by exactly two spaces
/// ending in `:` (`  worker:`); everything more deeply indented until the next such line —
/// or the next unindented top-level key — belongs to that service's body. Within a body,
/// any non-comment line starting with `SERVER_FEATURES:` (after its own leading
/// whitespace) marks that service as setting it, regardless of nesting depth (it always
/// lives under `build: args:`, but this does not require that exact path — a boring, tolerant
/// check).
fn parse_services(contents: &str) -> Result<Vec<(String, bool)>, Violation> {
    let lines: Vec<&str> = contents.lines().collect();
    let Some(start) = lines.iter().position(|l| *l == "services:") else {
        return Err(Violation::ComposeMissingServicesKey);
    };

    let mut services: Vec<(String, bool)> = Vec::new();
    let mut current: Option<(String, bool)> = None;

    for line in &lines[start + 1..] {
        // A non-blank, unindented line ends the services: block (e.g. a top-level
        // `volumes:` key).
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
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
                current = Some((name.to_owned(), false));
                continue;
            }
        }

        if let Some((_, sets_features)) = current.as_mut() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') && trimmed.starts_with("SERVER_FEATURES:") {
                *sets_features = true;
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
pub fn check_compose(contents: &str) -> Vec<Violation> {
    let services = match parse_services(contents) {
        Ok(services) => services,
        Err(parse_violation) => return vec![parse_violation],
    };

    let mut violations = Vec::new();
    for (name, sets_features) in &services {
        let is_kernel_linked = KERNEL_LINKED_SERVICES.contains(&name.as_str());
        match (sets_features, is_kernel_linked) {
            (true, false) => violations.push(Violation::UnexpectedKernelLink {
                service: name.clone(),
            }),
            (false, true) => violations.push(Violation::MissingKernelLink {
                service: name.clone(),
            }),
            _ => {}
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
fn logical_lines(contents: &str) -> Vec<String> {
    let mut logical = Vec::new();
    let mut buffer = String::new();
    let mut continuing = false;

    for raw in contents.lines() {
        if continuing {
            buffer.push(' ');
            buffer.push_str(raw.trim_start());
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

    let from_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with("FROM "))
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
    // comment lines the same way parse_services does for deploy/compose.yaml.
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
        assert!(violations.contains(&Violation::ContainerfileMissingBuildLine));
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
}
