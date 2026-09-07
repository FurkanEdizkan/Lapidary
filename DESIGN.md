<!-- TARGET: this file records the visual world committed on 2026-09-08, not the code as it
     stands. The incumbent grid is the anti-reference. Where a token here differs from
     web/src/styles.css, this file is the intent and the code is the backlog. The
     "Incumbent gap" block in Overview names every difference. -->

---
name: Lapidary
description: A visual index for 3D part libraries, where the part is lit and the bench is not.
colors:
  anodised-black: "#0b0c0e"
  machined-slate: "#131519"
  scribe-line: "#24272d"
  chalk: "#e6e8ec"
  graphite: "#9aa1ac"
  layout-blue: "#6ea8fe"
typography:
  display:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "clamp(1.75rem, 3vw, 2rem)"
    fontWeight: 900
    lineHeight: 1.05
    letterSpacing: "-0.02em"
  headline:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "normal"
  body:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: "0.08em"
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: "normal"
rounded:
  sm: "4px"
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
    textColor: "{colors.chalk}"
    rounded: "{rounded.md}"
    padding: "0"
  card-part-hover:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.md}"
    padding: "0"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.graphite}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "4px 8px"
  button-quiet-hover:
    backgroundColor: "transparent"
    textColor: "{colors.chalk}"
    rounded: "{rounded.sm}"
    padding: "4px 8px"
  button-standing:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
  chip-filter:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.graphite}"
    typography: "{typography.body}"
    rounded: "{rounded.full}"
    padding: "6px 14px"
  chip-filter-selected:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.full}"
    padding: "6px 14px"
  input-text:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "6px 8px"
  dialog-surface:
    backgroundColor: "{colors.machined-slate}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.md}"
    padding: "16px"
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
text roughly fourteen to one and why the interface has no decorative colour at all. This is
a tool a person opens forty times a day for six seconds each time; it must be legible at a
glance and invisible in memory.

The one place the system permits weight is the display line. A library title or a section
heading may be genuinely heavy — 900, tight-tracked — because a wall of identical tiles needs
a horizon. Everything below that horizon stays recessive.

**Incumbent gap.** The code as of 2026-09-08 does not implement this world yet. It matches
the palette, the flatness, the motion grammar and the recessive controls. It differs on:
type (system stack at 16px, max weight 500 — no display voice at all), radius (a single 4px
step, no tile or pill), tiles (1px-bordered cards with four buttons and four metadata rows
visible at rest, where this world wants a borderless image tile that reveals on hover), and
accent role (three uses, drag-only). Closing that gap is a redesign of the grid, not a
polish pass.

**Key Characteristics:**

- Near-black ground; the render is the only lit thing on the page
- Two type voices only — a heavy display horizon and a quiet body
- One saturated colour, reserved for live interaction
- Flat by tonal layering; no shadow except beneath an overlay
- Image tiles borderless and edge-to-edge; chrome tiles hairlined
- Motion is mechanical: transform and opacity, 120/180 ms, always reduced-motion aware

## Colors

Six values, named for the machine shop the product serves. The greys are a single stepped
ramp from unlit ground to hairline; there is no second hue anywhere in the system.

### Primary

- **Layout Blue** (`#6ea8fe`): the machinist's marking dye, and the only saturated colour in
  the product. It marks what is *live right now* — a valid drop target under a dragged part,
  a focused control, a selected filter. It is never used for emphasis, never for a heading,
  never for a brand flourish, and never on a resting surface.

### Neutral

- **Anodised Black** (`#0b0c0e`): the page ground, and the inside of an image tile before its
  render loads. The unlit bench.
- **Machined Slate** (`#131519`): every raised surface — tiles, dialogs, inputs, the standing
  button. One step up from the ground, never two.
- **Scribe Line** (`#24272d`): hairline borders and dividers on *chrome*. A scribed mark, not
  a drawn box.
- **Chalk** (`#e6e8ec`): primary text. Reserved for the thing the user is reading right now —
  a part name, a dialog title, a value they asked for.
- **Graphite** (`#9aa1ac`): secondary text, which is most text. Labels, metadata, helper
  copy, resting controls.

### Named Rules

**The Marking Dye Rule.** Layout Blue appears only where something is happening — dragging,
focused, selected, loading. If a screenshot taken at rest contains any Layout Blue, that is a
defect. Its rarity is what makes it readable without a legend.

**The Two-Greys Rule.** Text is Chalk or Graphite and nothing between. A third text grey is
how a hierarchy becomes a gradient nobody can parse; when something needs to recede further,
it leaves the screen rather than fading.

**The One Step Up Rule.** A surface may sit exactly one tonal step above its parent
(ground → slate). Two stacked panels do not get two stacked lightnesses; they get a scribe
line between them.

## Typography

**Display / Body Font:** Inter (with `-apple-system, BlinkMacSystemFont, Segoe UI, Roboto,
sans-serif`)
**Label/Mono Font:** `ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`

**Character:** One family doing two jobs at opposite ends of its weight axis — a heavy,
tight-tracked display voice for the horizon and a plain 400 for everything else, with nothing
in between competing. Mono is not decoration: it marks machine identity, and a value set in
mono is a value the user can select, copy and paste somewhere that will accept it.

### Hierarchy

- **Display** (900, `clamp(1.75rem, 3vw, 2rem)`, 1.05, −0.02em): library name, empty-state
  headline, the one line per screen that establishes where you are. At most one per viewport.
- **Headline** (700, 1.25rem, 1.2, −0.01em): a part's own name on its detail page.
- **Title** (600, 0.875rem, 1.3): tile captions, dialog titles, the name of a thing in a list.
- **Body** (400, 0.875rem, 1.5): prose, helper text, refusals. Capped at 70ch.
- **Label** (500, 0.75rem, 1.2, 0.08em, uppercase): section headings and metadata keys. The
  uppercase tracking is what lets a 12px label read as structure rather than as small text.
- **Mono** (400, 0.75rem, 1.4): part numbers, BLAKE3 hashes, storage paths, kernel versions.

### Named Rules

**The Copyable Mono Rule.** Anything a user might paste into another program — a path, a
hash, a part number — is set in mono and is selectable. Mono is a promise that the string is
exact and complete, never elided with an ellipsis in the middle.

**The Self-Hosted Font Rule.** Lapidary ships into air-gapped deployments. Inter is
self-hosted from the bundle as a variable font, subset to Latin + Latin Extended-A so Turkish
renders without a second file. A `fonts.googleapis.com` link is a defect that only shows up
on the one network that matters most.

**The One Horizon Rule.** Exactly one Display line per viewport. A second one is not a
hierarchy, it is a competition.

## Layout

A fluid tile grid over a fixed chrome frame. The sidebar is a fixed 240px rail; the grid
takes the rest and reflows by whole tiles — `repeat(auto-fill, minmax(220px, 1fr))` at
comfortable density, `minmax(160px, 1fr)` at compact. Gutters are 12px; the grid never
introduces a border to separate tiles, because the gutter already does it.

Spacing runs 4 / 8 / 12 / 16 / 24 / 32. Inside a tile, padding is 12px; inside a dialog,
16px; between sections on a detail page, 24px.

Prose is capped at 70ch wherever it appears, including refusals and helper text, and
especially inside dialogs where a full-width paragraph would otherwise stretch to the panel.

Breakpoints are content-driven rather than device-driven: the rail collapses to a drawer
below 768px, the grid drops to two columns below 560px, and the detail page's two-column
header stacks below 640px. Density is a user preference, not a breakpoint.

## Elevation & Depth

**Flat by doctrine.** Depth is tonal layering and hairlines, never shadow: ground → surface →
scribe line, plus a 1px hover lift that reads as the object rising toward the lamp rather
than as it casting anything. A resting screen has zero box-shadows on it.

**One exception, and only one.** An overlay that floats above the page — a dialog, a popover,
a menu — may carry a shadow, because it must be unambiguously *above* rather than merely
lighter. It is paired with a scrim (`rgba(0,0,0,0.6)`), and the scrim does most of the work.

### Shadow Vocabulary

- **Overlay** (`box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6)`): dialogs, popovers and menus
  only. Never on a tile, a button, an input, or anything that lives in the page flow.

### Named Rules

**The Unlit Bench Rule.** Surfaces do not cast shadows, because nothing in this room is lit
from above — the light is on the parts, not on the furniture. The only shadow is the one an
overlay throws by being in front of the page rather than in it.

## Shapes

Three radii and no others. **4px** for chrome that must feel machined — buttons, inputs,
selects, hairlined panels. **10px** for image tiles, which are the softest thing on screen
because they hold the only organic content. **Full** for filter chips and segmented controls,
where the pill shape carries "this is a choice among a set" without needing a border.

Borders are hairlines at 1px in Scribe Line, and they belong to *chrome only*. An image tile
has no border at all: the render meets the ground directly and the gutter separates it. A box
drawn around a picture is a second frame competing with the first.

### Named Rules

**The Frameless Picture Rule.** Never put a border on a tile whose content is an image. The
image is the edge.

## Components

### Buttons

- **Shape:** machined corners (4px), hairline outline, no fill at rest
- **Quiet (default):** transparent ground, Graphite label at 12px, 4px/8px padding. The
  workhorse — every per-card and per-section action.
- **Standing:** Machined Slate fill, Chalk label at 14px, 6px/12px padding. Reserved for the
  one action a screen is actually about (Download, Save). At most one per region.
- **Hover / Focus:** label lifts Graphite → Chalk and the whole control rises 1px over 120ms.
  Focus shows a 2px Layout Blue ring offset 2px from the control — visible against both the
  ground and a raised surface, and never removed without a replacement.
- **Disabled:** 50% opacity, no lift, no pointer.

### Chips

- **Style:** pill (full radius), Machined Slate ground, Graphite label, no border
- **State:** selected raises the label to Chalk and adds a 1px Layout Blue outline. A chip
  that narrows a result set always says what it narrowed to and always carries a dismiss.

### Cards / Containers

- **Corner Style:** 10px for image tiles, 4px for chrome panels
- **Background:** Machined Slate; the image well inside is Anodised Black so a render with a
  transparent margin sinks into the ground rather than floating on a lighter square
- **Shadow Strategy:** none — see Elevation & Depth
- **Border:** none on image tiles; 1px Scribe Line on chrome panels
- **Internal Padding:** 12px below the image well; the image itself is edge-to-edge
- **Behaviour:** at rest a tile shows its render and its name. Metadata and per-part actions
  appear on hover and on keyboard focus — never on hover alone, or the keyboard loses them.

### Inputs / Fields

- **Style:** Machined Slate ground, 1px Scribe Line, 4px radius, Chalk text at 14px
- **Focus:** border goes Layout Blue and a 2px ring of the same appears outside it
- **Error:** the message sits below the field in Body, says what is wrong and what to do, and
  is announced — the field is never left to communicate by colour alone

### Navigation

The category rail is a flat list, not a tree with decorative connectors: indentation and a
disclosure caret carry depth. The selected row is Chalk on Machined Slate; unselected rows
are Graphite on the ground. A row that is a valid drop target during a drag takes a 1px
Layout Blue outline and a 1px lift.

### The Part Tile

The signature component and the reason the system exists. A square image well on Anodised
Black holding the render, a Title-weight name beneath it, and nothing else at rest. On hover
or focus the metadata row and the action row fade up over 120ms. It is deliberately *not* an
anchor — it holds a render button, a move button and a download link, and nesting those in an
`<a>` is invalid HTML — so the whole tile takes a click handler while the name stays a real
link for the keyboard, middle-click and screen readers.

## Do's and Don'ts

### Do:

- **Do** keep Layout Blue (`#6ea8fe`) for live interaction only. A resting screenshot
  containing it is a defect.
- **Do** set every pasteable string — path, hash, part number — in mono, selectable, never
  middle-elided.
- **Do** give every part tile its metadata on hover *and* on `:focus-visible`, so the
  keyboard reaches everything the mouse does.
- **Do** ship one Display line per viewport and let it be genuinely heavy (900, −0.02em).
- **Do** label every mesh-derived measurement as approximate wherever it appears, including
  inside dialogs and tooltips. `CLAUDE.md` makes this a product rule, not a style preference.
- **Do** self-host Inter, subset to Latin + Latin Extended-A, so Turkish and an air-gapped
  install both work.
- **Do** route every user-facing string through `web/src/lib/strings.ts`.

### Don't:

- **Don't** put a border on an image tile. The image is its own edge.
- **Don't** add a box-shadow to anything that is not an overlay above a scrim.
- **Don't** introduce a third text grey, a second accent hue, or a fourth radius.
- **Don't** use a font weight between 500 and 900 — the gap between the body voice and the
  display voice is the hierarchy.
- **Don't** let colour alone carry a state. Every state has a second cue: a label, an
  outline, a position, an icon.
- **Don't** word an eviction so it reads as deletion, or a removal so it reads as a saving.
  The deletion vocabulary is fixed in `CLAUDE.md` and is a design constraint.
- **Don't** animate anything but `transform`, `translate` and `opacity`, and never past
  180ms for a state change.
