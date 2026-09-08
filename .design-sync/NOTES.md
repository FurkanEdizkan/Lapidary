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

- None. The final render check reported `bad: 0`, `thin: 0`, `variantsIdentical: 0` across
  all four components. Any warn on a future run is genuinely new — look at it.

- Three of four components (`Detail`, `FolderTree`, `MovePartDialog`) ship the **floor
  card** by design — the user scoped preview authoring to `Dialog` only on the first sync.
  `3 showing the typographic floor card` is expected, not a regression. They are fully
  importable regardless; authoring their previews is the standing offer on any re-sync.
- `extraFonts: copied <woff2> — add a matching @font-face` fires twice on every build. It is
  noise here: the compiled CSS already carries both `@font-face` rules and the build rewrites
  their `url()`s to `./fonts/`. Verified present in `_ds_bundle.css`.

## Re-sync risks

- **Previewing the other three needs a QueryClient.** `Detail`, `FolderTree` and
  `MovePartDialog` call `useQuery`/`useMutation`/`useQueryClient`, and
  `@tanstack/query-core` is inlined into the bundle but `QueryClientProvider` is **not** a
  `window.Lapidary` export. `cfg.provider.component` is validated against the bundle's
  export list and fails fatally on a name that isn't there, so a bare `QueryClientProvider`
  will not work. Authoring those previews means adding a tiny wrapper module — a provider
  around a pre-seeded `QueryClient` — via `cfg.extraEntries` with a `./`-relative path.
  Put that module **outside `web/src/`**: `src/no-bare-strings.test.ts` fails any
  user-facing string in `src/` that does not route through `src/lib/strings.ts`.
- **`fonts/fonts.css` carries root-absolute `url(/fonts/…)`** where `_ds_bundle.css` carries
  correct relative `./fonts/…`. Harmless today because `styles.css` imports `fonts.css`
  first and `_ds_bundle.css` second, so the correct rules win the cascade — but if that
  import order ever changes, Inter silently falls back.
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
