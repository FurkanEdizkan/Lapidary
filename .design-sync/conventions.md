# Building with Lapidary

Lapidary is a visual index for 3D part libraries: a person scans a wall of part renders, decides
whether one fits, and hands it to the right tool. The design's name is **The Lit Bench** — a
machinist's bench under one task lamp. The parts are lit; the bench is not. Every surface is the
unlit surround for a rendered object: near-black ground, grey-on-grey chrome, and the only
saturated colour in the room appears when something is happening.

The register is quiet, precise and unhurried. Secondary text outnumbers primary text, there is no
decorative colour at all, and nothing raises its voice.

## Setup

Importing `styles.css` is the whole of the styling setup — there is no theme object and no theme
provider. **Dark only**: `:root` already sets `color-scheme: dark`, the ground colour, the text
colour and Archivo. Never paint a light background; never add a theme switch. A host page's own
`body` background can cover `:root`, so give a screen's outermost element
`bg-[var(--color-bg)] text-[var(--color-text)]` and the ground holds wherever it renders.

Most components need nothing around them. Three need a router, and the four data components need
Lapidary's API — wrap those in **`DesignProviders`** (a bundle export):

```jsx
<DesignProviders>
  <AppFrame current="parts">…</AppFrame>
</DesignProviders>
```

A data component inside it shows its own loading state, since a design has no server. To draw it
with data, pass `seed` — query keys and the answers to find there:

```jsx
<DesignProviders seed={[[['folders', libraryId], [{ id, parentId: null, name: 'Brackets', slug: 'Brackets', partCount: 14 }]]]}>
  <FolderTree library={libraryId} selected={null} onSelect={() => {}} />
</DesignProviders>
```

| Needs | Components |
|---|---|
| nothing | `Icon`, `Menu`, `Dialog`, `Grid`, `GridSkeleton`, `SelectionBar`, `Figure`, `MeasureBar`, `SectionBar`, `FirstRun` |
| a router (`DesignProviders`) | `AppFrame`, `Card`, `Crash` — and `Grid`, whose cards are `Card`s |
| the API (`DesignProviders`, `seed` for data) | `FolderTree` (`['folders', library]`), `MovePartDialog`, `Detail`, `ShareDialog` |

## The styling idiom: arbitrary-value Tailwind over CSS variables

A Tailwind 4 system whose decisions live in custom properties, so colours are **arbitrary-value
utilities wrapping a token**: `bg-[var(--color-surface)]` is the idiom; `bg-surface` does not exist.

### Grounds — four, one step apart

| Token | Name | Value | Use |
|---|---|---|---|
| `--color-bg` | Anodised Black | `#121214` | the page ground, the drawer, a standing control's fill |
| `--color-raised` | Bench Grey | `#17171b` | the well a thing sits *in*: a card's image well, an input, the search field |
| `--color-surface` | Machined Slate | `#1a1a1d` | every raised surface: cards, dialogs, menus, the header, the selection bar |
| `--color-lamp` | Lamp | `#202024` | the centre of the lamp's pool under a part on a stage — the one non-flat ground |

**One step up**: a surface sits exactly one tonal step above its parent. Two stacked panels get a
scribe line between them, never two stacked lightnesses.

### Text — four greys and no fifth

| Token | Name | Value | Use |
|---|---|---|---|
| `--color-bright` | Quicklime | `#f0f0f2` | headings, names, the one value a row exists to show |
| `--color-text` | Chalk | `#e6e6e9` | primary text |
| `--color-dim` | Ash | `#c8c8ce` | body copy that is not a label: leads, notes, a part number beside its figures |
| `--color-muted` | Graphite | `#8a8a92` | labels, metadata, resting controls — most text |

Never a grey between these. Something that must recede further leaves the screen.

### Lines

| Token | Name | Value | Use |
|---|---|---|---|
| `--color-border` | Scribe Line | `#26262b` | dividers, the rule under a section label, the resting border of a card, menu or dialog |
| `--color-edge` | Edge | `#65656d` | the boundary of **anything you can operate** — input, button, a card under the pointer. 3:1 by WCAG SC 1.4.11. Never use Scribe Line for a control's edge. |

### Colour

| Token | Name | Value | Use |
|---|---|---|---|
| `--color-accent` | Layout Blue | `#2cb4f5` | **only what is live right now**: focus, a valid drop target, a pressed tool, a selected card's outline, progress, a measurement's marks on a part |
| `--color-warn` / `--color-good` / `--color-info` / `--color-bad` | Brass / Patina / Verdigris / Oxide | `#e8b06a` / `#4f9e94` / `#8fd7d0` / `#e88a8a` | a caution, a settled good, a note, a failure — each clears 4.5:1 on a card, so it may carry text |

**The Marking Dye Rule.** A screenshot at rest contains Layout Blue nowhere but the brand mark.
Never on a heading, never for emphasis, never filling a button — Upload and Download stand in
Quicklime. A state is never carried by colour alone: every one also has a label, an outline, a
position or an icon.

### Shape, depth, layers, motion

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | `4px` | machined chrome: a callout label, the skip link |
| `--radius-ctl` | `7px` | things you operate: buttons, inputs, list rows |
| `--radius-md` | `10px` | cards, stages, dialogs, menus |
| `--shadow-overlay` | `0 16px 48px rgba(0,0,0,.6)` | dialogs, menus and the floating selection bar — **nothing else** casts a shadow |
| `--z-header` / `--z-drawer` / `--z-overlay` / `--z-skip` | 20 / 30 / 40 / 50 | the only layers |
| `--duration-fast` / `--duration-base` / `--duration-slow` | 120 / 180 / 280 ms | hover; a state change; the flight, the turntable's step back, a whole arrival |
| `--ease-mechanical` | `cubic-bezier(0.2, 0, 0, 1)` | every transition (the `ease-mechanical` utility) |

Flat by doctrine: depth is tonal layering and hairlines, plus a 1px hover lift
(`hover:-translate-y-px`) that reads as the object rising toward the lamp. Transitions animate
`transform`, `translate` and `opacity` only. Reduced motion is handled globally.

Focus is global: a 2px Layout Blue ring at 2px offset. Never remove an outline without replacing it.

### Type

**Archivo** carries every word; **JetBrains Mono** carries every figure and identifier — part
numbers, counts, sizes, dimensions, hashes, paths — selectable and never middle-elided. The
`tabular` class sets both the mono face and tabular digits: put it on every figure. Headings and
names are weight 600, labels 500, the wordmark alone 700; nothing outside 400–700.

| Level | Style |
|---|---|
| Headline | `text-2xl leading-tight font-semibold tracking-tight text-[var(--color-bright)]` — one per page |
| Lead | `mt-2 max-w-[70ch] text-sm text-[var(--color-dim)]` |
| Section | `mt-10 border-t border-[var(--color-border)] pt-4`, opening on a Label |
| Label | `text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase` |
| Card title | 13px 600 Quicklime, two lines, broken at `-`, `_`, `.` |
| Figure | JetBrains Mono 10.5–12px, `tabular` |

Prose is capped at 70ch everywhere, dialogs included.

### Controls

- **Standing** — the one action a region is about:
  `ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px`
- **Quiet** — every secondary action: transparent, Edge border, 7px radius, Graphite 12px label,
  24px tall; pressed takes a Layout Blue edge and a Quicklime label.
- **Input** — `rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm`;
  an error sits below the field, says what is wrong and what to do.

## Page anatomy

Every page past the grid: one Headline, a lead no wider than 70ch in Ash, sections that open on a
Label over a Scribe Line (40px above, 16px below) rather than living in bordered boxes, and one
standing control. **Boxes are for things that float or that you pick up** — cards, dialogs, menus.

The parts page: sticky header (`AppFrame`), a 14rem category rail beside the grid from `md` (a
drawer under it), and the wall of cards reflowing by whole tiles, two columns under 35rem.

Spacing runs 4 / 8 / 12 / 16 / 24 / 32.

## Parts, measurements and honesty

- A part's render sits **inset 7%** in a square Bench Grey well on a bordered card; the border is
  the card's only edge. Never set a render edge to edge.
- A card's name, figures and "approximate" label show **at rest**; nothing hides behind hover.
- **Measurements taken from a mesh are approximations, and Lapidary always says so.** Wherever a
  mesh-derived value appears — card, stage, dialog, tooltip — it is labelled "approximate".
  `Figure` does this for you.
- A part in 3D stands on the lamp's pool with a soft contact shadow; a stage's controls sit on a
  Machined Slate dock, never on the lamp itself.

### One caveat that will bite you

The stylesheet is Lapidary's own **compiled** Tailwind output, so it holds only the utilities the
app already uses. Common ones are there (`flex`, `grid`, `gap-*`, `p-*`, `px-*`, `py-*`, `mt-*`,
`text-xs/sm/lg/2xl`, `font-semibold`, `rounded-md`, `rounded-full`, `border`, `truncate`,
`overflow-hidden`, and the `[var(--…)]` colour utilities the components use). Anything outside the
set silently does nothing — so when a spacing or colour you need is not there, use an inline
`style` over the tokens rather than a utility that may not resolve:
`style={{ padding: 14, borderColor: 'var(--color-edge)' }}`.

## Where the truth is

`styles.css` and the `_ds_bundle.css` it imports hold the full token block, the base layer and the
real utility set. Each component's props are in `components/general/<Name>/<Name>.d.ts`, with
`<Name>.prompt.md` beside it. Reading those beats trusting this summary.

## An idiomatic snippet

```jsx
<article className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
  <h3 className="line-clamp-2 text-[13px] leading-snug font-semibold text-[var(--color-bright)]">
    flange-dn40-lp-3310-02
  </h3>
  <p className="tabular mt-1 text-[11px] text-[var(--color-muted)]">
    <span className="text-[var(--color-dim)]">LP-3310-02</span> · 3,216 triangles · 41.2 × 41.2 × 12.0 mm approximate
  </p>
  <button
    type="button"
    className="ease-mechanical mt-3 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 py-1.5 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
  >
    Download
  </button>
</article>
```
