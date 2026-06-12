---
name: conventional-branches
description: >-
  Name a git branch following the Conventional Branch spec
  (conventionalbranch.org): <type>/<short-description>, lowercase,
  hyphen-separated, optionally issue-numbered. Use whenever you are about to
  create a branch, are asked to start work or cut a branch for an issue, need
  help choosing a branch type or slug, or want to rename a branch that doesn't
  match the convention. If the repo defines its own branch convention
  (CONTRIBUTING.md, an established branch-naming style), follow that — this skill
  is the general baseline.
---

# Conventional Branches

Name branches after the [Conventional Branch](https://conventionalbranch.org/)
spec:

```text
<type>/<short-description>
```

## Check what the project expects first

Before naming a branch, match the repo's existing practice:

- **CONTRIBUTING.md** — may define the required branch format (allowed types,
  whether an issue number is mandatory, separators). **It overrides the generic
  guidance here.**
- **`git branch -a` / the remote** — mirror the prefixes and slug style already
  in use (issue-numbered `feat/123-...` vs bare `feat/...`, which types appear).
- **Branch protection / PR rules** — most repos protect the main branch; branch
  off it and land changes via Pull Request rather than pushing to it directly.

If none of these exist, the spec below is a safe default.

## The format

- **type** — required, lowercase, from the list below.
- **`/`** — a single forward slash separates the type from the description.
- **description** — a concise, lowercase, hyphen-separated summary of the work.
- **issue number (optional but recommended)** — include the tracker/issue id to
  keep work traceable: `<type>/<issue#>-<short-slug>` (e.g.
  `feat/123-oauth-login`).

## Branch types

| Type                | Use for                                     |
| ------------------- | ------------------------------------------- |
| `feat` / `feature`  | a new feature                               |
| `fix` / `bugfix`    | a bug fix                                    |
| `hotfix`            | an urgent production fix                     |
| `release`           | preparing a release (e.g. `release/v1.2.0`) |
| `chore`             | tooling, deps, housekeeping, non-code tasks |
| `docs`              | documentation only                          |

The spec defines `feature`, `bugfix`, `hotfix`, `release`, `chore` and accepts
`feat`/`fix` as short aliases. Many teams extend this with the commit types
(`docs`, `refactor`, `test`, `ci`, …) so the branch and commit vocabularies
match — follow the repo's set when it has one.

## Character & formatting rules

Straight from the spec:

- Use only **lowercase letters (`a-z`), numbers (`0-9`), and hyphens (`-`)** to
  separate words. **No** uppercase, underscores, spaces, or other special
  characters.
- Hyphens (and dots) must **not** appear consecutively (`feat/new--login` ✗) and
  must **not** start or end the description (`feat/-login-` ✗).
- Dots are allowed **only** in release version numbers (`release/v1.2.0`).
- Keep it **descriptive yet concise** — indicate the purpose of the work, don't
  paste the whole issue title.

## Examples

```text
feat/oauth-device-login          # new feature
feat/123-oauth-login             # new feature, issue-numbered
fix/header-overflow              # bug fix
hotfix/security-patch            # urgent production fix
release/v1.2.0                   # release prep (dot allowed in version)
chore/bump-eslint                # tooling/deps
docs/api-deployment-guide        # docs
```

Invalid (rejected by the spec):

```text
Feat/OAuth-Login        # uppercase
feat/new--login         # consecutive hyphens
fix/_header_bug         # underscores
chore/update deps       # space
add-login               # missing type prefix
```

## Workflow — cutting a branch

1. **Branch off an up-to-date base** (usually the protected main branch):

   ```bash
   git switch main && git pull
   ```

2. **Pick the type** by the *intent* of the work, not the file kind (touching a
   test file is still `test/`).
3. **Find the issue number** if the work is tracked, and include it.
4. **Write a short kebab-case slug** — a few lowercase, hyphen-separated words.
5. **Create the branch:**

   ```bash
   git switch -c feat/123-oauth-login
   ```

To **rename** a branch that doesn't match the convention:

```bash
git branch -m <old-name> <type>/<short-description>
# already pushed? update the remote:
git push origin -u <new-name> && git push origin --delete <old-name>
```

## Quick checklist

- [ ] starts with an allowed lowercase `type`
- [ ] a single `/` after the type
- [ ] slug is lowercase `a-z`, `0-9`, hyphen-separated — no spaces/underscores
- [ ] no consecutive, leading, or trailing hyphens/dots
- [ ] dots only in a `release/` version number
- [ ] issue number included when the work is tracked
- [ ] descriptive yet concise
- [ ] branched off an up-to-date base
