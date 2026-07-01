---
name: Lapidary
description: A fast, dark-themed gallery and library manager for 3D-printable models.
colors:
  page: "#121214"
  nav: "#0d0d0e"
  surface: "#1a1a1d"
  surface-2: "#1d1d20"
  surface-3: "#18181b"
  surface-input: "#212125"
  border: "#26262a"
  border-2: "#2c2c30"
  border-3: "#2e2e34"
  border-4: "#34343a"
  border-hover: "#4a4a52"
  text: "#eaeaec"
  text-dim: "#c9c9cf"
  text-mute: "#9b9ba1"
  text-mute-2: "#8e8e96"
  text-faint: "#898991"
  accent: "#2cb4f5"
  accent-hover: "#4cc3ff"
  on-accent: "#06151d"
  danger: "#e07a7a"
  success: "#6fd394"
typography:
  display:
    fontFamily: "Archivo, sans-serif"
    fontSize: "19px"
    fontWeight: 750
    lineHeight: 1.2
    letterSpacing: "normal"
  title:
    fontFamily: "Archivo, sans-serif"
    fontSize: "14px"
    fontWeight: 650
    lineHeight: 1.25
    letterSpacing: "normal"
  body:
    fontFamily: "Archivo, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1.5
    letterSpacing: "normal"
  label:
    fontFamily: "Archivo, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "normal"
  mono:
    fontFamily: "JetBrains Mono, monospace"
    fontSize: "11.5px"
    fontWeight: 500
    lineHeight: 1.5
    letterSpacing: "0.02em"
  kicker:
    fontFamily: "JetBrains Mono, monospace"
    fontSize: "9.5px"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0.2em"
rounded:
  xs: "5px"
  sm: "8px"
  md: "9px"
  lg: "12px"
  xl: "16px"
  pill: "99px"
spacing:
  xs: "6px"
  sm: "10px"
  md: "14px"
  lg: "18px"
  xl: "28px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "9px 20px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
    textColor: "{colors.on-accent}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text-dim}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "9px 18px"
  chip-active:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.pill}"
    padding: "5px 12px"
  chip:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text-dim}"
    rounded: "{rounded.pill}"
    padding: "5px 12px"
  input:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "9px 11px"
  tile:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.sm}"
  modal:
    backgroundColor: "{colors.surface-3}"
    rounded: "{rounded.xl}"
    padding: "26px 28px"
---

# Design System: Lapidary

## 1. Overview

**Creative North Star: "The Lapidary's Loupe"**

Lapidary is a dark velvet field with one bright facet of light. Surfaces recede into a near-black
graphite (`#121214`); a single cyan accent (`#2cb4f5`) is the one cut that catches the light. The
name is the brief: a lapidary cuts and polishes gems, and this interface treats each model as the
gem under inspection — the chrome is the loupe, precise and almost invisible, and the model is the
thing in focus. Every decision serves that act of looking closely.

The personality is **Precise · Fast · Trustworthy**: instrument-like, confident, dense but always
legible. Information density is high — spec tables, mono-set data, pill filters — yet nothing
shouts. Restraint is the discipline. Archivo carries the structure; JetBrains Mono carries the
data, so a dimension or a triangle count reads as a *measurement*, not prose. The accent is rationed:
it marks the live thing (selection, primary action, the focused facet) and nothing else.

This system explicitly rejects four neighbors. It is **not consumer marketplace clutter** (no
ad-heavy, infinite-scroll, social-noise storefront). It is **not heavy enterprise CAD/PLM** (no
gray nested toolbars, no industrial bleakness). It is **not a generic SaaS dashboard** (no purple
gradients, no identical card grids, no hero-metric template, no tracked-uppercase eyebrow on every
section). And it is **not playful / childish maker vibes** (no cartoonish rounding, no emoji chrome).
Precision without coldness; density without noise.

**Key Characteristics:**
- Near-black tonal layering; depth from subtle borders, not heavy shadow.
- A single rationed cyan accent — the facet, never decoration.
- Dual-typeface system: Archivo for structure, JetBrains Mono for every measured value.
- Image-first: the model fills the frame; metadata reveals on intent.
- High density held calm by a tight type scale and generous, rhythmic spacing.

## 2. Colors

A near-monochrome graphite field, layered tonally, with one saturated cyan facet and two reserved
semantic signals.

### Primary
- **Loupe Cyan** (`#2cb4f5`): the one bright facet. Reserved for the live thing — primary action
  buttons, the active filter/chip, current selection, the brand mark, and focus. **Hover Cyan**
  (`#4cc3ff`) lifts it on interaction; **On-Facet Ink** (`#06151d`) is the near-black text that
  rides *on* the accent so cyan fills never carry low-contrast white.

### Neutral
- **Velvet Page** (`#121214`): the body field everything floats on; the darkroom black.
- **Nav Black** (`#0d0d0e`): the darkest chrome — the top bar and deepest wells.
- **Graphite Surfaces** (`#1a1a1d` / `#1d1d20` / `#18181b`): the tonal layer stack for tiles,
  panels, and modals. They read apart by a half-step of lightness plus a hairline border, not by shadow.
- **Input Well** (`#212125`): the slightly-raised field surface for inputs and icon buttons.
- **Border Ramp** (`#26262a` → `#2c2c30` → `#2e2e34` → `#34343a`, hover `#4a4a52`): hairline
  separators doing the structural work shadows would do elsewhere. Heavier borders mark more
  interactive edges; `border-hover` is the lift.
- **Ink Ramp** (`#eaeaec` primary → `#c9c9cf` dim → `#9b9ba1` mute → `#8e8e96` → `#898991` faint):
  the text scale. Primary ink for content, the muted steps for labels and secondary data. The faint
  step (`#898991`) is the placeholder/kicker floor, tuned to hold WCAG AA 4.5:1 on every graphite
  surface (down to the raised input well `#212125`) — never *darker*. (Earlier `#6c6c73`/`#75757d`
  faint steps read below 4.5:1 and were merged up into this single AA-compliant floor.)

### Tertiary (Semantic)
- **Signal Coral** (`#e07a7a`): danger / destructive only.
- **Signal Sage** (`#6fd394`): success / confirmation only.

### Named Rules
**The One Facet Rule.** Loupe Cyan appears on a small fraction of any screen — the primary action,
the current selection, the focus ring. Its rarity is the entire point; the moment a second element
competes for it, the facet stops reading as the focus. Never use cyan for decoration, borders-at-rest,
or body text.

**The Tonal Depth Rule.** Surfaces separate by lightness and a hairline border, not by drop shadow.
If two panels are hard to tell apart, raise the border one step on the ramp — do not add a shadow.

## 3. Typography

**Structure Font:** Archivo (with `sans-serif` fallback)
**Data / Mono Font:** JetBrains Mono (with `monospace` fallback)

**Character:** A two-voice system, paired on a true contrast axis (grotesque sans vs. monospace), never
two similar sans. Archivo is the confident, slightly condensed structural voice for names, titles, and
controls. JetBrains Mono is the instrument readout: every dimension, file size, triangle count, tag,
and print setting is set in mono so measured values look measured. The contrast itself is the system.

### Hierarchy
- **Display** (Archivo 750, 19px, line-height 1.2): modal and section titles. This is the ceiling —
  product UI has no hero; the largest type is a dialog heading, not a billboard.
- **Title** (Archivo 650, 14px, line-height 1.25): model names on tiles and rows; the loudest thing
  in a dense grid.
- **Body** (Archivo 500, 12.5–13px, line-height 1.4–1.5): the workhorse — controls, copy, secondary
  metadata. Prose blocks cap at 65–75ch; dense tables and rows may run tighter.
- **Label** (Archivo 600, 11px): field labels and inline control text; muted ink, never the accent.
- **Mono Data** (JetBrains Mono 500, 11–11.5px, letter-spacing 0.02em): all measured values, tags,
  file badges, and settings. The signal that "this is a number you can act on."
- **Kicker** (JetBrains Mono 500, 9.5px, uppercase, letter-spacing 0.2em): the lone spec micro-label
  (e.g. `SPECS`). Used as a deliberate, sparse system mark — *not* as an eyebrow above every section.

### Named Rules
**The Measured-Value Rule.** Anything that is a measurement — size, triangle count, file size, print
setting, date — is set in JetBrains Mono. Anything that is a name or a sentence is set in Archivo.
The reader should be able to tell data from prose without reading a word.

**The No-Eyebrow Rule.** The mono kicker is a sparse system accent, not section scaffolding. Never
stack a tracked-uppercase label above every section; that is the SaaS/AI grammar this brand rejects.

## 4. Elevation

Flat by default, lifted only as a response to state. Lapidary builds depth almost entirely from
**tonal layering plus hairline borders** — the page is darkest, surfaces step up a half-tone, fields
step up again, each separated by a border off the ramp. Shadow is rare and reserved: it appears only
when an element genuinely leaves the plane (a tile lifting on hover) or floats above everything (a
modal over a blurred scrim). At rest, surfaces are flat.

### Shadow Vocabulary
- **Lift** (`box-shadow: 0 14px 30px rgba(0,0,0,0.5)`): a grid tile on hover, paired with a 2px
  upward translate and a brightened border. The only resting-element shadow in the system.
- **Float** (`box-shadow: 0 44px 90px rgba(0,0,0,0.65)`): modal dialogs above a `blur(7px)`,
  74%-opacity scrim. Deep and soft, so the dialog reads as clearly above the field.

### Named Rules
**The Flat-At-Rest Rule.** Surfaces cast no shadow until they change state. A shadow on a static panel
is a bug. Depth at rest is tone and border; depth on interaction is the Lift; depth above the app is
the Float. Nothing else casts.

## 5. Components

### Buttons
- **Shape:** softly squared (9px radius); never pill-shaped except chips, never sharp-cornered.
- **Primary:** Loupe Cyan fill (`#2cb4f5`) with On-Facet Ink text (`#06151d`), 9px radius, ~9px 20px
  padding, weight 700. The single highest-emphasis action on a surface (`+ Add model`, `Add to library`).
- **Ghost / Secondary:** transparent fill, `border-4` (`#34343a`) hairline, dim ink, 9px radius. The
  default for non-primary actions (`Cancel`, `Tags ⁄ Groups`).
- **Hover / Focus:** cyan primary lifts to `#4cc3ff`; ghost buttons shift their border to Loupe Cyan
  (`.hover-cyan`). Focus must show a visible cyan ring (see Do's) — not rely on color shift alone.
- **Segmented control** (Grid/Cards/List): a `surface-2` track with `border-2`, 9px outer radius, 3px
  inset; the active segment fills `#2e2e34` with bright ink, inactive segments stay transparent/muted.

### Chips
- **Style:** pill (99px), three sizes (tag 11px mono, bar 12px, rail 12.5px). Inactive chips use a
  graphite surface (`surface-2` / `surface-input`) with dim ink and a ramp border.
- **State (selected):** the chip fills Loupe Cyan with On-Facet Ink — the same facet logic as buttons.
  Selection must also be legible without color (it is the filled, higher-contrast chip), satisfying the
  colorblind-safe rule.

### Cards / Tiles
- **Corner Style:** 8px radius, fully clipped (`overflow: hidden`) so the image meets the corner.
- **Background:** `surface` (`#1a1a1d`) behind a radial-gradient placeholder tinted from the model's
  own color; the thumbnail sits on top. Image-first — the tile is mostly picture.
- **Shadow Strategy:** flat at rest; the **Lift** shadow + 2px translate + brightened border on hover only.
- **Reveal:** a bottom-up dark scrim fades in on hover (160ms) to surface name, creator, format, and
  size. The default tile is already complete; the reveal enriches, it doesn't gate content.
- **Internal Padding:** 14px within the overlay; metadata badges use mono at 9px.

### Inputs / Fields
- **Style:** `surface-2` well, `border-2` hairline, 8px radius, primary ink, 13px. Mono variant for
  numeric, tag, and settings fields. The global search is the one pill-shaped input (99px).
- **Placeholder:** `text-faint` (`#898991`) — held at the AA-compliant readable floor (≥ 4.5:1 on
  every surface), never darker.
- **Focus:** must render a visible Loupe Cyan ring (see Do's). The drop-zone uses a dashed `border-4`
  that shifts to cyan on hover.
- **Disabled:** reduced opacity (~0.5) with the control left in place; never removed.

### Navigation
- **Top bar:** 58px, `nav` black (`#0d0d0e`), bottom hairline border. Left: the diamond brand mark
  (a 45°-rotated cyan-outlined square with a solid cyan core) beside the `LAPIDARY` wordmark (Archivo
  800, letter-spacing 0.14em). Center: the pill search with a leading glyph. Right: the segmented view
  control, a ghost button, and the primary action — a consistent left-to-right emphasis ramp.

### Modal
- **Scrim:** fixed full-bleed `rgba(6,6,8,0.74)` with `backdrop-filter: blur(7px)` — the one
  deliberate, purposeful use of blur in the system (focus the dialog, dim the field). Not decorative glass.
- **Box:** `surface-3` panel, `border-3` hairline, 16px radius, 26–28px padding, the **Float** shadow,
  `max-height: 92vh` with internal scroll. Header is a 19px title with a square icon-button close.

## 6. Do's and Don'ts

### Do:
- **Do** ration Loupe Cyan to the live thing only — primary action, current selection, focus. Honor
  **The One Facet Rule**.
- **Do** set every measured value (size, triangles, file size, dates, print settings) in JetBrains
  Mono, and every name or sentence in Archivo. Honor **The Measured-Value Rule**.
- **Do** build depth from tone + hairline borders; add a shadow only on hover-lift or for modals.
  Honor **The Flat-At-Rest Rule**.
- **Do** render a visible Loupe Cyan focus ring on every interactive control (the current code sets
  `outline: none` on inputs with no replacement — that is an accessibility gap to close, not a pattern
  to copy). Keyboard users must always see where they are.
- **Do** carry a second, non-color cue for every state — selection is the *filled* chip, error pairs
  Signal Coral with an icon/label — so the UI is legible to colorblind users (WCAG 2.1 AA).
- **Do** give every transition a `prefers-reduced-motion: reduce` alternative (crossfade or instant);
  no information is gated behind motion.
- **Do** keep body and placeholder text at ≥ 4.5:1 against its graphite surface; bump toward the ink
  end of the ramp before going lighter.

### Don't:
- **Don't** ship **consumer marketplace clutter** — no ad rails, infinite-scroll noise, or social cruft
  around the gallery. It is a personal library, not a storefront.
- **Don't** drift into **heavy enterprise CAD/PLM** — no gray nested toolbars, no industrial-bleak
  panel-in-panel density.
- **Don't** apply the **generic SaaS dashboard** look — no purple gradients, no identical card grids,
  no hero-metric template, no tracked-uppercase eyebrow above every section (honor **The No-Eyebrow Rule**).
- **Don't** slip into **playful / childish maker vibes** — no cartoonish rounding, no emoji chrome,
  no toy-like styling that undercuts the pro-tool credibility.
- **Don't** use `background-clip: text` gradient text, decorative glassmorphism (the modal blur is the
  one sanctioned use), or a colored side-stripe (`border-left` > 1px) as an accent. These are banned outright.
- **Don't** let Loupe Cyan become a second neutral. If more than a small fraction of a screen is cyan,
  the facet is gone.
