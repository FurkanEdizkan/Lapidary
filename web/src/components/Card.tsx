import { Link } from '@tanstack/react-router'
import { useEffect, useRef, type ReactNode } from 'react'
import { reduced } from '../lib/motion'
import { hasWebGL } from '../lib/viewer-math'
import { type Layout } from '../lib/preferences'
import { strings } from '../lib/strings'
import { PART_DRAG_TYPE, partDragPayload } from './FolderTree'
import type { BlobHash, PartCard, PartId } from '../lib/types'

/**
 * The origin for a tile with no render yet. `flipFrom` declines on a zero-area rectangle, so
 * a part still waiting on the worker opens its panel without a flight rather than bursting
 * out of a point.
 */
export const DEFAULT_ORIGIN = new DOMRect(0, 0, 0, 0)

/**
 * A card's own box, its render's well, and its text, in each layout.
 *
 * Whole class strings, never assembled: Tailwind generates what it finds in source text, and
 * a class built at runtime is a class that was never generated.
 *
 * **Every layout keeps `Figures` visible**, and that is the constraint the gallery is
 * built around rather than an afterthought. `v2`'s gallery overlay shows a name and three
 * dimensions and nothing else; this one carries the approximate label as well, because
 * `CLAUDE.md` makes that label unconditional and an overlay is exactly where it would drop out.
 */
// Flat, like everything that is not an overlay: the hover says "this one" with the edge and a
// one-pixel lift, and a shadow under a card was the one place the grid broke that.
const CARD_SHAPE: Record<Layout, string> = {
  detail:
    'ease-mechanical group relative flex h-full cursor-pointer flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-px hover:border-[var(--color-edge)]',
  gallery:
    'ease-mechanical group relative flex h-full cursor-pointer flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-px hover:border-[var(--color-edge)]',
  // No lift on a row. Forty rows each rising under a passing pointer is a list that shimmers;
  // the edge brightening is enough to say which one is addressed.
  list: 'ease-mechanical group relative flex cursor-pointer items-center gap-3 overflow-hidden rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-surface)] p-2 duration-[var(--duration-fast)] hover:border-[var(--color-edge)]',
}

const WELL: Record<Layout, string> = {
  detail:
    'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)] p-[7%]',
  gallery:
    'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)] p-[7%]',
  list: 'relative flex size-[46px] flex-none items-center justify-center overflow-hidden rounded-[var(--radius-ctl)] bg-[var(--color-raised)] p-1',
}

/**
 * The name, per layout. The gallery clamps it to one line, and that is the fix for the first
 * gallery this shipped: a two-line name above the figures made the overlay two-thirds of a
 * square card, which put the name over the middle of the render — the critique's P0, "a tile
 * that hides the part", back again — and set white text over light grey facets. Every test
 * and a measured `checkVisibility` on the approximate label passed it, because none of them
 * asks whether the *part* can be seen. A screenshot at two widths did. The full name stays in
 * the link's text for every reader and in its `title` for a pointer.
 */
const NAME: Record<Layout, string> = {
  // Two lines, then an ellipsis: a slug of a name is long, and a card that grows a third line
  // for one part pushes its whole row out of step with the rest.
  detail: 'line-clamp-2 text-[13px] leading-snug font-semibold text-[var(--color-bright)]',
  gallery: 'truncate text-[13px] leading-snug font-semibold text-[var(--color-bright)]',
  list: 'text-sm leading-snug font-semibold',
}

const FOOTER: Record<Layout, string> = {
  detail: 'flex flex-1 flex-col gap-1.5 p-3',
  // Clear at the top so the render reads through, and dark enough under the text for the
  // palette's contrast to hold over any render. `v2`'s own stops (0.88 at 48%) did not: over the
  // brightest face `raster.rs` can draw, the name's top row measured 4.1:1 and the triangle
  // count 3.9:1. The name needs alpha 0.53 there and muted text 0.91; these stops give 0.63 and
  // 0.93. The stop is in rem so it moves with `pt-6` rather than with the overlay's height.
  // `contrast.test.ts` cannot see text over a picture — re-measure from a screenshot if the
  // stops, the padding or the text sizes here change.
  gallery:
    'absolute inset-x-0 bottom-0 flex flex-col gap-0.5 bg-[linear-gradient(180deg,rgba(18,18,20,0)_0%,rgba(18,18,20,0.93)_2.5rem,rgba(18,18,20,0.97)_100%)] px-3 pt-6 pb-2.5',
  list: 'flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-4 gap-y-1',
}

/** How long the pointer rests on a card before it turns: long enough that sweeping past starts nothing. */
const SPIN_INTENT_MS = 150

/**
 * Start the turntable in `well`, unless this reader or this browser should not have it: reduced
 * motion (the thumbnail already is the still, lit picture), no WebGL, or — for a pointer — a screen
 * that cannot hover, where a tap fires `mouseenter` on its way to opening the part. The three.js
 * chunk is fetched here, on the first spin, and never for a visitor who does not hover a card.
 */
function turn(well: HTMLElement, hash: BlobHash, fromPointer: boolean): () => void {
  if (reduced() || !hasWebGL()) return () => {}
  if (fromPointer && !window.matchMedia('(hover: hover)').matches) return () => {}
  let stop: (() => void) | null = null
  let cancelled = false
  void import('./turntable')
    .then((turntable) => {
      if (!cancelled) stop = turntable.spin(well, hash)
    })
    .catch(() => undefined)
  return () => {
    cancelled = true
    stop?.()
  }
}

export function Card({
  part,
  onRender,
  busy,
  hostRoot,
  layout,
  selecting,
  selected,
  onToggle,
  onOpen,
  onHover,
  spins = false,
  onLook,
}: {
  part: PartCard
  onRender: (part: PartId) => void
  busy: boolean
  hostRoot: string | null
  layout: Layout
  selecting: boolean
  selected: boolean
  onToggle: (part: PartId, range: boolean) => void
  onOpen: (part: PartCard, from: DOMRect) => void
  onHover: (part: PartCard) => void
  /** Whether resting on this card turns its part. Off while a quick look is open or parts are being picked. */
  spins?: boolean
  /** Space on the name: the part on the quick look's stage, from where its render sits. */
  onLook?: (part: PartCard, from: DOMRect) => void
}) {
  const nameId = `part-name-${part.id}`
  const well = useRef<HTMLDivElement>(null)
  const turning = useRef<{ timer: ReturnType<typeof setTimeout> | null; stop: (() => void) | null }>({
    timer: null,
    stop: null,
  })
  const halt = () => {
    const { timer, stop } = turning.current
    if (timer !== null) clearTimeout(timer)
    stop?.()
    turning.current = { timer: null, stop: null }
  }
  const intend = (fromPointer: boolean) => {
    const hash = part.tessellationL0
    if (!spins || hash === null) return
    halt()
    turning.current.timer = setTimeout(() => {
      turning.current.timer = null
      if (well.current !== null) turning.current.stop = turn(well.current, hash, fromPointer)
    }, SPIN_INTENT_MS)
  }
  // A quick look opening, or selection starting, stops a card already turning.
  useEffect(() => {
    if (!spins) halt()
  }, [spins])
  useEffect(() => halt, [])
  const directory = part.directory
  // A model still in the shared store has no directory to rename, and the move route
  // refuses it. The card withholds the move rather than letting the user discover that
  // from a `409` — the same status the route uses for a name collision, which the UI
  // would otherwise present as one.
  const movable = directory !== null
  // Where the name can be clamped (a line in the gallery, two in the detail grid); a list row
  // has room for the whole name and a tooltip repeating it is noise. Decided here rather than
  // in the attribute, because a literal inside a user-visible attribute reads to the
  // bare-strings gate as copy.
  const clampedName = layout === 'list' ? undefined : part.name
  return (
    <article
      aria-labelledby={nameId}
      onMouseEnter={() => {
        onHover(part)
        intend(true)
      }}
      onMouseLeave={halt}
      /*
        The whole card opens the panel. It stays a handler rather than an anchor because the
        name inside it is itself a link, and an anchor inside an anchor is invalid HTML that
        browsers resolve by guessing — the click is filtered instead of the markup reshaped.

        The filter still asks `closest`, because the name is the one control left in here and
        a click on it belongs to it: the name is the keyboard path, the middle-click path,
        and what a screen reader announces for the card.
      */
      onClick={(event) => {
        if (!(event.target instanceof Element)) return
        if (event.target.closest('a, button, input')) return
        // While selecting, the card is the checkbox's larger target, and the panel waits.
        if (selecting) {
          onToggle(part.id, event.shiftKey)
          return
        }
        // Measured here rather than in the panel, because by the time the panel exists this
        // tile may have been scrolled, re-laid-out by a density change, or replaced by the
        // next page. Where the render *was* when it was clicked is the only honest origin.
        const render = event.currentTarget.querySelector('img')
        onOpen(part, render === null ? DEFAULT_ORIGIN : render.getBoundingClientRect())
      }}
      draggable={movable}
      onDragStart={(event) =>
        event.dataTransfer.setData(
          PART_DRAG_TYPE,
          partDragPayload({ id: part.id, name: part.name }),
        )
      }
      /*
        **A border, which reverses what stood here.** The old note argued that the render is
        its own edge and a box around a picture is a second frame competing with the first.
        That held while the render went edge to edge; `v2` insets it instead, so the card's
        own ground is visible all the way round and the tile has no edge of its own left.
        A hairline is what puts one back — and it is the thing that lifts on hover, which is
        how a pointer says which tile it is on without moving the picture.

        `--color-border` at rest and `--color-edge` under the pointer: the card is a control
        and 1.4.11 wants 3:1 on the boundary that identifies one, but only while it is the
        one being addressed. A wall of forty tiles all drawn at 3:1 is a grid of boxes
        rather than a page of parts.
      */
      className={selected ? `${CARD_SHAPE[layout]} outline-2 outline-[var(--color-accent)]` : CARD_SHAPE[layout]}
    >
      {/*
        The well the render sits in, one step *down* from the card and inset from it.

        `v2` paints the thumbnail `center/86%` on `#17171b` rather than filling the tile:
        the render floats with air around it, which is what makes a wall of parts read as
        objects on shelves instead of as a mosaic. `p-[7%]` is the same 86% from the other
        side, in the one unit that keeps it proportional as the density control changes the
        column width.
      */}
      {/*
        Rendered only while selecting, never rendered and hidden: with selection off the card's
        name is its one tab stop, and a hidden checkbox would still be a second.
      */}
      {selecting ? (
        <input
          type="checkbox"
          checked={selected}
          aria-label={strings.selection.selectPart(part.name)}
          onChange={() => undefined}
          onClick={(event) => onToggle(part.id, event.shiftKey)}
          className="absolute top-2 left-2 z-10 size-6 accent-[var(--color-accent)]"
        />
      ) : null}
      <div ref={well} className={WELL[layout]}>
        {part.thumbnail === null ? (
          // Never an <img> with an empty src: a broken-image glyph reads as a failure,
          // and "the worker has not rasterized this yet" is not one.
          // A 46px list well cannot hold the sentence, and the row's name already says which
          // part this is — so there it is read, not drawn.
          <span className={layout === 'list' ? 'sr-only' : 'text-xs text-[var(--color-muted)]'}>
            {strings.parts.noThumbnail}
          </span>
        ) : (
          <img
            src={part.thumbnail}
            alt={strings.parts.thumbnailAlt(part.name)}
            className="h-full w-full object-contain"
          />
        )}
      </div>

      {/*
        The footer, and the only thing besides the render that survives at rest. A tile a
        person is scanning has to answer "which part is this" without being hovered.
      */}
      <div className={FOOTER[layout]}>
        {/*
          The name is the link, not the whole card — the keyboard path, the middle-click
          path, and what a screen reader announces. It is the card's only tab stop, which is
          why reaching the fiftieth part costs fifty Tab presses rather than two hundred and
          sixty.

          That count is only defensible because the controls it replaced went somewhere a
          keyboard can reach. They live in this panel *and* on the part's own page, which
          this link goes to — for a while they were in the panel alone, and the panel opens
          on a click of a tile that has no key handler, so Render, Move and the storage path
          existed nowhere a keyboard could get to. WCAG 2.2 SC 2.1.1 is Level A and it asks
          whether a function is available, not which surface offers it. Do not move a
          control out of `parts.$partId.tsx` without checking this again.
        */}
        <div className="flex min-w-0 flex-col gap-1">
          <h2 id={nameId} className={NAME[layout]}>
            <Link
              to="/parts/$partId"
              params={{ partId: part.id }}
              title={clampedName}
              onFocus={() => intend(false)}
              onBlur={halt}
              onKeyDown={(event) => {
                // Space, which on a link would only scroll the page. Enter still follows the link.
                if (event.key !== ' ' || onLook === undefined) return
                event.preventDefault()
                halt()
                const render = well.current?.querySelector('img')
                onLook(part, render == null ? DEFAULT_ORIGIN : render.getBoundingClientRect())
              }}
              className="ease-mechanical duration-[var(--duration-fast)] hover:underline"
            >
              {breakable(part.name)}
            </Link>
          </h2>
        </div>
        {/*
          `CLAUDE.md` says a mesh-derived measurement is labelled approximate *always*. It
          used to sit in the hover panel, where "always" quietly meant "never" — the row was
          clipped off the top of the tile at every desktop width. Always means here.
        */}
        <Figures part={part} tight={layout === 'gallery'} />
      </div>
    </article>
  )
}

/**
 * A name with break opportunities after `-`, `_` and `.`.
 *
 * Part names here are slugs (`flange-dn40-lp-3310-02`), which have no spaces, so a browser
 * wraps them wherever the column runs out, mid-word: `flange-dn40-lp-3310-` over `02`.
 * `<wbr>` lets the line break at a separator instead, and adds no text, so the link's
 * accessible name and a copied name are unchanged.
 */
export function breakable(name: string): ReactNode[] {
  return name.split(/(?<=[-_.])/).flatMap((piece, index) => (index === 0 ? [piece] : [<wbr key={index} />, piece]))
}

/**
 * The card's one line of figures: the part number, then the triangle count and its label.
 *
 * A triangle count is tessellation-derived by construction, so a card showing one is
 * showing a mesh-derived figure whatever the wire's `approximate` says. CLAUDE.md
 * forbids such a figure appearing unlabelled, so the label is not a sibling conditional
 * that the count can drift away from: either the count and the label render together or
 * neither does, and the label also renders alone when the flag says some other figure on
 * the part is mesh-derived. No branch here can emit a count without a label.
 *
 * The label is a quiet lowercase word in the line rather than a boxed badge. It is still on
 * every card at rest, never behind a hover; forty boxed capitals down a grid were louder than
 * the parts' names.
 */
function Figures({ part, tight = false }: { part: PartCard; tight?: boolean }) {
  // Narrowed with typeof rather than compared to null: the binding says `number | null`,
  // but the response is cast rather than validated, so a field that disappears upstream
  // arrives here as undefined and would reach .toLocaleString() as one.
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  const labelled = part.approximate || count !== null
  if (part.partNumber === null && !labelled) {
    return null
  }
  return (
    <p
      // Without the top padding over a gallery render: the overlay is the lower third of the
      // card, and eight pixels of it spent on air is eight pixels more of the part hidden.
      className={
        tight
          ? 'tabular mt-auto flex flex-wrap items-baseline gap-x-1.5 text-[11px] text-[var(--color-muted)]'
          : 'tabular mt-auto flex flex-wrap items-baseline gap-x-1.5 pt-1 text-[11px] text-[var(--color-muted)]'
      }
    >
      {part.partNumber === null ? null : <span className="text-[var(--color-dim)]">{part.partNumber}</span>}
      {part.partNumber === null || !labelled ? null : <span aria-hidden="true">·</span>}
      {count === null ? null : <span>{strings.parts.triangles(count)}</span>}
      {!labelled ? null : (
        <span
          title={strings.parts.approximateDetail}
          className="underline decoration-[var(--color-edge)] decoration-dotted underline-offset-2"
        >
          {strings.parts.approximate}
        </span>
      )}
    </p>
  )
}
