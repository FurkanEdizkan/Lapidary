# Building with Lapidary

Lapidary is a visual index for 3D part libraries. Dense, dark, instrument-like: a screen is
usually a grid of part renders plus the numbers that tell you whether a part fits.

## Setup

No provider, no theme object, no wrapper. Import `styles.css` and the components are styled.

The palette is **dark only** — there is no light mode and no toggle. `:root` already sets
`color-scheme: dark`, `background-color: var(--color-bg)`, `color: var(--color-text)` and
Inter. Never paint a light background; never add a theme switch.

## The styling idiom: arbitrary-value Tailwind over CSS variables

This is a Tailwind 4 system whose design decisions live in CSS custom properties, so colors
are written as **arbitrary-value utilities wrapping a token** — not as named color
utilities. `bg-[var(--color-surface)]` is the idiom; `bg-surface` does not exist here.

| Token | Value | Use it for |
|---|---|---|
| `--color-bg` | `#0b0c0e` | the page ground |
| `--color-surface` | `#131519` | raised things: cards, panels, inputs |
| `--color-border` | `#24272d` | dividers and hairlines between things |
| `--color-edge` | `#606368` | the boundary of anything you can *operate* — inputs, selects, drop targets. 3:1 contrast, WCAG 2.2 SC 1.4.11. Never use `--color-border` for a control's edge. |
| `--color-text` | `#e6e8ec` | body text |
| `--color-muted` | `#9aa1ac` | secondary text, hints, units |
| `--color-accent` | `#6ea8fe` | the marking dye: focus rings, selection, the one live thing |
| `--radius-sm` / `--radius-md` | `4px` / `10px` | 4px for chrome that should feel machined; 10px for image tiles |
| `--duration-fast/base/slow` | `120/180/280ms` | fast for hover, base for state changes, slow for an authored moment |
| `--ease-mechanical` | `cubic-bezier(0.2, 0, 0, 1)` | every transition |
| `--font-sans` / `--font-mono` | Inter / ui-monospace | Inter ships with the system |

Three real classes, not utilities: **`.ease-mechanical`** (the timing function),
**`.tabular`** (`font-variant-numeric: tabular-nums` — put it on every column of
measurements, counts or byte sizes so digits align), and **`.scrim-in` / `.panel-in`** (the
overlay entrance animations `Dialog` uses).

**Motion rules.** Transitions animate `transform`, `translate` and `opacity` only — never
color, size or layout. A global `@layer base` rule already applies `--ease-mechanical` and
`--duration-base` to everything, so you usually only override the duration:
`duration-[var(--duration-fast)]`. `prefers-reduced-motion` is handled globally.

**Focus is global.** `:focus-visible` draws a 2px `--color-accent` outline at 2px offset,
everywhere. Never write `outline: none` without putting something else in its place.

### One caveat that will bite you

The stylesheet is Lapidary's own **compiled** Tailwind output, so it contains only the
utilities this app already uses — roughly 150 of them. Common ones are there (`flex`,
`grid`, `gap-1/2/3/4/6`, `p-3/4/6`, `mt-*`, `px-*`, `py-*`, `text-xs/sm/lg/xl`, `rounded`,
`rounded-md`, `rounded-full`, `border`, `truncate`, `overflow-hidden`). Anything outside
that set silently does nothing. When you need spacing or a color the set doesn't cover,
reach for an inline `style` using the tokens — `style={{ padding: '14px', borderColor:
'var(--color-edge)' }}` — rather than a utility that may not resolve.

## The components

`Dialog` is the composable one: pure presentation, no data. Use it for any modal.

`Detail`, `FolderTree` and `MovePartDialog` are live application components — they read
from Lapidary's API through TanStack Query and need a `QueryClientProvider` the bundle does
not export, so they will throw if you mount them directly. Treat their `.d.ts` and
`.prompt.md` as the contract for how Lapidary's real screens are shaped, and build new
screens from tokens plus `Dialog`.

## Where the truth is

`_ds/<folder>/styles.css` and the `_ds_bundle.css` it imports — the full token block, the
base layer and the focus ring. Per-component props are in
`components/general/<Name>/<Name>.d.ts`.

## An idiomatic snippet

```jsx
<div className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
  <h2 className="text-sm font-medium">flange-dn40-lp-3310-02</h2>
  <p className="tabular mt-2 text-xs text-[var(--color-muted)]">
    128,540 triangles · 41.2 × 41.2 × 12.0 mm
  </p>
  <button
    type="button"
    className="ease-mechanical mt-4 rounded border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
  >
    Open
  </button>
</div>
```

Measurements taken from a mesh are approximations, and Lapidary always says so. If a value
is mesh-derived, label it — never present it as exact.
