<!-- Superseded in part on 2026-09-08 by `Lapidary Library v2.dc.html`, the design project
     at claude.ai/design/p/e221a512. The frontmatter below is updated to what the code now
     carries, and on 2026-09-13 the Colors, Typography and Shapes prose was rewritten to
     match it rather than left marked as stale.

     v2 is a refinement rather than a replacement — the same dark neutral register, lifted
     off black, with the blue-grey cast taken out of the greys and a colder accent. What
     actually changed: Archivo replaces Inter, JetBrains Mono arrives for every figure, the
     grounds move up one step, tiles get a border back (v2 insets the render, so a tile has
     no edge of its own any more), and a control radius joins the ladder.

     Two of v2's values are NOT carried, and both are contrast: its fourth text grey
     (#6a6a72) measures 3.2:1 on a card where SC 1.4.3 wants 4.5:1, and its control border
     (#3a3a42) is 1.5:1 where 1.4.11 wants 3:1. PRODUCT.md commits to WCAG 2.2 AA. The
     substitutes and the measurements are in `web/src/styles.css`.

     2026-09-17: reconciled with the refined Lit Bench build (studio part page, turntable,
     first-run bench, one header, page anatomy). The lamp ground, the z scale, the xs
     breakpoint and motion.ts are recorded; the headline hierarchy and the weight Don't are
     rewritten to what the build does, and the stale accent hex and tile-border Don't are
     corrected. Where a rule changed, the section says so.

     2026-10-03, THE PIVOT. Lapidary is a visual library: pictures and search, and a 3D view
     only when somebody presses **view**. The render pipeline and the CAD kernel are gone
     (`docs/ROADMAP.md` § "The pivot" has the shas). The turntable, the thumbnail shading,
     the first-run bench's scene, measuring and the assembly and PMI sections went with them.

     2026-10-18, A17: the full pass the pivot asked for, and the redesign's direction
     (`docs/goals/REDESIGN.md`, owner-approved). The prose below no longer describes the
     removed things; where a rule rested on one, it is rewritten or gone. New: Doorway,
     Lattice, Stepper, Dialogs, the stage and filmstrip, and a Motion section with two
     engines. A15 made a card's picture fill its tile, so the Inset Picture Rule is now The
     Filled Tile Rule. The palette, type, radii, z scale and breakpoints are unchanged, and
     `web/src/styles.css` is still the live copy of every token. Two motion tokens named
     here, `--duration-scene` and `--duration-intro`, are added by A18; until A18 merges
     they exist only in this file. Where this file describes a component a later goal
     (A19–A24) builds, it says so.

     2026-10-18, A25: the redesign's finish pass. Every route was shot at 1440 and 390 with
     reduced motion on and off, and this file now says what A18–A24 shipped where it had
     drifted: the route enter inside AppFrame, the still Lattice for a missing picture, the
     format chip, Card framing, FACT and HINT, the Quicklime stepper marker, the grey checkbox,
     and the words-that-act tier of quiet controls. -->
---
name: Lapidary
description: A visual index for 3D part libraries, where the part is lit and the bench is not.
colors:
  anodised-black: "#121214"
  machined-slate: "#1a1a1d"
  bench-grey: "#17171b"
  lamp: "#202024"
  scribe-line: "#26262b"
  edge: "#65656d"
  chalk: "#e6e6e9"
  quicklime: "#f0f0f2"
  ash: "#c8c8ce"
  graphite: "#8a8a92"
  layout-blue: "#2cb4f5"
  brass: "#e8b06a"
  patina: "#4f9e94"
  verdigris: "#8fd7d0"
  oxide: "#e88a8a"
typography:
  display:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "1.875rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.025em"
  headline:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.025em"
  scope:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "normal"
  title:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: 1.375
    letterSpacing: "normal"
  body:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: "0.05em"
  wordmark:
    fontFamily: "Archivo, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "0.16em"
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: "normal"
    fontFeature: "tnum"
rounded:
  sm: "4px"
  ctl: "7px"
  md: "10px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  xxl: "32px"
components:
  card-part:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.quicklime}"
    typography: "{typography.title}"
    rounded: "{rounded.md}"
    padding: "12px"
  card-well:
    backgroundColor: "{colors.bench-grey}"
    padding: "0"
  button-standing:
    backgroundColor: "{colors.anodised-black}"
    textColor: "{colors.quicklime}"
    rounded: "{rounded.ctl}"
    padding: "8px 16px"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.graphite}"
    rounded: "{rounded.ctl}"
    padding: "0 8px"
    height: "24px"
  button-quiet-hover:
    backgroundColor: "transparent"
    textColor: "{colors.chalk}"
    rounded: "{rounded.ctl}"
    padding: "0 8px"
    height: "24px"
  chip-filter-active:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.full}"
    padding: "4px 12px"
  input-text:
    backgroundColor: "{colors.bench-grey}"
    textColor: "{colors.chalk}"
    typography: "{typography.body}"
    rounded: "{rounded.ctl}"
    padding: "6px 8px"
  header:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    padding: "9px 13px"
  dialog-surface:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.md}"
    padding: "16px"
  stage:
    backgroundColor: "{colors.lamp}"
    rounded: "{rounded.md}"
  lattice:
    textColor: "{colors.edge}"
    strokeWidth: "1px"
---

# Design System: Lapidary

## Overview

**Creative North Star: "The Lit Bench"**

A machinist's bench under a single task lamp. The parts are lit; the bench is not. Every
surface in this interface exists to be the unlit surround for a pictured object — the ground
is near-black, the chrome is grey on grey, and the only saturated colour in the room appears
when you are doing something. The user's pinned reference is ArtStation's community browse
page, and what it gets right is exactly this: a dark room, a dense wall of work, and chrome
so quiet you stop seeing it after four seconds.

The register is quiet, precise and unhurried. Nothing raises its voice. Trust is earned by
being exact rather than by being emphatic, which is why secondary text outnumbers primary
text and why the interface has no decorative colour at all. This is a tool a person opens
forty times a day for six seconds each time; it must be legible at a glance and invisible in
memory.

The lamp falls in one kind of place: a stage. The part page and the quick look each have one,
and it holds the part's own picture, whole, on the lamp's pool. When somebody presses **view**,
three.js draws the original mesh on that same pool, set down on a soft contact shadow.
Everything outside a stage is flat, tonal and silent.

**The cutting bench** (2026-10-18). The redesign adds one motif and no new colour: geometry
drawn as fine grey lines, the way a part looks laid out on a marking table before it is cut.
At full size it is the Lattice, and it appears in only two places, the sign-in page and setup.
Everywhere else the same idea shows only as hairlines: the Scribe Line, a stepper's rail, the
marker under the current place.

**Key Characteristics:**

- Near-black ground; the part on its lamp-lit stage is the only lit thing on the page
- Archivo for every word, JetBrains Mono for every figure; headings share one weight (600) and part by size and colour
- One saturated colour, reserved for live interaction and the Lattice's draw head while it moves
- Flat by tonal layering; a shadow only under something that floats over the page
- A picture fills its card's tile; the card's hairline lifts to Edge under the pointer
- Motion is mechanical: transform and opacity, 120/180/280 ms, one curve, always reduced-motion aware; authored moments only at the front door
- One decorative motif, the drawn Lattice, and only on the pages before you are signed in

## Colors

Named for the machine shop the product serves. The greys are a single stepped ramp from
unlit ground to hairline; the accent is the only hue, and the four semantics are the only
other saturated values in the system.

### Primary

- **Layout Blue** (`#2cb4f5`): the machinist's marking dye. It marks what is *live right
  now* — a valid drop target, the focus ring, a selected card's outline, upload progress,
  text selection and the caret, and the Lattice's draw head while it is drawing. It is never
  used for emphasis, never for a heading, and never on a resting control: Upload and Download
  stand without it.

### Neutral — four grounds

- **Anodised Black** (`#121214`): the page ground, the drawer, and the fill of a standing control.
- **Bench Grey** (`#17171b`): the well a thing sits *in* — a card's image well, an input,
  the search field, the header's places switch.
- **Machined Slate** (`#1a1a1d`): every raised surface — cards, dialogs, menus, the header,
  the selection bar, a stage's control dock. One step up from the ground, never two.
- **Lamp** (`#202024`): the centre of the lamp's pool on a stage, fading to Anodised Black at
  the box's nearest edges. The only ground that is not flat. Every text token clears 4.5:1 on
  it (Graphite, the lowest, 4.74:1); Edge does not (2.81:1), which is why a stage's controls
  sit on a Machined Slate dock and never on the lamp itself.

### Neutral — four texts, and why not five

- **Quicklime** (`#f0f0f2`): headings, names, and the one value a row exists to show.
- **Chalk** (`#e6e6e9`): primary text — what the user is reading right now.
- **Ash** (`#c8c8ce`): body copy that is not a label — leads, notes, a part number beside its figures.
- **Graphite** (`#8a8a92`): labels, metadata, resting controls. Most text.

`v2` draws a fifth at `#6a6a72` for small mono captions. It is not here. Measured against
the grounds it sits on it is 3.24:1 on a card and 3.49:1 on the page, under the 4.5:1 SC
1.4.3 asks of body text — and at 9–11px no large-text exemption applies. Lifting it to pass
lands eight values off Graphite, a distinction nobody can see. So the tier collapses.

### Lines

- **Scribe Line** (`#26262b`): hairline *dividers*, the rule under a page section's label,
  the header's underline, and the resting border of a card, menu or dialog.
- **Edge** (`#65656d`): the boundary of anything you can operate — an input, a button, the
  selection bar, a card under the pointer, the approximate label's dotted underline. 3.24:1 on
  the ground and 3.01:1 on a card, because SC 1.4.11 asks 3:1 of a control's visual boundary.
  `web/src/contrast.test.ts` guards it.

### Semantics

Four, each earned by a state the interface has, each clearing 4.5:1 on a card so any of
them may carry text: **Brass** (`#e8b06a`) a caution, **Patina** (`#4f9e94`) a settled
good, **Verdigris** (`#8fd7d0`) an informational note, **Oxide** (`#e88a8a`) a failure.

### Named Rules

**The Marking Dye Rule.** Layout Blue appears only where something is happening — dragging,
focused, selected, pressed, uploading, drawing. If a screenshot taken at rest contains Layout
Blue anywhere but the brand mark, that is a defect. Its rarity is what makes it readable
without a legend. A native checkbox takes Ash as its `accent-color` (A25): a box that is checked
by default is at rest, and setup's ticked "Import models from … links" was a solid blue square
in a screenshot nobody had touched.

**The Marked Part Rule** is retired with measuring (2026-10-03). Nothing draws on a part any
more, so the image well has no exemption.

**The Fixed Greys Rule.** Text is Quicklime, Chalk, Ash or Graphite and nothing between. A
fifth text grey is how a hierarchy becomes a gradient nobody can parse; when something needs
to recede further, it leaves the screen rather than fading.

**The One Step Up Rule.** A surface may sit exactly one tonal step above its parent
(ground → slate). Two stacked panels do not get two stacked lightnesses; they get a scribe
line between them.

## Typography

**Body Font:** Archivo (with `-apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif`)
**Figure Font:** JetBrains Mono (with `ui-monospace, SFMono-Regular, Menlo, monospace`)

**Character:** Two faces with one job each. Archivo carries every word; JetBrains Mono
carries every number and every identifier. The split is not decoration — a column of
proportional digits is ragged, and a value set in mono is a value the user can select,
copy and paste somewhere that will accept it. `.tabular` applies both the face and
`tabular-nums`, so a count that changes as pages load does not shift the words beside it.

### Hierarchy

- **Display** (600, 1.875rem, 1.25, −0.025em): an empty library's heading (`FirstRun`), and only that.
- **Headline** (600, 1.5rem, 1.25, −0.025em): the one headline of a page past the grid — a
  part's name on its page, Settings, People, Shared with me, Sign in, the crash page.
- **Scope** (600, 15px, 1): the line above the grid naming what is on screen, and the title
  of an empty or no-result grid state.
- **Title** (600, 13–14px, 1.375): card names, clamped to two lines and broken at `-`, `_`
  and `.` rather than mid-word, in the detail layout; the gallery overlay truncates to one
  line, because a two-line name there covers the part. The gallery's name carries no `<wbr>`:
  Chrome breaks at one even under `nowrap`, which is how it wrapped there until A25.
- **Body** (400, 0.875rem, 1.5): prose, leads, helper text, refusals. Capped at 70ch.
- **Label** (500, 0.75rem, 0.05em, uppercase, Graphite): a page section's label over its
  scribe line, rail headings, metadata keys.
- **Wordmark** (700, 0.75rem, 1, 0.16em, uppercase): "Lapidary" in the header. Uppercased in
  CSS, never in the string, so a screen reader says the name rather than spelling it.
- **Figure** (JetBrains Mono 400, 10.5–12px, tabular): part numbers, triangle counts, byte
  sizes, dimensions, volumes, hashes, storage paths.

**This hierarchy changed.** The previous version stated a 1.25rem/500 headline, a 900-weight
display horizon and a "no weight between 500 and 900" Don't. None of that was buildable: the
self-hosted Archivo file spans 400–700 only, so a 900 never rendered. The build settles on
600 for every heading and name, 500 for labels, 700 for the wordmark alone, and it separates
the levels by size and colour (Quicklime over Ash over Graphite), not by a weight gap.

### Named Rules

**The Copyable Mono Rule.** Anything a user might paste into another program or compare
against another value — a path, a hash, a part number, a dimension — is set in mono
and is selectable. Mono is a promise that the string is exact and complete, never elided in
the middle. Words are not figures: a sentence that happens to contain a number stays in Archivo,
and so does a value cell that holds a sentence ("Not available — give the part its material…"),
even in a column of mono values. The grid's count line ("50 parts so far.") is a sentence too, so it
is Archivo with tabular digits, which keeps a growing count from shifting the words after it.

**The Self-Hosted Font Rule.** Lapidary ships into air-gapped deployments. Archivo (400–700)
and JetBrains Mono (400–600) are self-hosted from `web/public/fonts/` as variable fonts, two
subsets each (`latin`, `latin-ext`), so Turkish renders without a mid-word fallback.

**The One Headline Rule.** A page has one Headline, and the wordmark is not competing with it.
Sections below it open on a Label, never on a second Headline.

## Layout

A sticky header over a fluid page. At `md` and up a 14rem category rail sits beside the grid;
under `md` the rail becomes a left drawer (18rem, at most 85vw) over a 60% black scrim, and
the header's search takes a full row of its own.

The grid reflows by whole tiles — `repeat(auto-fill, minmax(11rem, 1fr))` with a 16px gutter
at comfortable density, `minmax(8rem, 1fr)` with 12px at compact — and under **xs (35rem)**
it is forced to two columns, because one card the width of a phone is a list of pictures
rather than a grid. The gutter separates tiles; no rule is drawn between them.

**The part page is a stage and a column.** At `lg` it is two zones: the stage on the left
(sticky under the header, up to 72vh) with its filmstrip directly under it, and a 22rem column
on the right read top to bottom — name, Download and the ⋯ menu, geometry, then what people
recorded. Rarely needed matter (file, identity, history) sits under both in two columns after a
scribe line. Below `lg`, source order is reading order: name and Download first, then the
stage and filmstrip, then the rest.

**The doorway is a split.** The two pages before you are signed in (sign-in and setup) have no
header. At `lg` they are a 55/45 grid; below it, one column. See Doorway.

**Page anatomy** (`web/src/components/Page.ts`), shared by every page past the grid: one
Headline, a lead no wider than 70ch in Ash, then sections that open on a Label over a
Scribe Line (40px above, 16px below) rather than living in bordered boxes, and one standing
control. Boxes are for things that float or that you pick up. `Page.ts` also holds `FACT` (a value
nobody can change here) and `HINT` (12px Graphite at 60ch, under a field or a fact), and the grid's
empty and no-result states use the same `LEAD`, `HINT` and `QUIET` rather than sizes of their own.

Spacing runs 4 / 8 / 12 / 16 / 24 / 32. Inside a card footer, 12px; inside a dialog or menu,
12–16px; between the part page's zones, 24–32px. Prose is capped at 70ch everywhere, dialogs included.

**Layers are four and no others:** header 20, drawer 30, overlay 40 (dialogs, the drop
overlay — a dialog opened from inside the drawer must not open behind it), skip link 50.
Native popover menus sit in the top layer and need no number.

## Elevation & Depth

**Flat by doctrine.** Depth on the page is tonal layering and hairlines, never shadow: ground
→ surface → scribe line, plus a 1px hover lift that reads as the object rising toward the lamp.
The header is the one sticky surface that changes once the page scrolls under it: it gains a
Scribe Line, so content passing beneath reads as beneath. It stays opaque Machined Slate and is
never blurred (A21): a see-through header over a white picture drops its Edge borders under 3:1
and its grey labels under 4.5:1.
A card under the pointer brightens its edge and lifts; it casts nothing.

**One exception in the page: something floating over it.** A dialog, a menu, and the
selection bar pinned over the grid while you pick parts carry the overlay shadow, because
each must read as *above* the page rather than merely lighter. Dialogs pair it with a
`rgba(0,0,0,0.6)` scrim, which does most of the work.

**Inside a stage, light is real.** The lamp's radial pool and the part's contact shadow are
the scene's lighting, not interface elevation: a soft round falloff at 60% black under the
part's lowest point, 1.5× its footprint, so a part stands on the ground instead of floating
over it.

### Shadow Vocabulary

- **Overlay** (`box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6)`): dialogs, menus and the floating
  selection bar only. Never on a card, a button, an input or anything in the page flow.
- **Contact** (3D scene only, 60% black radial falloff): under a part the viewer draws. Never a CSS shadow.

### Named Rules

**The Unlit Bench Rule.** Surfaces do not cast shadows, because the light is on the parts, not
on the furniture. The only shadows are the one an overlay throws by being in front of the page,
and the one a part throws on the lamp's ground.

**The Lamp Rule.** The lamp's pool (Lamp at 50% 42% fading to Anodised Black at the box's
closest side) is the one gradient in the system and belongs only under a part or a drawing of
one: the part page's stage, the quick look's stage, the viewer a **view** press opens, and the
Doorway's left half under the Lattice. `closest-side`, so no rectangle shows where it stops.

## Shapes

Three radii and a pill. **4px** for machined chrome — a callout label, the skip link, the
focus ring's corner. **7px** for things you operate — buttons, inputs, list rows, the drawer's
controls. **10px** for cards, stages, dialogs and menus, which hold or float over content.
**Full** for the active-filter chip (a Layout Blue edge: a live narrowing) and the scrollbar thumb.

Borders are 1px. Cards, menus and dialogs rest on a Scribe Line border and a card lifts to Edge
under the pointer; dividers keep the Scribe Line always; anything you can operate is bounded by
Edge. A card's border is not an ornament on the picture: a photograph runs to the tile's edge,
and against a dark photograph the hairline is the only thing that says where the card stops.

Radii nest: a 10px card or dialog holds 7px controls, and a 7px control holds a 4px chip.
Something inside a rounded thing never has a larger radius than its parent.

### Named Rules

**The Filled Tile Rule** (A15; replaces The Inset Picture Rule). A card's picture
fills its square tile, framed by its row's `fit` and `focus` (`cover` by default), because the
owner's pictures are photographs and store renders with their own backgrounds, and a 7% inset
only made them a third smaller. A **stage** is the opposite: it shows the whole picture,
`contain`, on the lamp (A16), because the stage is where you look at the part, and a crop there
hides the thing you came to see. The tile crops; the stage never does.

A part with no picture has the still Lattice on the lamp's pool, and its format in the footer's
chip (see Lattice). It never shows a blank that looks like a picture failed to load — the
dashboard's recently-added widget showed one until A25, and now shows the still Lattice too.

**The Operable Edge Rule.** If a person can click, type into or drop onto it, its boundary is
Edge, not Scribe Line. A divider gets the hairline.

## Components

### Motion

**Rewritten 2026-10-18 (A17).** The previous section named one engine; there are now two, each
with a fenced home, under one doctrine. The doctrine is the part that matters: an engine is only
how a rule gets carried out.

**The doctrine.**

- **A change of state takes 120, 180 or 280 ms, on the one curve** (`cubic-bezier(0.2, 0, 0, 1)`).
  120 for hover and a scrim; 180 for a state change — a dialog, a menu, a step, a crossfade, a
  route; 280 for the longest a person waits on anything they asked for — the quick look's flight,
  a whole `arrive()` run. Nothing a person does every day takes longer.
- **Springs are for layout only**, and critically damped: `{ type: 'spring', visualDuration: 0.28,
  bounce: 0 }`. A spring moves something that changed *place* — the stepper's rail marker, the
  Lattice sliding into setup's left column — because a
  layout change can be interrupted halfway and a spring picks up from where it is. It never
  overshoots: bounce is a toy's motion, and this is a tool. Hover, opacity and presence are never
  springs.
- **Authored moments exist only at the front door**, and every one can be skipped. The sign-in
  page's Lattice drawing itself and the intro into setup are the whole list. They get the two
  long tokens: `--duration-scene` (480 ms, one scene inside a moment, such as the diamond and
  Lattice drawing in at the centre) and `--duration-intro` (900 ms, the ceiling for a whole
  moment, end to end). A key, a click or the visible **Skip** ends a moment at its end state.
  Inside the app there are no authored moments: a person opens it forty times a day, and an
  animation that was charming once is a wait the fortieth time.
- **Nothing loops, with one exception.** No shimmer that repeats, no idle drift, no breathing
  glow. Anything that moves on its own stops within 5 s and stays still (WCAG 2.2.2). A skeleton is
  static or shimmers once. The exception is the sign-in drawing (A26, owner 2026-10-18): it keeps
  turning, so it carries a visible Pause, stops while the page is hidden, and is still under
  reduced motion. It is the front door's one loop and nothing inside the app may borrow it.
- **Only `transform`, `opacity` and SVG `stroke-dashoffset` are animated** — `translate`, `scale`
  and `rotate` are transform. The Lattice is rotated through its SVG `transform` attribute, never
  by animating `d`, `points` or a coordinate. Nothing animates `width`, `height`, `top`, `left`,
  `filter`, colour or `box-shadow`; a hover's border brightening is a colour change with no
  transition at all, under the 1px lift that does move.
- **Reduced motion is the end state**, never a slower or shorter version. CSS transitions drop to
  0.01 ms (`styles.css`), `arrive()` and `drawIn()` jump, the flight does not fly, and
  `MotionRoot` passes `{ duration: 0 }` to everything under it when `reduced()` is true. A
  front-door moment does not play at all: the Lattice is drawn and settled in its first frame.
- **Blue at rest is still a defect.** The Lattice's draw head is Layout Blue only while it is
  moving; when a moment ends, or is skipped, or never starts, there is no blue left on the page
  but the brand mark.
- **Every Motion element carries `transition-none`.** `styles.css` gives every element a base
  transition on `transform`, `translate` and `opacity` (so that every hover lift actually runs).
  An engine that writes those properties every frame would have each write eased again by CSS,
  and the motion smears. The primitives in `motion-react.tsx` put `transition-none` on their
  `m.div` themselves, so feature code cannot forget it. `motion.ts` sends DOM targets through
  anime's Web Animations adapter, which no CSS transition sees; anything anime drives by inline
  style instead (A18 checks `drawIn()`'s SVG strokes) carries `transition-none` as well.
- **Motion must say something.** Each animation answers one of: something arrived, something
  left, something changed place, or this is where you are. "It looks nice" is not on the list.

**The two engines, and where each may be imported.** A test (`web/src/lib/motion.test.ts`,
generalised by A18) fails the build if either is imported anywhere else, and bans
`framer-motion` outright.

- **anime.js** (pinned) is imported only by `web/src/lib/motion.ts`. It does sequences and
  staggers on real DOM and SVG nodes, outside React's render:
  - **arrive()** — newly added content (a first page, the next page, a new filter's results):
    opacity plus a 4px rise at 120 ms. The first 12 elements take turns, step
    `min(24 ms, (280 − 120) / (n − 1))`, so the whole run lands inside 280 ms; anything after
    the twelfth lands with it. Reverted on completion so no inline style outranks a hover lift.
    Never for a refetch of what is already on screen. Unchanged by the redesign.
  - **drawIn()** (A18) — the Lattice drawing itself: anime's `createDrawable` and a stagger over
    its edges on `stroke-dashoffset`, inside `--duration-intro`.
  - **tween()** — numbers on a three.js object, at 180 ms by default.
  - **curve()** — the same curve as a function, for a script drawing its own frames.
- **motion** — motion.dev's MIT `motion` package, pinned exactly and bundled by Vite, so an
  air-gapped install has it — is imported only by `web/src/lib/motion-react.tsx` and
  `web/src/lib/motion-features.ts`. It does presence (things entering and leaving React's tree)
  and layout (`layoutId`), which anime cannot see:
  - `MotionRoot`, in the root: `LazyMotion strict` with `domMax` loaded lazily from
    `motion-features.ts`, and `MotionConfig reducedMotion="user"`.
  - `Presence`, and the primitives `Fade`, `Rise`, `Step` and `Indicator`, each an `m.div` with
    `transition-none`. Feature code uses these and nothing else; a component that wants a new
    kind of motion adds a primitive here, under the same rules.
  - Motion+ (motion.dev/ui) is paid and is not used; anything like it is built in-house as a
    primitive.
- **The tokens are shared.** They live in `styles.css @theme` — `--ease-mechanical`,
  `--duration-fast` / `-base` / `-slow`, and A18's `--duration-scene` / `--duration-intro`. The
  anime-free math (`tokens`, `durationMs`, `reduced`, `staggerStep`, `curve`) moves into
  `web/src/lib/motion-tokens.ts`, which both engines read and `motion.ts` re-exports, so the
  root never pulls anime into the entry bundle.
- **Which engine.** Something entering or leaving React's tree, or changing place in the layout:
  motion. A sequence or stagger over nodes that already exist, an SVG drawing itself, a number on
  a three.js object: anime. A single element changing state on hover or focus: CSS, as it always
  was. Never two engines on one element.

**The motion in the app**, after the redesign (A18–A24):

- **Routes.** `AppFrame` wraps the page's content, under its header, in `<Rise key={pathname}>`
  keyed on the router's resolved pathname: enter only, 180 ms, 8px. No exit, so two pages never
  share the DOM and a route change never waits. Not around `<Outlet>` in `__root.tsx`, where REDESIGN
  first put it (A21): every page renders its own header, so a rise there faded the whole header in on
  each navigation, hid the places marker's move, and for 180 ms made the drawer, its scrim and the
  skip link fixed to a transformed wrapper. Settled, the wrapper has no transform. The doorway pages
  and the crash page have no enter.
- **The places marker.** The current place in Parts, Dashboard and Shared with me is underlined by
  one shared `layoutId` marker that moves between them, 180 ms, no bounce.
- **Menus** scale from 0.98 and fade in over 120 ms, from their anchor (`transform-origin` at the
  button).
- **Dialogs** arrive and leave by presence (see Dialogs).
- **The quick look's flight.** The part's picture travels from the card it was opened from to its
  place on the stage, 280 ms, one `transform` measured at the click (`flipFrom`, unchanged). The
  Carried Picture Rule.
- **The filmstrip.** Choosing a picture crossfades it onto the stage over 180 ms.
- **Cards.** `arrive()` on new pages; a 1px lift on hover at 180 ms. Nothing else.

### Doorway

The frame for the two pages a person sees before they are signed in: sign-in and setup
(`web/src/components/Doorway.tsx`). No header, no places, no menu, because none of them work yet.
It is the only place in the product shaped like a front page, and it gets the only authored
motion.

- **At `lg` and wider, a 55/45 grid**, full height (`min-h-dvh`, never `min-h-screen`, which jumps
  under a phone's address bar). **The left half** is an `aside`: the `stage-lamp` pool, the
  Lattice drawn on it, and at its foot the diamond, the wordmark and one pitch line from
  `strings.ts` in Ash. **The right half** is the form, `max-w-sm`, centred in its half both ways,
  with the page's one Headline over it.
- **Why 55/45.** The drawing is the reason the split exists, so it gets the larger half; the form
  needs only 24rem, and a form centred in 45% of a wide screen sits where the eye already is.
  50/50 reads as two equal things competing; this is one thing and its frame.
- **Below `lg`**, one column: the Lattice becomes a band about 9rem tall above the form, the pool
  still under it, and the form follows at full width with a 16px gutter.
- **The left half shows no data**, ever. A signed-out page cannot show anybody's parts, pictures,
  names or counts. The Lattice is drawn from a pure, deterministic function, not from a part. And
  no invented testimonial or user count either: PRODUCT.md calls the product pre-alpha.
- **The form keeps its labels, names and order.** Labels above fields, the standing **Sign in**
  never wrapping, and the forgot-password note under it, never beside it (at 390 beside it wrapped
  the button onto two lines).
- **Setup** uses the same split: the left half holds the Lattice and the Stepper; the right holds
  the step. See Stepper.

### Lattice

The drawn motif: a part laid out as a fine triangle mesh, the way geometry looks before it is cut.
It is the only decoration in the product, which is why it is allowed only where there is nothing
else to show.

- **What it is.** An SVG of a low-poly part — a spur gear or a bracket — produced by a pure,
  deterministic `lattice()` in `web/src/lib/lattice.ts` (A19), unit-tested for its edge count, no
  duplicate edges, every point inside the viewBox, and the same output every time. It is drawn
  by `web/src/components/Lattice.tsx`, `aria-hidden`, because it says nothing a reader needs.
- **Strokes.** 1px (`vector-effect: non-scaling-stroke`), round joins. Settled, the edges are
  **Edge** (`#65656d`) for the silhouette and **Scribe Line** (`#26262b`) for the interior
  triangulation, so the outline reads first and the mesh is texture. No fill except the lamp under
  it. No second hue.
- **The draw head.** While an edge is drawing, its leading end — the last few percent of its
  length — is Layout Blue, the marking dye moving across the bench. The head exists only while the
  stroke is moving; each edge settles to its grey the moment it is complete.
- **It draws in 2.4 s on sign-in**, so it reads as drawing rather than a sweep (owner's call,
  2026-10-18); setup's intro (A20) keeps inside `--duration-intro` and then stays still.
- **On sign-in it keeps moving** (A26, owner 2026-10-18: "it should not stop after one cycle").
  Once drawn, the gear turns about its axle, one tooth pitch every 3 s — which looks the same as
  a full turn, so the loop has no seam — and three short Layout Blue marks travel round the tooth
  profile, the hub's rim (the other way, twice as fast) and the bore, the marking dye still moving
  across the bench. Blue is on only
  what moves, so the rule holds. A quiet **Pause the drawing** control sits at the drawing's foot
  (WCAG 2.2.2), the turn stops while the tab is hidden, and the lines are written straight to the
  SVG each frame by anime.js's `cycle` (`lib/motion.ts`), never through React.
- **Everywhere else it never loops**, never drifts, never redraws on hover or on focus.
- **Reduced motion: static.** It is rendered in its final frame — every edge drawn, every edge
  grey, no blue — and never moves.
- **Drawing only at the front door.** Sign-in and setup draw it, as a lazy import. Under `lg` the band
  is shorter than the drawing, which is cropped by it on purpose (A19), so setup's intro lands into a
  crop as well.
- **Still, it stands in for a missing picture** (A17's audit, taken by the lead): on a card and on
  the part page's stage, `<Lattice still />` is the settled final frame — one shared path tree, no
  `drawIn`, no blue — over the lamp, with the format. Most parts have no picture, and forty
  identical "STL" squares said nothing. It never moves there. The list row, the quick look and the
  dashboard's recently-added widget use the same still frame. It loads with the grid's chunk (which
  already carries anime for `arrive()`), never with the entry.

### Stepper

Setup's wizard (`routes/setup.tsx`, A20), in the Doorway's split.

- **On the left**, under the Lattice: a vertical list of the steps — Account, Server, Options —
  each a 13px Title with the current one in Quicklime and `aria-current="step"`, the done ones in
  Ash with a 12px check glyph, the ones ahead in Graphite. Beside it a 1px Scribe Line rail, with
  a 2px Quicklime marker over the current step's length that moves by `layoutId` (the layout spring).
  Quicklime, as every current-place marker in the product is (the places, the filmstrip): progress
  is not live interaction, so it is not blue, and the Edge it first had was 2.81:1 on the lamp (A25).
- **On the right**, the step's content, in `AnimatePresence mode="wait"`: the old step leaves to
  the left over 120 ms, the new one arrives from the right over 180 ms, 8px each way; **Back**
  reverses both. Under reduced motion it swaps.
- **One `<form>`**, and nothing is saved until **Finish**. Back keeps what was typed. A refused
  setup code goes back to the step that holds that field, with the message under the field.
- **Below `lg`**, the stepper is a single line above the step: "Step 2 of 3 · Server", the rail
  as a horizontal hairline under it, its done part in Quicklime.
- **Server's folders are facts** (`FACT`), as in Settings, and every hint is `HINT`. Back and Skip
  are quiet and bounded by Edge.
- **The intro into setup** (once per page load, inside `--duration-intro`): the diamond and the
  Lattice draw in at the centre over `--duration-scene`; then the Lattice slides into the left
  column (`layoutId`) while the wizard rises 8px and fades in. A key, a click or **Skip** ends it.
  Under reduced motion it does not play.
- **Done.** The Lattice's lines settle and the diamond fills while `followRestart` or `followPort`
  runs. The animation never delays either: if the server answers first, the page moves on.

### Buttons

- **Standing:** Anodised Black fill, 1px Edge, 7px radius, Quicklime label at 14px/600, 8px/16px
  in the page anatomy (`Page.ts`), 6px/12px beside the part page's ⋯ menu (where Copy link is
  quiet: Download is the part page's one standing control). The one action a
  region is about — Download on the part page, Upload in the header (12px, icon plus label, the
  label hidden under `sm`), Sign in, Finish; Download stands in the quick look as well. One per region: when two controls in a row both
  stand, neither does. Never blue at rest; it is brighter and heavier than the quiet controls
  beside it, and that is enough. A label never wraps: shorten it or give it the row.
- **Quiet:** transparent, 1px Edge, 7px radius, Graphite label at 12px, 24px tall (32px beside a
  field, matching the field). Every secondary action: Select, View, New category, Check. Pressed
  takes a Layout Blue edge and a Quicklime label.
- **Hover / Focus:** label lifts Graphite → Chalk, and a standing control rises 1px, over 120 ms.
  Focus is one ring for the whole app: 2px Layout Blue, offset 2px, on `:focus-visible`.
- **Words that act:** in a row of several actions about one thing — a person's New password and
  Remove, Import's Remove beside each folder, the folder picker's path — the action is a Graphite
  word, underlined under the pointer, with no Edge. The row is the boundary; a box round each word
  would draw a table of buttons. This is the only control without an Edge, and only in such a row.
- **Disabled:** 50% opacity, no lift, and visibly different from a resting quiet button.
- **Overlay control** (the stage's View, A23): Machined Slate plate, 1px Edge, 7px radius, Chalk
  label, set on the stage's foot, never directly on the lamp (Edge does not clear 3:1 there).

### Header (AppFrame)

The one header, sticky, 56px, Machined Slate: the mark and wordmark, the places (Parts,
Dashboard, Shared with me), search, then the page's own controls and the account menu. The
brand mark is a 19px Layout Blue diamond, the one resting use of the accent. A skip link sits
above everything at z 50.

- **The current place** is marked by text weight and a 2px Quicklime underline marker that moves
  between places (`layoutId`, 180 ms, no bounce). It replaces the old Bench Grey tray (and the
  tray's off-ladder 8px radius). A page in the account menu (Profile, Settings, People) marks the
  account button; the part page keeps Parts marked.
- **Once the page scrolls**, the header gains its Scribe Line (opaque, never blurred). At the top
  it sits flush on the ground.
- **One standing control on the right**: Upload, on the grid. Library and Import are quiet. Two
  controls with one intent side by side read as a choice nobody can make. Pages past the grid carry
  their own: the dashboard's Add a widget, once the board has a widget (an empty board holds its
  own, so the name is never on screen twice). Settings, People, Profile, Tags, Shared with me and the
  part page have none in the header.
- **The places keep their width.** The current place is semibold and the others regular, so each
  label reserves its semibold width (`.place-label`); without it the search field beside them moved
  by a pixel or two from page to page.
- **Under `md`** it is one row: menu, mark, search (an icon that expands to the row), Upload as
  its icon alone, account. The places, the library switch and Import move into the drawer. Three header rows — 140px of a
  844px phone, measured in the audit — is the page before the parts.
- `<meta name="theme-color">` matches Anodised Black, so a phone's browser chrome is part of the
  room.

### Menus

Native popovers (`popover="auto"`) anchored under their button where CSS anchor positioning
exists: 16rem, Machined Slate, Scribe Line border, 10px radius, 12px padding, overlay shadow,
arriving by scaling from 0.98 and fading in over 120 ms from the button's corner. Escape,
click-outside and focus return come from the browser. The part page's ⋯ menu holds every
per-part action that is not Download.

### Dialogs

`web/src/components/Dialog.tsx`, used by Import, Share, the folder picker, delete and link
import. Machined Slate, Scribe Line border, 10px radius, 16px padding, the overlay shadow, over a
60% black scrim.

- **Arrival and departure are presence** (A22), replacing the `scrim-in` and `panel-in` keyframes:
  the scrim fades and the panel rises 8px from a 0.98 scale over 180 ms; leaving fades it over
  120 ms, and it stops being a dialog (no role, inert) the moment it starts to leave. Focus handling is unchanged: focus moves in, is trapped, and returns to the
  control that opened it.
- **Focus is unchanged by the redesign.** The audit saw the close button's Layout Blue ring in the
  Import dialog's first frame, but that dialog was opened by a scripted click, which Chrome's
  `:focus-visible` treats as no pointer at all. Opened by a real pointer (A22, and A25 again for
  Import, Share, the folder picker and Add a widget at 1440 and 390), focus lands on the close button
  with no ring; opened from the keyboard, the ring shows, as it should.
- **The title is Scope** (15px/600, Quicklime), the lead Ash and no wider than 70ch.
- **One standing control**, at the foot, right-aligned: the dialog's verb, standing only once it
  can act ("Add 3 selected", not "Add 0 selected").
- **No raw native widgets.** A file input is visually hidden behind a quiet button and a Graphite
  line saying what was chosen.
- **Under `sm`**, a dialog is a sheet from the bottom at full width, its scrim over the header too;
  `overscroll-behavior: contain` on its scroller, as on the drawer.

### Cards / Containers

- **Corner Style:** 10px (7px for a list row)
- **Background:** Machined Slate; the picture fills the square tile (The Filled Tile Rule)
- **Shadow Strategy:** none — see Elevation & Depth
- **Border:** 1px Scribe Line at rest, Edge under the pointer, plus a 1px lift over 180 ms
  (a list row brightens its edge with no lift). No glow, no blue, no scale. Selected adds a 2px
  Layout Blue outline.
- **Footer:** 12px padding, a fixed height so a row of cards lines up whatever the names; the
  name in Title, Archivo 600, Quicklime, clamped to two lines and broken at separators (one
  truncated line in the gallery overlay, so the name never covers the part); one mono figures
  line at 11px in Graphite: the format chip, part number in Ash, the triangle count, and the
  approximate mark with its dotted Edge underline whose title explains it. Present at rest,
  always; nothing about a part hides behind hover.
- **No picture:** the tile is the still Lattice on the lamp's pool (A21), and the format is the
  footer's chip to the eye and an `sr-only` word in the well to a reader, said once. Forty
  identical "STL" squares, as the audit found, said nothing.
- **Several pictures:** a "1 of 3" count and two round arrows on the tile, shown under the pointer,
  on keyboard focus, and always on a device that cannot hover.
- **Hovering a card loads no mesh** and draws no canvas. `Grid.test.tsx` asserts it.
- **Skeletons** are the card's own shape in Bench Grey, static, or shimmer once. `arrive()`
  replaces them.

### Chips

- **Style:** the active filter is a pill, Machined Slate ground, Chalk label at 12px, 1px Layout
  Blue edge (it is a live narrowing), 4px/12px padding, a 12px close glyph. A format chip is a
  4px-radius Bench Grey label in mono Graphite, uppercased in CSS, with no edge: it is a fact, not
  a control. It leads a card's figures line, and it sits at the stage's foot opposite View, over a
  picture as well as over the Lattice (A23), so the format is in the same place whatever the stage
  holds.
- **State:** a chip that narrows a result set says what it narrowed to and dismisses on click,
  widening the search. It lifts 1px on hover.

### Inputs / Fields

- **Style:** Bench Grey ground, 1px Edge, 7px radius, Chalk text at 13–14px, Graphite placeholder
- **Label above, hint below**, the hint at 12px in Graphite, never 11px (`HINT`, 60ch)
- **Read-only values are not inputs.** A value nobody can change here (the port, the models
  folder) is a fact (`FACT`): mono on the Bench Grey well, 7px radius, no Edge, because an Edge
  border promises you can type. The audit found the Port drawn as an input. A path nobody knows is
  a sentence in Archivo instead, not a fact.
- **Focus:** the border goes Layout Blue under the global ring; the caret is Layout Blue
- **Error:** the message sits below the field in Body, says what is wrong and what to do, and
  is announced — never colour alone

### Navigation

The category rail is a flat list: indentation and a disclosure caret carry depth. The selected
row is Chalk on Machined Slate; unselected rows are Graphite on the ground. A row that is a
valid drop target during a drag takes a 1px Layout Blue outline. Under `md` the rail slides in
from the left as a drawer over a scrim, on `translate`, with `overscroll-behavior: contain`.

### Upload and the drop overlay

Dragging files anywhere over the window raises a full-window overlay at z 40: the page ground at
95%, a 2px dashed Layout Blue boundary (live — something is being dragged), and one Quicklime
sentence on a Machined Slate plate. At rest there is no drop strip on the page. While uploading,
the header's Upload shows a mono count and a 2px Layout Blue progress line along its foot.

### Icons

Phosphor, regular weight, vendored as path data in `web/src/components/Icon.tsx`: 16px,
`currentColor`, always `aria-hidden`, always inside a control that carries its own text label.
Add a glyph by copying its path, never by installing the package.

### The stage and filmstrip

The part page and the quick look each have one stage: the lamp's pool (`stage-lamp`), 10px
radius, holding the part.

- **The whole picture, always.** `contain`, never `cover` (A16; The Filled Tile Rule). A square
  store picture on a wide stage has lamp on both sides, and that is correct.
- **No picture is still a stage.** The lamp and the format, with View on it. The audit found a
  600px black void under a full-width View bar, and that is the common case: most parts have no
  picture.
- **View** is an overlay control on the stage's foot, keeping its accessible name. Pressing it
  hands the browser the original mesh and three.js draws it on the same pool, with the contact
  shadow — the one place three.js draws.
- **The filmstrip** sits directly under the stage, not at the foot of the column: a row of
  64px tiles (filled, 7px radius, Scribe Line, Edge under the pointer, the current one with a
  2px Quicklime underline). Choosing one crossfades it onto the stage over 180 ms. A15's move
  controls and drag stay on the tiles, and the first tile says "On the card". The add-picture
  controls are the strip's last tile. When a part has no pictures, the strip is that one tile. With
  more tiles than fit, the strip scrolls sideways (`overscroll-x-contain`) rather than wrapping.
- **Card framing** (A23, owner only): because a click on a tile now chooses it, the focal point and
  fit a card crops the chosen picture to are set in one row under the strip, labelled "Card
  framing" (it read "On a card" beside the tile's "On the card" until A25).
- **The quick look** at 1280 and wider is a non-modal pane beside the grid; narrower it is a
  modal dialog (a bottom sheet under `sm`). It always has a stage, so View sits on it rather than
  alone on a row. A section with nothing in it (PICTURES with no pictures) is not rendered.
- **The flight.** The picture flies from the card into the stage, 280 ms (`flipFrom`). Under
  reduced motion it does not fly and the stage still opens.
- **The column beside it** is label-over-rule sections. An add-form (tag, material, source) opens
  from a quiet **Add** in its label row rather than standing open, so the column reads as the part
  and not as a form.

### Named Rules

**The Carried Picture Rule.** When a surface opens *about* something on screen, the thing
itself moves into it. A stage that merely appears makes a person look back at the grid to
check they opened what they meant to.

**The One Lit Place Rule** (replaces The Four Lit Places Rule). three.js draws in the viewer a
**view** press opens, on the part page's stage or the quick look's, and nowhere else. Every light
and material is defined once in `studio.ts`: a cool-neutral hemisphere fill over a
bench-coloured ground, one key light, a faint rim from behind, under sRGB output.

**The Front Door Rule.** The only authored motion, the only decorative drawing and the only
55/45 split are on the pages before sign-in. Inside the app, motion reports a change and
nothing more.

## Do's and Don'ts

### Do:

- **Do** keep Layout Blue (`#2cb4f5`) for live interaction and for the Lattice's draw head while
  it moves. A resting screenshot containing it anywhere but the brand mark is a defect.
- **Do** set every pasteable or comparable figure — path, hash, part number, dimension,
  count — in mono with tabular figures, selectable, never middle-elided.
- **Do** show a card's name, figures and approximate label at rest, and reach every per-part
  action from the part page, so nothing exists only behind a pointer.
- **Do** give a page one Headline (600, 1.5rem), a 70ch lead, label-over-rule sections and one
  standing control, from `Page.ts`.
- **Do** label every mesh-derived figure as approximate wherever it appears, including the stage,
  dialogs and tooltips. The triangle count is one: it carries the ≈ on the part page as on a card.
- **Do** show the whole picture on a stage, and fill the tile on a card.
- **Do** import anime.js only in `web/src/lib/motion.ts`, and `motion` only in
  `web/src/lib/motion-react.tsx` and `web/src/lib/motion-features.ts`; feature code uses their
  primitives.
- **Do** put `transition-none` on every element an engine moves.
- **Do** make every front-door moment skippable by a key, a click or Skip, and absent under
  reduced motion.
- **Do** use the z scale (20 / 30 / 40 / 50) for anything layering over the page; a local
  `z-10` inside a card or the grid (a select checkbox, the sticky selection bar) is not a layer.
- **Do** self-host Archivo and JetBrains Mono, `latin` + `latin-ext`.
- **Do** route every user-facing string through `web/src/lib/strings.ts`, and write `…`, never `...`.

### Don't:

- **Don't** drop a card's hairline; against a dark photograph it is the card's only edge.
- **Don't** crop a picture on a stage.
- **Don't** add a box-shadow to anything that is not a dialog, a menu or the floating
  selection bar.
- **Don't** introduce a fifth text grey, a second accent hue, or a fourth radius.
- **Don't** use a weight outside 400–700; the self-hosted Archivo file stops at 700. Headings
  and names are 600, labels 500, the wordmark alone 700.
- **Don't** fill a standing control with Layout Blue; Upload and Download stand in Quicklime.
- **Don't** put a stage's controls directly on the lamp; Edge does not clear 3:1 there, so they
  sit on a Machined Slate plate.
- **Don't** let colour alone carry a state. Every state has a second cue: a label, an
  outline, a position, an icon.
- **Don't** word an eviction so it reads as deletion, or a removal so it reads as a saving.
- **Don't** animate anything but `transform`, `translate`, `opacity` and SVG `stroke-dashoffset`,
  or past 180 ms for a state change; 280 ms is for the flight and a whole arrival run, and the
  scene and intro tokens are for the front door alone.
- **Don't** loop anything but the sign-in drawing (which has its Pause), bounce anything, or put a
  spring on anything but a layout change.
- **Don't** import `framer-motion`, or Motion+.
- **Don't** show anybody's data, a testimonial or a user count on a signed-out page.
