---
name: conventional-commits
description: >-
  Write git commit messages that follow the Conventional Commits 1.0.0 spec, in
  any repository. Use whenever you are about to create a commit, are asked to
  commit changes, need help choosing a type/scope, or want to fix a commit
  message a commit-lint check rejected. If the repo defines its own commit
  conventions (CONTRIBUTING.md, a commitlint config, an established git log
  style), follow those — this skill is the general baseline.
---

# Conventional Commits

A commit message follows [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):

```text
<type>(<optional scope>): <description>

<optional body>

<optional footer(s)>
```

## Check what the project enforces first

Before committing, glance at how the repo handles commits and match it:

- **`commitlint.config.*`** (often extending `@commitlint/config-conventional`)
  — the project lints commit messages; the rules below apply.
- **`.husky/commit-msg`** or other git hooks — messages are validated locally on
  commit; a bad message is rejected before it lands.
- **CONTRIBUTING.md** — may list the allowed types, scopes, and issue-reference
  style this repo expects. **These override the generic guidance here.**
- **`git log`** — mirror the existing style (scopes used, body conventions).

If none of these exist, still write Conventional Commits — it's a safe default.

## Format rules

- **type** — required, lowercase, from the list below.
- **scope** — optional; in parentheses, lowercase (e.g. a module/area: `api`,
  `auth`, `ui`, `deps`). Omit it for repo-wide changes.
- **description** — required: `: ` then a short summary in the imperative mood
  ("add", not "added"/"adds"), lowercase start, no trailing period.
- **header** (first line) — keep it short; commitlint/config-conventional caps
  it at **≤ 72** by default for the subject (and many setups enforce ≤ 100).
- **body / footer lines** — `@commitlint/config-conventional` enforces each line
  **≤ 100 characters**; hard-wrap long paragraphs. Even without commitlint,
  wrapping the body (~72 chars) is good practice.
- a **blank line** must separate header from body, and body from footer.

## Allowed types

| Type       | Use for                                                  |
| ---------- | -------------------------------------------------------- |
| `feat`     | a new feature                                            |
| `fix`      | a bug fix                                                |
| `docs`     | documentation only                                       |
| `refactor` | code change that neither fixes a bug nor adds a feature  |
| `test`     | adding or correcting tests                               |
| `chore`    | build, deps, tooling, housekeeping                       |
| `ci`       | CI configuration / pipeline changes                      |
| `perf`     | a performance improvement                                |
| `build`    | build system or external dependency changes              |
| `style`    | formatting/whitespace, no code-meaning change            |
| `revert`   | reverting a previous commit                              |

`feat` and `fix` are defined by the spec; the rest are the widely-used
config-conventional set. Use whichever matches the change's *intent*.

## Breaking changes

Signal a breaking change in **either** way (or both):

- a `!` after the type/scope: `feat(api)!: drop legacy v1 endpoints`
- a footer: `BREAKING CHANGE: <what broke and the migration path>`

## Referencing issues

Reference the issue in the summary or body (e.g. `(#12)`) per the project's
convention. Auto-close keywords like `Closes #12` usually belong in the **PR**
description, not the commit — check the repo's practice.

## Examples

```text
feat(auth): add OAuth2 device-code login

fix(api): return 404 instead of 500 for unknown ids (#231)

docs: document the deployment workflow

refactor: extract request retry logic into a helper

chore: bump eslint to v9

feat(db)!: switch primary keys to uuidv7

BREAKING CHANGE: integer ids are no longer accepted; run the
2024_uuid_migration before deploying.
```

## How to commit (workflow)

When asked to commit, don't guess a message — derive it from the diff:

1. **Look at what changed.** Run `git status` and `git diff --staged` (and
   `git diff` for unstaged). Stage the relevant files if nothing is staged.
2. **Pick the type** based on the *intent* of the change, not the file kind
   (editing a test file to add coverage is still `test:`).
3. **Pick a scope** — a module/area name — or omit it for repo-wide changes.
   Reuse scopes already seen in `git log`.
4. **Write the header**: imperative mood, short, lowercase after the colon,
   no trailing period.
5. **Add a body** (after a blank line) when the change needs the "why" or has
   notable details. Wrap lines ≤ 100 chars (≤ 72 is nicer). Reference issues.
6. **Add footers** for breaking changes (`BREAKING CHANGE:`) or co-authors.
7. **Commit** with a multi-line message so the body survives:

   ```bash
   git commit -m "feat(auth): add device-code login" \
              -m "Adds the RFC 8628 flow behind a feature flag. (#42)"
   ```

   If a `commit-msg` hook or CI rejects it, read the lint output and fix the
   offending part — usually a missing/invalid type, a capitalized or
   period-terminated subject, an over-length header, or a body line > 100 chars.

## Quick checklist before committing

- [ ] starts with an allowed lowercase `type`
- [ ] scope (if any) is in `()` and lowercase
- [ ] `: ` then an imperative, lowercase summary with no trailing period
- [ ] header is short (≤ 72, hard cap commonly 100)
- [ ] body/footer lines ≤ 100 chars (commitlint default)
- [ ] breaking change marked with `!` and/or a `BREAKING CHANGE:` footer
- [ ] body separated by a blank line; issue referenced per repo convention
