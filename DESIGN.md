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
     corrected. Where a rule changed, the section says so. -->
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
    padding: "7%"
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
  measurement-callout:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.quicklime}"
    typography: "{typography.mono}"
    rounded: "{rounded.sm}"
    padding: "2px 6px"
---

# Design System: Lapidary

## Overview

**Creative North Star: "The Lit Bench"**

A machinist's bench under a single task lamp. The parts are lit; the bench is not. Every
surface in this interface exists to be the unlit surround for a rendered object — the ground
is near-black, the chrome is grey on grey, and the only saturated colour in the room appears
when you are doing something. The user's pinned reference is ArtStation's community browse
page, and what it gets right is exactly this: a dark room, a dense wall of work, and chrome
so quiet you stop seeing it after four seconds.

The register is quiet, precise and unhurried. Nothing raises its voice. Trust is earned by
being exact rather than by being emphatic, which is why secondary text outnumbers primary
text and why the interface has no decorative colour at all. This is a tool a person opens
forty times a day for six seconds each time; it must be legible at a glance and invisible in
memory.

Where the lamp actually falls is now built. A part is shown in one of four lit places — the
part page's studio viewer, the card's turntable under the pointer, the Space stage, and an
empty library's first-run bench — each drawn under the same light as the thumbnails, and
every one but the turntable (which stays inside its card's well) on the one non-flat ground
in the system, the lamp's pool, with the part set down on a soft contact shadow. Everything outside those places is
flat, tonal and silent.

**Key Characteristics:**

- Near-black ground; the part on its lamp-lit stage is the only lit thing on the page
- Archivo for every word, JetBrains Mono for every figure; headings share one weight (600) and part by size and colour
- One saturated colour, reserved for live interaction and for marks drawn over a part
- Flat by tonal layering; a shadow only under something that floats over the page
- Renders inset 7% in a Bench Grey well on a bordered card, lifting to Edge under the pointer
- Motion is mechanical: transform and opacity, 120/180/280 ms, one curve, always reduced-motion aware

## Colors

Named for the machine shop the product serves. The greys are a single stepped ramp from
unlit ground to hairline; the accent is the only hue, and the four semantics are the only
other saturated values in the system.

### Primary

- **Layout Blue** (`#2cb4f5`): the machinist's marking dye. It marks what is *live right
  now* — a valid drop target, the focus ring, a pressed measuring tool, a selected card's
  outline, upload progress, text selection and the caret — and the marks a measurement draws
  over a part. It is never used for emphasis, never for a heading, and never on a resting
  control: Upload and Download stand without it.

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
focused, selected, pressed, uploading. If a screenshot taken at rest contains Layout Blue
anywhere but the brand mark, that is a defect. Its rarity is what makes it readable without
a legend.

**The Marked Part Rule.** The image well is exempt from The Marking Dye Rule (owner decision,
2026-09-08), and the licence is now spent: a measurement callout is a Layout Blue line
between its picks with a mono figure label edged in Layout Blue, drawn in with `tween`. The
exemption covers marks *on the part* and nothing else in the well.

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

- **Display** (600, 1.875rem, 1.25, −0.025em): the first-run bench's heading, and only that.
- **Headline** (600, 1.5rem, 1.25, −0.025em): the one headline of a page past the grid — a
  part's name on its studio page, Removed parts, Shared libraries, the crash page.
- **Scope** (600, 15px, 1): the line above the grid naming what is on screen, and the title
  of an empty or no-result grid state.
- **Title** (600, 13–14px, 1.375): card names, clamped to two lines and broken at `-`, `_`
  and `.` rather than mid-word, in the detail layout; the gallery overlay truncates to one
  line, because a two-line name there covers the part.
- **Body** (400, 0.875rem, 1.5): prose, leads, helper text, refusals. Capped at 70ch.
- **Label** (500, 0.75rem, 0.05em, uppercase, Graphite): a page section's label over its
  scribe line, rail headings, metadata keys.
- **Wordmark** (700, 0.75rem, 1, 0.16em, uppercase): "Lapidary" in the header. Uppercased in
  CSS, never in the string, so a screen reader says the name rather than spelling it.
- **Figure** (JetBrains Mono 400, 10.5–12px, tabular): part numbers, triangle counts, byte
  sizes, dimensions, volumes, hashes, storage paths, a measurement callout's label.

**This hierarchy changed.** The previous version stated a 1.25rem/500 headline, a 900-weight
display horizon and a "no weight between 500 and 900" Don't. None of that was buildable: the
self-hosted Archivo file spans 400–700 only, so a 900 never rendered. The build settles on
600 for every heading and name, 500 for labels, 700 for the wordmark alone, and it separates
the levels by size and colour (Quicklime over Ash over Graphite), not by a weight gap.

### Named Rules

**The Copyable Mono Rule.** Anything a user might paste into another program or compare
against another value — a path, a hash, a part number, a measured dimension — is set in mono
and is selectable. Mono is a promise that the string is exact and complete, never elided in
the middle. Words are not figures: a sentence that happens to contain a number stays in Archivo.

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

**The part page is a studio.** At `lg` it is two zones: the stage on the left (sticky under the
header, up to 72vh) and a 22rem column on the right read top to bottom — name, Download and
the ⋯ menu, geometry, then what people recorded. Rarely needed matter (file, identity,
history) sits under both in two columns after a scribe line. Below `lg`, source order is
reading order: name and Download first, then the stage, then the rest.

**Page anatomy** (`web/src/components/Page.ts`), shared by every page past the grid: one
Headline, a lead no wider than 70ch in Ash, then sections that open on a Label over a
Scribe Line (40px above, 16px below) rather than living in bordered boxes, and one standing
control. Boxes are for things that float or that you pick up.

Spacing runs 4 / 8 / 12 / 16 / 24 / 32. Inside a card footer, 12px; inside a dialog or menu,
12–16px; between studio zones, 24–32px. Prose is capped at 70ch everywhere, dialogs included.

**Layers are four and no others:** header 20, drawer 30, overlay 40 (dialogs, the drop
overlay — a dialog opened from inside the drawer must not open behind it), skip link 50.
Native popover menus sit in the top layer and need no number.

## Elevation & Depth

**Flat by doctrine.** Depth on the page is tonal layering and hairlines, never shadow: ground
→ surface → scribe line, plus a 1px hover lift that reads as the object rising toward the lamp.
A card under the pointer brightens its edge and lifts; it casts nothing.

**One exception in the page: something floating over it.** A dialog, a menu, and the
selection bar pinned over the grid while you pick parts carry the overlay shadow, because
each must read as *above* the page rather than merely lighter. Dialogs pair it with a
`rgba(0,0,0,0.6)` scrim, which does most of the work.

**Inside a stage, light is real.** The lamp's radial pool and the part's contact shadow are
the scene's lighting, not interface elevation: a soft round falloff at 60% black under the
part's lowest point, 1.5× its footprint, so a part stands on the ground instead of floating
over it. The turntable fades it in with its step back, because the thumbnail it replaces has
none.

### Shadow Vocabulary

- **Overlay** (`box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6)`): dialogs, menus and the floating
  selection bar only. Never on a card, a button, an input or anything in the page flow.
- **Contact** (3D scene only, 60% black radial falloff): under a part on a stage. Never a CSS shadow.

### Named Rules

**The Unlit Bench Rule.** Surfaces do not cast shadows, because the light is on the parts, not
on the furniture. The only shadows are the one an overlay throws by being in front of the page,
and the one a part throws on the lamp's ground.

**The Lamp Rule.** The lamp's pool (Lamp at 50% 42% fading to Anodised Black at the box's
closest side) is the one gradient in the system and belongs only under a part: the studio
viewer, the Space stage, the first-run bench. `closest-side`, so no rectangle shows where it stops.

## Shapes

Three radii and a pill. **4px** for machined chrome — a callout label, the skip link, the
focus ring's corner. **7px** for things you operate — buttons, inputs, list rows, the drawer's
controls. **10px** for cards, stages, dialogs and menus, which hold or float over content.
**Full** for the active-filter chip (a Layout Blue edge: a live narrowing) and the scrollbar thumb.

Borders are 1px. Cards, menus and dialogs rest on a Scribe Line border and a card lifts to Edge
under the pointer; dividers keep the Scribe Line always; anything you can operate is bounded by
Edge. A card's border is not an ornament on the picture: the render is inset, so the border is
the only thing that gives the card an edge.

### Named Rules

**The Inset Picture Rule.** A render sits *inset* in its card — 7% on every side of a square
Bench Grey well, with a transparent background of its own — so a wall of parts reads as objects
on shelves rather than as a mosaic, and the card's hairline border gives it an edge. The
turntable canvas matches the same inset so the swap from picture to 3D is unseen.

Thumbnails are written with alpha rather than a baked-in background, so a palette change never
leaves every thumbnail carrying a square that no longer matches the well under it.

**The Operable Edge Rule.** If a person can click, type into or drop onto it, its boundary is
Edge, not Scribe Line. A divider gets the hairline.

## Components

### Motion

One curve (`cubic-bezier(0.2, 0, 0, 1)`), three durations (120 / 180 / 280 ms), transform,
translate and opacity only. CSS moves every state change; `web/src/lib/motion.ts` is the only
importer of anime.js and the only place motion is sequenced in script. It reads the durations
and curve from `styles.css`, so a token edit reaches script too.

- **arrive()** — newly added content (a first page, the next page, a new filter's results):
  opacity plus a 4px rise at 120 ms. The first 12 elements take turns, step
  `min(24 ms, (280 − 120) / (n − 1))`, so the whole run lands inside 280 ms; anything after the
  twelfth lands with it. Reverted on completion so no inline style outranks a hover lift. Never
  for a refetch of what is already on screen.
- **tween()** — numbers on a three.js object (a callout's draw-in, a light), at 180 ms by default.
- **curve()** — the same curve as a function, for a script drawing its own frames.
- Under reduced motion, script moves jump to their end state, CSS transitions drop to 0.01ms,
  and the turntable, flight and bench do not move at all.

### Buttons

- **Standing:** Anodised Black fill, 1px Edge, 7px radius, Quicklime label at 14px/600, 8px/16px
  in the page anatomy (`Page.ts`), 6px/12px beside the part page's ⋯ menu.
  The one action a region is about — Download on the part page, Upload in the header (12px,
  icon plus label, the label hidden under `sm`) and on the first-run bench. Never blue at rest;
  it is brighter and heavier than the quiet controls beside it, and that is enough.
- **Quiet:** transparent, 1px Edge, 7px radius, Graphite label at 12px, 24px tall. Every
  secondary action: Select, View, New category, measuring tools. Pressed takes a Layout Blue
  edge and a Quicklime label.
- **Hover / Focus:** label lifts Graphite → Chalk, and a standing control rises 1px, over 120 ms.
  Focus is one ring for the whole app: 2px Layout Blue, offset 2px.
- **Disabled:** 50–60% opacity, no lift.

### Header (AppFrame)

The one header, sticky, Machined Slate over a Scribe Line: the mark and wordmark, the places
switch (Parts, Removed parts, Shared libraries — a Bench Grey tray, 3px padding, with the
current place raised; its 8px corner is an off-ladder one-off, not a fourth radius), search, the Library menu and Upload. The brand mark is a 19px Layout Blue diamond,
the one resting use of the accent. Under `md` a menu button opens the drawer and search wraps
to its own row. A skip link sits above everything at z 50.

### Menus

Native popovers (`popover="auto"`) anchored under their button where CSS anchor positioning
exists: 16rem, Machined Slate, Scribe Line border, 10px radius, 12px padding, overlay shadow,
arriving with the panel's 4px rise. Escape, click-outside and focus return come from the browser.
The part page's ⋯ menu holds every per-part action that is not Download.

### Cards / Containers

- **Corner Style:** 10px (7px for a list row)
- **Background:** Machined Slate; the image well inside is Bench Grey, square, render inset 7%
- **Shadow Strategy:** none — see Elevation & Depth
- **Border:** 1px Scribe Line at rest, Edge under the pointer, plus a 1px lift over 180 ms
  (a list row brightens its edge with no lift). Selected adds a 2px Layout Blue outline.
- **Footer:** 12px padding; the name in Title, Quicklime, clamped to two lines and broken at
  separators (one truncated line in the gallery overlay, so the name never covers the part); one mono figures line at 11px in Graphite — part number in Ash, a `·`, the
  triangle count, and a lowercase "approximate" with a dotted Edge underline whose title
  explains it. Present at rest, always; nothing about a part hides behind hover.
- **Turntable:** a pointer resting 150 ms on a card (hover-capable screens, WebGL, no reduced
  motion) swaps the thumbnail for a canvas drawn under the thumbnail's own light and framing.
  It holds still through the 180 ms crossfade, steps back over 280 ms to a framing that holds
  the part at every angle, then turns once every 8 s.

### Chips

- **Style:** one chip ships, the active filter: a pill, Machined Slate ground, Chalk label at
  12px, 1px Layout Blue edge (it is a live narrowing), 4px/12px padding, a 12px close glyph.
- **State:** a chip that narrows a result set says what it narrowed to and dismisses on click,
  widening the search. It lifts 1px on hover.

### Inputs / Fields

- **Style:** Bench Grey ground, 1px Edge, 7px radius, Chalk text at 13–14px, Graphite placeholder
- **Focus:** the border goes Layout Blue under the global ring; the caret is Layout Blue
- **Error:** the message sits below the field in Body, says what is wrong and what to do, and
  is announced — never colour alone

### Navigation

The category rail is a flat list: indentation and a disclosure caret carry depth. The selected
row is Chalk on Machined Slate; unselected rows are Graphite on the ground. A row that is a
valid drop target during a drag takes a 1px Layout Blue outline. Under `md` the rail slides in
from the left as a drawer over a scrim, on `translate`.

### Upload and the drop overlay

Dragging files anywhere over the window raises a full-window overlay at z 40: the page ground at
95%, a 2px dashed Layout Blue boundary (live — something is being dragged), and one Quicklime
sentence on a Machined Slate plate. At rest there is no drop strip on the page. While uploading,
the header's Upload shows a mono count and a 2px Layout Blue progress line along its foot.

### Icons

Phosphor, regular weight, vendored as path data in `web/src/components/Icon.tsx`: 16px,
`currentColor`, always `aria-hidden`, always inside a control that carries its own text label.
Add a glyph by copying its path, never by installing the package.

### The Stage (Space)

Space on a card's name puts the part on a stage in a large dialog: the render on the lamp's
pool, its name and mono figures beside it, arrows walking to the next and previous part.
The render flies in from the card — the system's one authored flight.

**The flight.** The scrim fades at 120 ms and the box rises 4px at 180 ms. The part's render
then travels from the rectangle the card occupied to the one the stage gives it, at 280 ms on
the mechanical curve: a single `transform`, measured at the click rather than at mount. Under
reduced motion the flight does not happen and the stage still opens.

### The Studio (part page)

The part on a lamp-lit stage (three.js, studio lights, flat-shaded pale-grey material, contact
shadow) with a Machined Slate dock of measuring and section tools under it; the name as
Headline, the part number in mono Ash, Download standing beside a quiet ⋯ menu; then Geometry
as label-over-rule with every figure in mono and every mesh-derived one marked approximate.
A measurement draws a Layout Blue line between its picks and a mono label edged in Layout
Blue, drawn in with `tween`.

### The First-Run Bench

An empty library is a Display heading, one Ash sentence, a standing Upload and a drop hint,
beside a 16:10 lamp stage holding three real example parts (spur gear, flange, vee block)
lit as the grid will light them. The key light sweeps ±25° over 12 s. The scene is
`aria-hidden` and absent without WebGL; the words are the page.

### Named Rules

**The Carried Picture Rule.** When a surface opens *about* something on screen, the thing
itself moves into it. A stage that merely appears makes a person look back at the grid to
check they opened what they meant to.

**The Four Lit Places Rule.** three.js draws in the studio viewer, the turntable, the Space
stage and the first-run bench, and nowhere else. Every light and material is defined once in
`studio.ts`, in two pairs: the turntable and the bench use the raster pair, which matches
`raster.rs`'s thumbnail shading to the byte, so the crossfade from picture to model is unseen;
the studio viewer keeps the studio pair, because its marks, ghost and section cap are colours
chosen in sRGB and a linear output would shift them.

## Do's and Don'ts

### Do:

- **Do** keep Layout Blue (`#2cb4f5`) for live interaction and for marks drawn on a part. A
  resting screenshot containing it anywhere but the brand mark is a defect.
- **Do** set every pasteable or comparable figure — path, hash, part number, dimension,
  count — in mono with tabular figures, selectable, never middle-elided.
- **Do** show a card's name, figures and approximate label at rest, and reach every per-part
  action from the part page, so nothing exists only behind a pointer.
- **Do** give a page one Headline (600, 1.5rem), a 70ch lead, label-over-rule sections and one
  standing control, from `Page.ts`.
- **Do** label every mesh-derived measurement as approximate wherever it appears, including
  the stage, the studio, dialogs and tooltips.
- **Do** put a part on the lamp's ground with its contact shadow whenever three.js draws it.
- **Do** sequence script motion only through `web/src/lib/motion.ts`, inside 280 ms end to end.
- **Do** use the z scale (20 / 30 / 40 / 50) for anything layering over the page; a local
  `z-10` inside a card or the grid (a select checkbox, the sticky selection bar) is not a layer.
- **Do** self-host Archivo and JetBrains Mono, `latin` + `latin-ext`.
- **Do** route every user-facing string through `web/src/lib/strings.ts`.

### Don't:

- **Don't** set a card's render edge to edge or drop the card's hairline; the render is inset
  and the border is the card's only edge.
- **Don't** add a box-shadow to anything that is not a dialog, a menu or the floating
  selection bar.
- **Don't** introduce a fifth text grey, a second accent hue, or a fourth radius.
- **Don't** use a weight outside 400–700; the self-hosted Archivo file stops at 700. Headings
  and names are 600, labels 500, the wordmark alone 700.
- **Don't** fill a standing control with Layout Blue; Upload and Download stand in Quicklime.
- **Don't** put a stage's controls directly on the lamp; Edge does not clear 3:1 there, so they
  sit on a Machined Slate dock.
- **Don't** let colour alone carry a state. Every state has a second cue: a label, an
  outline, a position, an icon.
- **Don't** word an eviction so it reads as deletion, or a removal so it reads as a saving.
- **Don't** animate anything but `transform`, `translate` and `opacity`, or past 180 ms for a
  state change; 280 ms is for the flight, the turntable's step back and a whole arrival run.
