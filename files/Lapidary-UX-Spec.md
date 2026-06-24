# Lapidary — UI/UX Specification

> **For the coding agent.** This consolidates the existing Claude Design handoff
> (`design/Lapidary.dc.html`) into a buildable spec and adds the two detail-page
> enhancements the owner asked for (a printed-photo hero backdrop behind the viewer, and a
> related/similar rail). The visual language below is **already decided** — match it; do not
> introduce new palettes or fonts. Per ADR-0001 the frontend stays **React/Vite**, pointed at
> the new Rust API; the API contract (DTOs) is unchanged.

---

## 1. Design language (decided — do not drift)

**Concept.** A maker's workbench instrument. Near-black surfaces, a single cyan accent used
*only* for interactive/active state, and uppercase monospace micro-labels for anything
technical (SPECS, CREATOR, dimensions, layer heights). The feeling is a lapidary's loupe over
a dark bench — precise, quiet, data-forward. Spend all boldness on the mono labels + cyan;
keep everything else disciplined.

**Color tokens** (hex pulled verbatim from the mockup):

| Token | Hex | Use |
|---|---|---|
| `--bg` | `#121214` | app background |
| `--ink-well` | `#0d0d0e` | deepest well — 3D viewer background, behind content |
| `--surface-1` | `#1a1a1d` | cards, panels |
| `--surface-2` | `#1d1d20` / `#212125` | raised panels, inputs, chips |
| `--surface-3` | `#242428` | hover / active fill |
| `--border-soft` | `#26262a` | hairline dividers |
| `--border` | `#2c2c30` | default borders |
| `--border-strong` | `#2e2e32` | rail / header separators |
| `--text` | `#eaeaec` | primary text |
| `--text-muted` | `#9b9ba1` | secondary |
| `--text-dim` | `#82828a` | tertiary |
| `--text-faint` | `#6c6c73` | mono micro-labels |
| `--accent` | `#2cb4f5` | active state, focus ring, viewer highlights, links |

**Type.**
- **Archivo** (sans) — all UI text, model names, body. Set a tight scale; model names are the
  largest UI type, everything else steps down.
- **JetBrains Mono** — the **signature**. Uppercase, letter-spaced (~0.08em), `--text-faint`,
  small. Used for eyebrow labels (`SPECS`, `CREATOR`, `ADDED`, `TYPE`, `TAGS`, `GROUPS`,
  `PINNED`) and for numeric/technical values (dimensions, layer height, exposure, file size).
  This treatment is what makes Lapidary recognizable — apply it consistently.

**Accent discipline.** Cyan never fills large areas. It marks the active view toggle, the
focused input ring, the selected nav item, links (creator/tag), and small viewer accents.
Default chips/buttons are `--surface-2` with `--border`; cyan only on hover/active/focus.

**Motion.** Quiet. ~120–160ms ease on hover fills, view switches, and the detail-page
open/close (a fast fade + slight scale-in for the overlay). Respect `prefers-reduced-motion`:
drop the scale, keep an instant fade. No ambient/looping animation anywhere.

**Quality floor.** Responsive to mobile (rail collapses to a drawer), visible keyboard focus
(cyan ring), full keyboard nav of the gallery + viewer controls, ARIA on the view toggle and
modal. Sentence case for buttons that act ("Add to library", "Reset view"), uppercase mono
only for the labels.

---

## 2. App shell & screen inventory

```
┌──────────────────────────────────────────────────────────────────────┐
│  LAPIDARY            [ search…              ]        [ + Add to library]│  top bar
├───────────────┬──────────────────────────────────────────────────────┤
│ PINNED        │  MODELS · 20            [ Grid ] [ Cards ] [ List ]    │  toolbar
│  · creators   │                                                        │
│  · tags       │   ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐                      │
│  · groups     │   │ thmb│ │ thmb│ │ thmb│ │ thmb│   ← image-first grid │
│               │   └─────┘ └─────┘ └─────┘ └─────┘                      │
│ CREATORS      │   ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐                      │
│  TrenchWorks  │   │ thmb│ │ thmb│ │ thmb│ │ thmb│                      │
│  Forge&Fathom │   └─────┘ └─────┘ └─────┘ └─────┘                      │
│  …            │                                                        │
│ GROUPS  (All) │                                                        │
│  Trench Army★ │                                                        │
│ TAGS          │                                                        │
│  wargaming …  │                                                        │
└───────────────┴──────────────────────────────────────────────────────┘
```

Three screens/surfaces total: **Gallery** (shell + grid), **Detail** (overlay over the
gallery), **Add/Scan** (modal). The left rail and top bar persist on the gallery.

**Left rail** — `PINNED` (pinned creators/tags/groups, star-marked), `CREATORS` (filter),
`GROUPS` with an All/Shared toggle (shared groups marked), `TAGS` (filter). Clicking any entry
filters the grid; pinned entries float to the top of their section. Rail collapses to a
hamburger drawer below ~900px.

**Top bar** — brand `LAPIDARY` (mono, letter-spaced), the search field with a live suggest
dropdown, and the primary `+ Add to library` action.

---

## 3. Gallery

**Image-first.** Tiles lead with the thumbnail (the always-hot tier — see §7), name below in
Archivo, and a mono sub-line (`TYPE · creator`). On hover: a subtle `--surface-3` lift, and a
metadata reveal (dimensions in mm + format + file size, all mono).

**Three views** (toggle in the toolbar, cyan marks the active one):
- **Grid** — dense thumbnail-only grid; name/meta on hover. Default.
- **Cards** — larger cards with name, creator, type, tag chips always visible.
- **List** — one row per model: thumbnail chip, name, creator, type, size, format, file size,
  added date — the mono columns make this read like a parts table.

**Search suggest.** Typing in the top-bar field opens a dropdown grouped into **Models**,
**Creators**, **Tags** (try `dragon`). Arrow-key navigable; Enter on a model opens Detail,
Enter on a creator/tag applies it as a filter. Backed by SQLite FTS5 (Phase 2).

**Toolbar count.** `MODELS · N` (mono) reflects the current filtered count, updating live.

**States.**
- *Loading:* thumbnail skeletons in `--surface-2` (no spinners).
- *Empty (filtered):* "No models match" + the active filters as removable chips + a "Clear
  filters" action. (This exact copy is in the mockup — keep it.)
- *Empty (fresh library):* invitation to add or scan — "Your library is empty. Add a model or
  point a Scan at a folder." A fresh DB is seeded with 20 samples (Phase 2), so this is rare.

---

## 4. Detail page (the centerpiece — includes the requested enhancements)

Opens as an overlay over the gallery (fast fade + slight scale-in). Three stacked layers:

```
┌───────────────────────────────────────────────────────────── [ ✕ ]──┐
│ ░░░ blurred + darkened printed-result photo as backdrop ░░░░░░░░░░░░░ │ ← BACKDROP (new)
│ ░░░                                                            ░░░░░░ │
│ ░░░          ┌───────────────────────────────┐                ░░░░░░ │
│ ░░░          │                               │   MODEL              │
│ ░░░          │      interactive 3D viewer     │   Gothic Trench…    │ ← FOREGROUND
│ ░░░          │      (orbit · zoom)            │   ───────────────   │
│ ░░░          │                               │   SPECS             │
│ ░░░          │            [ Reset view ]      │   120 × 45 × 62 mm  │ ← META PANEL
│ ░░░          └───────────────────────────────┘   STL · 18.4 MB     │
│ ░░░            [ View full mesh ]                 TYPE  Terrain      │
│ ░░░                                              CREATOR TrenchWorks│
│ ░░░                                              ADDED 2026-01-14    │
│ ░░░                                              TAGS  ⬡ ⬡ ⬡        │
│ ░░░  PRINTER COMPATIBILITY  · Bambu X1C · P1S · Ender 3            │
│ ░░░  SUGGESTED PRINT SETTINGS   [editable rows · import]            │
│ ░░░  PRINTED RESULTS   [photo] [photo] [+]                          │
├──────────────────────────────────────────────────────────────────────┤
│  SIMILAR    ┌────┐ ┌────┐ ┌────┐ ┌────┐   ← related rail (new)        │
│             └────┘ └────┘ └────┘ └────┘                              │
└──────────────────────────────────────────────────────────────────────┘
```

**Layer 1 — backdrop (NEW).** The model's primary **printed-result photo**, blurred (~24px)
and darkened (~65% black overlay, matching the mockup's `rgba(0,0,0,0.65)`), fills the page
behind everything. This is the "real image as feed on the background" request. **Fallback
order:** primary printed photo → any printed photo → a soft radial gradient derived from the
model's `color` field (so it always looks intentional, never blank).

**Layer 2 — viewer (foreground).** The interactive 3D viewer sits center-left over the well
(`--ink-well`), drag-to-rotate / scroll-to-zoom, with a `Reset view` control. It streams the
**Draco-GLB** viewer mesh (warm tier, §7), *not* the raw original. A `View full mesh` action
loads the full original on explicit request only. While the GLB streams, show the model's
procedural placeholder shape (the mockup already has a seeded procedural renderer for this).

**Layer 3 — metadata panel.** Right-aligned, each block led by a mono uppercase label:
- **SPECS** — `size` as `X × Y × Z mm`, `format · fileSize`, `triangleCount` (mono values).
- **TYPE** — the model type.
- **CREATOR** — links to a creator-filtered gallery.
- **ADDED** / **CREATED** — dates.
- **TAGS** — hexagon/chip tags (`⬡`), each a link to a tag-filtered gallery.
- **PRINTER COMPATIBILITY** — the compatible printers as chips.
- **SUGGESTED PRINT SETTINGS** — the editable key/value rows (Process, Layer height, Infill,
  Supports, Material, Note…), with **import** from PrusaSlicer/Orca `.ini` or Cura `.json`.
- **PRINTED RESULTS** — a strip of attached printed/painted photos + an add-photo tile.

**Layer 4 — SIMILAR rail (NEW).** A horizontal rail of related models at the bottom.
Relatedness is computed cheaply from **shared tags + same type + shared group** (a SQL join —
this is what the owner meant by "related games / similar products": e.g. other
`trench-crusade`/`wargaming` terrain). Start with this overlap score; an embedding/geometry
similarity can replace the scorer later without UI change. 6–10 tiles, same thumbnail
treatment as the gallery, click swaps the detail page in place.

**Close / navigate.** `✕` or Esc closes back to the gallery scroll position. Left/right arrows
(or on-screen chevrons) step through the current filtered set.

---

## 5. Add / Scan modal

`+ Add to library` opens a modal (`Add model to library`): file picker or a path to scan,
name/creator/type fields, tag and group assignment, optional printed photos, and `Create` /
`Cancel`. A `Scan` flow indexes a read-only library folder (`LIBRARY_PATH`) and enqueues
extraction jobs (thumbnail/LOD/GLB) per file — the model appears with a skeleton thumbnail
that fills in when the worker (Phase 3) finishes.

---

## 6. Component map (existing React → responsibility)

The repo already has these under `web/src/components/` — keep them, rewire data to the Rust
API. No rename needed.

| Component | Owns |
|---|---|
| `TopNav` | brand, search field entry |
| `SearchSuggest` | grouped Models/Creators/Tags dropdown (FTS-backed) |
| `PinnedGroupsBar`, `TagRail`, `GroupChips` | left-rail pins, tags, group All/Shared |
| `Gallery` + `GridView` / `CardsView` / `ListView` | the three gallery views |
| `ModelTile` | a single gallery tile (thumbnail-first) |
| `DetailOverlay` | the detail page shell + layer composition |
| `ModelViewer` (+ `lib/threeViewer.ts`, `lib/mesh3d.ts`, `lib/mesh3dProc.js`) | 3D viewer + procedural placeholder |
| `SpecTable`, `PrinterCompat`, `SettingsTable`, `PrintedResults` | the detail metadata blocks |
| `AddModelModal` | add/scan flow |
| `TagsGroupsManager` | manage tags/groups |
| `theme.ts`, `global.css`, `lib/format.ts`, `lib/thumbs.ts`, `store.ts` | tokens, formatting, thumb URLs, state |

**New work for the enhancements:** extend `DetailOverlay` with the backdrop layer (Layer 1)
and a `SimilarRail` component (Layer 4). Everything else is data-rewiring.

---

## 7. Performance & rendering rules (tie UI to the storage tiers)

The UI must honor the three-tier storage so the gallery stays instant:
- **Gallery** only ever requests the tiny always-hot **thumbnail** (`hasThumbnail`). Never
  load meshes in the grid. Use keyset pagination + grid virtualization for large libraries;
  lazy-load thumbnails as tiles enter the viewport.
- **Detail open** streams the **Draco-GLB** (warm tier). On a cache miss the server
  regenerates it (lazy-promote) — show the procedural placeholder until it arrives.
- **`View full mesh`** is the *only* path that loads the full (compressed) original, decompressed
  on demand. Gate it behind the explicit action so opening a model never pays that cost.
- Reflect tier availability with `hasThumbnail` / `hasLod` / `hasOriginal` from the DTO (e.g.
  disable `View full mesh` if an archived original is still rehydrating).

---

## 8. Voice & copy

Interface voice, not personal. Active verbs on buttons ("Add to library", "Reset view",
"View full mesh", "Import settings"). Mono labels are nouns, uppercase. Empty/error states give
direction, never apologize: "No models match" + how to clear; on a failed render, "Preview
isn't ready yet — retrying" rather than an error dump. An action keeps its name through the
flow (the `Create` button's success returns the user to the model it created).

---

## 9. Build order for the frontend (Phase 4, Track A)

1. Port `theme.ts`/`global.css` tokens to the table in §1 (verify against the mockup).
2. Wire `api/client.ts` to the Rust endpoints; confirm DTO shape (camelCase) matches.
3. Gallery parity: grid/cards/list, hover meta, search suggest, rail filters, counts, states.
4. Detail parity: viewer + all four metadata blocks + add/scan.
5. **Enhancements:** backdrop layer + `SimilarRail`.
6. A11y + responsive pass (rail drawer, focus rings, reduced motion).
