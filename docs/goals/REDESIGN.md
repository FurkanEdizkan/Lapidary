# REDESIGN — the front door, motion, and the whole app (A16–A25)

Owner-approved plan, 2026-10-18. Each goal below gets its own `A<n>.md` when claimed; this is the shared brief.

## Context

The owner, 2026-10-18, said the part page's big picture "appears restricted to the view": a square store picture is
cropped to a wide band. They also called the UI/UX "meh". They asked for:
- a sign-in page split in two, with an animation on the left and the sign-in form on the right;
- a short animated transition into setup, then a better setup page;
- a whole-app redesign that uses motion.dev, anime.js, impeccable, the taste skills and Vercel's frontend skills.

**Decisions the owner made:**
- **Motion:** add the free `motion` package (motion.dev, MIT), pinned exactly and bundled so offline installs work. It
  sits beside anime.js. motion.dev/ui's Motion+ components are paid, so equivalents are built in-house.
- **Sign-in left half:** a self-drawing SVG triangle-mesh lattice. A signed-out page cannot show anyone's data.
- **Scope:** the whole app, in goals.

**What exploration found:**
- The cause of the crop: `Preview` (`web/src/components/PartDetail.tsx` ~792-874) applies `pictureStyle(image)`, and a
  stored picture's fit is `cover`. That goes into a wide box with a fixed height (~1285). QuickLook shares `Preview`.
- `styles.css` gives every element a base `transition` on transform and opacity, which smears an engine that writes
  styles every frame. Every Motion element therefore needs `transition-none`.
- Sign-in and setup are not driven by the rig (stack.sh runs setup with curl), so redesigning them can't break its flows.
  The grid, quick look, part page and dialogs are driven by the rig, so they must keep their accessible names.
- The written rules this changes: CLAUDE.md Style (anime.js is the only engine), DESIGN.md Motion, ARCHITECTURE.md:201,
  and `web/src/lib/motion.test.ts:85` (only `motion.ts` may import anime). AGENTS.md is regenerated from CLAUDE.md.
- The rules that stay:
  - dark only;
  - the DESIGN.md palette (Layout Blue at rest only on the brand mark);
  - self-hosted Archivo and JetBrains Mono, with no Google Fonts;
  - WCAG 2.2 AA (`contrast.test.ts`);
  - every string through `strings.ts` (`no-bare-strings.test.ts`);
  - a `<title>` on every route, and `role="list"` on lists;
  - the no-polling test;
  - hovering a card loads no mesh, and opening a part reads no model file;
  - every figure keeps its ≈;
  - no invented testimonials or user counts (PRODUCT.md calls the product pre-alpha);
  - Phosphor icons vendored as paths.

## Design direction: evolve the look, don't replace it

The identity stays: the dark grounds, four greys, one accent, both typefaces, and the ≈ mark. What's new is "the
cutting bench": geometry drawn as fine grey lines. The lattice is the one decorative motif; everywhere else it appears
only as hairlines.

**Sign-in**
- At `lg` and wider, a 55/45 grid.
- **Left half:** the `stage-lamp` pool, and on it a lattice of a part from a pure, deterministic `lattice()` (a
  low-poly spur gear or bracket).
  - It draws itself stroke by stroke, with an accent "draw head" while drawing, and settles to grey in under 5 s.
  - Then it stays still: nothing loops (WCAG 2.2.2).
  - It carries the diamond, the wordmark and one pitch line from strings.
- **Right half:** the form, `max-w-sm`, keeping today's labels and names.
- **Below `lg`:** the lattice becomes a short band above the form.

**The intro into setup**
- In `routes/setup.tsx`, once per page load, at most 900 ms.
  1. The diamond and lattice draw in at the centre (0-500 ms).
  2. The lattice slides into the left column (`layoutId`) while the wizard rises 8px and fades in.
- A key, a click or the visible **Skip** ends it. With reduced motion it does not play.

**The setup wizard**
- Split like sign-in. On the left, the lattice and a vertical stepper (Account, Server, Options) with
  `aria-current="step"` and a hairline progress rail that moves with a `layoutId` marker.
- On the right, the step content in `AnimatePresence mode="wait"`: 120 ms out to the left, 180 ms in from the right,
  reversed on Back.
- Still one `<form>`, and nothing is saved until Finish. A refused setup code goes back to the step with that field.
- **Done:** the lines settle and the diamond fills while `followRestart` or `followPort` runs, never delaying them.

**The app shell**
- A 56px header, with a hairline and a blur behind it once the page scrolls.
- The current place in Parts, Dashboard and Shared is underlined by a shared `layoutId` marker (180 ms, no bounce).
- Menus scale from 0.98 and fade in over 120 ms.

**Grid cards**
- The picture fills the tile (as A15 made it). Hover lifts the card 1px and brightens its edge, with no glow and no
  blue.
- Name in Archivo 600, figures in mono with ≈, and a format chip.
- Skeletons stay static, or shimmer once. `arrive()` is unchanged.

**The part page**
- The stage shows the whole picture on the lamp (A16).
- The gallery becomes a filmstrip under the stage. Choosing a picture crossfades it in over 180 ms. A15's reorder
  controls stay.
- View becomes an overlay control on the stage, keeping its name. `flipFrom` from the card stays.

**Dialogs:** Motion presence replaces the `panel-in` and `scrim-in` keyframes. Focus handling is unchanged.

## Motion architecture

- **Where each engine lives.**
  - `animejs` is imported only by `web/src/lib/motion.ts`: timelines, the new `drawIn()` (`createDrawable` and
    stagger), `arrive`, `tween`.
  - `motion/react` is imported only by `web/src/lib/motion-react.tsx` and `web/src/lib/motion-features.ts`.
    - `motion-react.tsx` holds `MotionRoot`: `LazyMotion` (`strict`, with `domMax` loaded lazily from
      motion-features) plus `MotionConfig reducedMotion="user"`.
    - It also holds `Presence` and the primitives `Fade`, `Rise`, `Step` and `Indicator`, each `m.div` with
      `transition-none`.
    - Feature code uses only these primitives.
- **Shared tokens.** They stay in `styles.css @theme`.
  - The anime-free math (`tokens`, `durationMs`, `reduced`, `staggerStep`, `curve`) moves into
    `web/src/lib/motion-tokens.ts` and is re-exported by `motion.ts`, so the root never pulls anime into the entry
    bundle.
  - New tokens: `--duration-scene: 480ms` and `--duration-intro: 900ms`.
- **Doctrine** (written into CLAUDE.md and DESIGN.md):
  - A change of state takes 120, 180 or 280 ms on the one curve.
  - Springs are for layout only, critically damped (`visualDuration 0.28, bounce 0`).
  - Authored moments exist only at the front door, and every one can be skipped.
  - Nothing loops.
  - Only transform, opacity and SVG `stroke-dashoffset` are animated. The lattice is rotated through its SVG
    `transform` attribute.
  - With reduced motion, everything jumps to its end state. `MotionRoot` passes `{duration: 0}` when `reduced()` is
    true.
  - Blue at rest is still a defect.
- **Routes.** `__root.tsx` wraps `<Outlet>` in `<Rise key={pathname}>`, enter only, at 180 ms. No exit, so two pages
  never share the DOM.
- **Containment test.** `motion.test.ts` is generalised:
  - an anchored animejs import is allowed only in `motion.ts`;
  - a `motion` or `motion/react` import only in the two files above;
  - `framer-motion` is banned.
  `test-setup.ts` sets `MotionGlobalConfig.skipAnimations = true`.
- **Budget.** A18 measured the entry at 94.16 kB gzip (+10.3 kB for LazyMotion and MotionConfig); later goals may add at most 4 kB to it. The lattice is a lazy
  import from sign-in and setup only.

## Goals, in order

Each goal gets the usual protocol: a BOARD row and `docs/goals/<ID>.md`, `scripts/claim-goal.sh`, the gate, and a
`--no-ff` merge. Goals in the same wave run as parallel agents in their own worktrees. Builds take turns under the
heavy lock, and only one e2e stack runs at a time. Everyone rebases onto main before merging (`strings.ts` and
`styles.css` are shared).

| ID | Goal | Depends | Wave |
|---|---|---|---|
| A16 | The part page shows the whole picture: `style={stage ? {objectFit:'contain'} : pictureStyle(image)}` (cards keep `cover`), with a test that finds the hero by `strings.parts.pictureAlt` on a `cover` fixture | — | 1 |
| A17 | The direction, docs only: audit every route's screenshots (redesign-existing-projects, an impeccable critique), stress-test with design-taste-frontend and high-end-visual-design, then write DESIGN.md (Doorway, Lattice, Stepper, Motion), CLAUDE.md Style and ARCHITECTURE.md:201, and regenerate AGENTS.md | — | 1 |
| A18 | The motion foundation: pin `motion`; `motion-tokens.ts`, `motion-react.tsx`, `motion-features.ts`; `MotionRoot` in the root; `drawIn()`; the containment test; the test skip; the bundle baseline | A17 | 2 |
| A19 | Sign-in: `Doorway` gains an `aside`; `lib/lattice.ts` (pure, unit-tested: edge count, no duplicate edges, everything in the viewBox, deterministic) and `components/Lattice.tsx`; login.tsx | A18 | 3 |
| A21 | The app shell, grid cards and route enter (AppFrame, Grid, Card, Skeleton, `__root.tsx`), with all rig flows | A18 | 3 |
| A22 | Dialogs: presence for Dialog, ImportDialog, ShareDialog and FolderPicker; remove the old keyframes | A18 | 3 |
| A20 | Setup: the intro, the stepper wizard, step presence, the done moment; setup.test.tsx (stepper semantics, Skip, reduced motion) | A19 | 4 |
| A23 | The part page: hero overlay, filmstrip, studio polish; the part-detail, view-original, view-tools, pictures and delete flows stay green | A16, A21 | 4 |
| A24 | Dashboard, settings, people, profile, tags and shared, page by page on `Page.ts` | A21 | 4 |
| A25 | The finish pass: screenshots at 1440 and 390, reduced motion on and off; a web-design-guidelines review, impeccable-finish-reviewer, impeccable-documenter to bring DESIGN.md in line with what shipped; the full rig, the gate and the budget | all | 5 |

**How the skills are used.**
- frontend-design, design-taste-frontend and high-end-visual-design set the direction once, in A17.
- From then on, every goal's finish review (impeccable-finish-reviewer) checks against DESIGN.md.
- web-design-guidelines (Vercel's) is the accessibility and interaction audit in A17 and A25.

## Verification

- **Every goal:** `cargo xtask verify slice` in the foreground (web tests, typecheck, build), and all existing guards
  stay green.
- **New tests:**
  - A16's crop test;
  - the containment test;
  - the lattice geometry test;
  - stepper `aria-current` and Back/Next;
  - Skip on a key press, and no intro under reduced motion;
  - the new colour pairs in `contrast.test.ts`.
- **The rig** (`scripts/e2e/stack.sh` build, up, seed, drive: 17 seed checks, 27 flows) runs before merging A21, A22,
  A23 and A25. A name change updates the flows in the same commit. A new `doorway` flow signs out, sees the sign-in
  form by role, and on a fresh stack skips the intro and reaches step 1.
- **Screenshots:** headless Chrome through `scripts/e2e/cdp.mjs session({shots})` of each changed route at 1440×900
  and 390×844, with reduced motion off and on. The lattice must be in its final frame under reduced motion. I look at
  every screenshot before merging.
- **Bundle:** the entry gzip from `vite build` is recorded in A18 and checked in every later goal, at most +8 KB.
- **After each merge:** the gate runs on main, and `./install.sh` rebuilds the owner's install on :3000. Nothing is
  pushed until the owner asks.

## Taken from A17's audit (lead, 2026-10-18)

- The settled, still lattice stands in for a missing picture on cards and on the part page's stage: most parts have
  none, and forty identical "STL" squares was the audit's first finding. `lattice()` then loads with the grid's
  chunk, not the entry, and must stay inside the entry budget. A21 (cards) and A23 (stage) own it.
- A23's main case is the part with no picture: the lamp, the lattice, the format, and View on the stage.
- A21 owns a one-row header under `md`, and a current-place marker on every page (Settings, People, Profile, Tags,
  the part page included). The places marker moves in 180 ms on the curve; springs stay for the stepper rail and the
  lattice slide.
- A22 re-checks the Import dialog's first focus with a real pointer click (`Input.dispatchMouseEvent`); a blue ring at
  rest after a pointer open is a defect to fix in `Dialog.tsx`'s initial focus.
