# design-sync notes — lapidary-web

Repo-specific gotchas for `/design-sync`. Read this before a re-sync.

## Shape

- Lapidary's frontend is an **application**, not a published component library: no
  Storybook, no library build, `private: true`, and no `main`/`module`/`exports`. So
  `shape: "package"`, and the component list cannot come from shipped `.d.ts` — there are
  none.
- The component surface is chosen explicitly by **`web/.ds-entry.tsx`**, a re-export barrel
  committed alongside this config. `cfg.entry` points at it. Add a component to the import
  by exporting it there *and* adding it to `cfg.componentSrcMap`; both are required, because
  with no `.d.ts` tree the discovery pass finds nothing on its own.
- `.ds-entry.tsx` sits outside `web/src/` deliberately. `web/tsconfig.json`'s `include` is
  `["src", "vite.config.ts", "vitest.config.ts"]`, so `npm run build` never typechecks it and
  the app build pays nothing for the sync.

## The 2026-09-18 refresh (the Lit Bench)

The first sync (8 Sep) predates the Lit Bench refinement. The refresh changed:

- **Fonts.** Inter is gone from `web/public/fonts/`; the app is **Archivo** for words and
  **JetBrains Mono** for figures, two subsets each. `cfg.extraFonts` lists the four woff2s. The
  project still holds `fonts/inter-*` from the first sync — the re-sync's plan should delete them.
- **Surface: 17 components** (was 4), in three kinds by what they need — see `web/.ds-entry.tsx`'s
  header and `conventions.md`'s table. Every one has a hand-written `dtsPropsFor` body; keep them
  in step with source in the same commit.
- **`DesignProviders`** (`web/.ds-providers.tsx`, re-exported from the entry) replaces the
  `cfg.provider`/`extraEntries` route the first notes proposed: previews and designs wrap
  themselves, so nothing depends on a config key's schema. It gives a memory router (what `Link`
  needs in `AppFrame`, `Card`, `Crash`) and a `QueryClient` with queries off, plus `seed` —
  `[queryKey, answer]` pairs — so a data component can draw with data. It is an export, not a
  card: it is deliberately absent from `componentSrcMap`.
- **Authored previews**: `AppFrame` (Parts with a seeded `FolderTree` rail; Sharing in `Page.ts`
  anatomy), `Card` (detail, gallery, list, selected), `Grid` (both densities, picking with
  `SelectionBar`, loading), `Menu` (Library, opened on mount with `showPopover()`; closed), and
  `Dialog` brought up to today's classes, and `Icon` (the six glyphs; one inside a control). The
  bracket's picture is `raster.rs`'s golden render of
  `fixtures/bracket-lp-1042-03`, inlined as a data URL; the other fixture parts show the empty well
  on purpose. The remaining components take the floor card.
- **Every preview paints its own ground** (`Ground` in each `.tsx`). The card page's template
  body is white and covers the dark `:root`, so a preview that relied on `:root` came out as
  Lapidary controls on a white page. Designs have the same trap: a screen's outermost element
  should paint `bg-[var(--color-bg)]` itself.
- **Checked before the run**: the previews and both new modules typecheck against the real
  components (a throwaway tsconfig mapping `lapidary-web` to `.ds-entry.tsx` and `react` to
  `web/node_modules/@types/react`), and `cfg.buildCmd` produces a `dist/ds.css` carrying both
  font families, every token (`--color-lamp` included) and every utility `conventions.md` names.

## Build

- Run from the **repo root**, with `--node-modules web/node_modules` (there is no root
  `node_modules` — this is a Rust repo with a web subdirectory).
- `cfg.buildCmd` copies the hashed Vite CSS to a stable name:
  `cd web && npm run build && cp dist/assets/index-*.css dist/ds.css`. **Do not point
  `cssEntry` at `dist/assets/index-<hash>.css`** — the hash changes with every content
  change and the config would rot. `dist/ds.css` is the stable copy. Equally, do not point
  it at `web/src/styles.css`: that is Tailwind *source*, where `@import "tailwindcss"` is
  unresolved and `@theme` is uncompiled.
- **Vite empties `web/dist/` on every build**, which takes `dist/ds.css` with it. Run the
  `cp` *after* the build, every time — that is why `buildCmd` chains them rather than listing
  them separately. Symptom when you forget: the converter prints `[CSS_RUNTIME] no static CSS
  found` and ships a bundle with no tokens, no fonts and no component styles, while the render
  check still passes 4/4 and validate still exits 0. Nothing else catches it, so treat that
  warn line as fatal for this repo.
- `--entry` must NOT be used on the command line for this repo — `cfg.entry` covers it.
  Passing a `dist/assets/index-*.js` would bundle the *app*, which runs `main.tsx`'s
  `createRouter`/`RouterProvider` bootstrap on import.

## Prop contracts are hand-written

With no `.d.ts` tree, the extractor emits `[key: string]: unknown` for every component — an
empty API contract, which is worse than useless to the design agent. All four components
therefore have hand-written bodies in `cfg.dtsPropsFor`. **When a component's props change
in source, update `dtsPropsFor` in the same commit** — nothing checks this automatically.

## Upload

The first import is **uploaded and anchored**: project `20053b00-d7ba-496e-abb6-06e58c40f99d`
("Lapidary"), 28 files, `_ds_sync.json` written last. The id is pinned in `config.json`, so
the next run is a re-sync — fetch that project's `_ds_sync.json` into
`.design-sync/.cache/remote-sync.json` and pass it as `--remote` to `resync.mjs`, which
re-verifies only what moved.

Note the first run needed `/design-login` partway through: `DesignSync` returns an
authorization error rather than a permission prompt when design scopes are missing. Relay
its message and wait — do not treat it as a tool failure, and do not poll it.

Two ordering rules that are cheap to honour and permanent to get wrong:

- **`_ds_sync.json` is the absolute final write**, in its own `write_files` call, after every
  content write and every delete. It is the anchor that vouches for the rest; written early,
  a failure part-way leaves it vouching for files the project does not have, and no future
  diff ever repairs them.
- **`_ds_needs_recompile` fences the upload** — write it first, and re-write it at the end.
- Any write or delete failure that retries do not clear means **stop**: no sentinel re-arm,
  no `_ds_sync.json`. An un-anchored project merely re-verifies next sync.

`DesignSync(report_validate)` was sent with
`{total: 4, bad: 0, thin: 0, variantsIdentical: 0, iterations: 3}`.

## Known render warns

- The 2026-09-18 render check: 17/17, `bad: 0`, `thin: 0`, `variantsIdentical: 0`. Any warn on a
  future run is new — look at it.
- **Eight floor cards, by design.** `Detail`, `FolderTree`, `MovePartDialog`, `ShareDialog` read
  the query cache and have no authored preview; `Figure`, `GridSkeleton`, `MeasureBar`,
  `SelectionBar` draw from props alone but nobody authored them (`GridSkeleton` and `SelectionBar`
  appear inside `Grid`'s preview). All are importable; authoring any of them is the standing offer.
- `extraFonts: copied <woff2> — add a matching @font-face` fires once per font on every build.
  Noise: the compiled CSS carries the `@font-face` rules and the build rewrites their `url()`s to
  `./fonts/`. Verified present in `_ds_bundle.css`.
- The `AppFrame` Parts card shows category names cut to a letter or two in the rail. That is the
  real `FolderTree`: a row's Share/Rename/Delete buttons are `opacity-0` until hover but keep
  their width, so a 14rem rail leaves the name almost nothing. An app bug, not a sync one.

## Re-sync risks

- **Data components need `DesignProviders`** (settled 2026-09-18, see the refresh above).
  `QueryClientProvider` is not a `window.Lapidary` export and `cfg.provider.component` fails
  fatally on a name that isn't one, which is why the wrapper is a bundle export of its own and
  every preview that needs it wraps itself. It lives **outside `web/src/`**:
  `src/no-bare-strings.test.ts` fails any user-facing string in `src/` that does not route
  through `src/lib/strings.ts`, and preview-only strings have no business there.
- **`fonts/fonts.css` carries root-absolute `url(/fonts/…)`** where `_ds_bundle.css` carries
  correct relative `./fonts/…`. Harmless today because `styles.css` imports `fonts.css`
  first and `_ds_bundle.css` second, so the correct rules win the cascade — but if that
  import order ever changes, Archivo and JetBrains Mono silently fall back.
- **The shipped stylesheet is Lapidary's compiled Tailwind subset** (~150 utilities), not
  all of Tailwind. `conventions.md` documents this and tells the design agent to fall back
  to inline styles over tokens. If the app's own class usage shrinks, the vocabulary
  available to designs shrinks with it — re-read `conventions.md`'s caveat section against
  the fresh build on every sync.
- **Worktrees branch from `origin/main` by default**, which was ~7 commits behind local
  `main` when this ran. A stale base silently produced a 47-line `styles.css` and a build
  missing every token, `@font-face` and keyframe. If tokens look absent, check
  `git log -1` before believing it.
- Preview grades live in the gitignored `.design-sync/.cache/`; the durable record of what
  was verified is the uploaded project's `_ds_sync.json`. A sync that never uploaded leaves
  the project un-anchored, and the next run re-verifies everything. That is the safe state.
