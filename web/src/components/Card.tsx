import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { fetchPartImages } from '../lib/api'
import { Icon } from './Icon'
import Lattice from './Lattice'
import { type Layout } from '../lib/preferences'
import { strings } from '../lib/strings'
import { PART_DRAG_TYPE, partDragPayload } from './FolderTree'
import type { PartCard, PartId, PartImage } from '../lib/types'

/**
 * The origin for a tile with no picture. `flipFrom` declines on a zero-area rectangle, so a
 * part nobody has given a picture opens its panel without a flight rather than bursting out
 * of a point.
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

// Edge to edge since A15: the owner's pictures are photographs, and the inset left them a third smaller than the tile.
const WELL: Record<Layout, string> = {
  detail: 'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)]',
  gallery: 'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)]',
  list: 'relative flex size-[46px] flex-none items-center justify-center overflow-hidden rounded-[var(--radius-ctl)] bg-[var(--color-raised)] p-1',
}

/** A well with no picture: the lamp's pool under the still lattice, as on a stage (DESIGN.md § Lattice). */
const BARE_WELL: Record<Layout, string> = {
  detail: 'stage-lamp relative flex aspect-square items-center justify-center overflow-hidden',
  gallery: 'stage-lamp relative flex aspect-square items-center justify-center overflow-hidden',
  list: 'stage-lamp relative flex size-[46px] flex-none items-center justify-center overflow-hidden rounded-[var(--radius-ctl)] p-1',
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
  detail: 'line-clamp-2 min-h-[2.75em] text-[13px] leading-snug font-semibold text-[var(--color-bright)]',
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
  // Under `sm` the figures always go under the name: wrapping only when the name is long put them in two places.
  list: 'flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-4 gap-y-1 max-sm:flex-col max-sm:items-start',
}

/**
 * A card's picture arrows: shown under a pointer, always where there is no hover to reveal them.
 * Whole strings, for the reason `CARD_SHAPE`'s are.
 */
const FLIP_LEFT =
  'ease-mechanical absolute top-1/2 left-1.5 z-10 flex size-7 -translate-y-1/2 items-center justify-center rounded-full bg-[rgba(18,18,20,0.8)] text-[var(--color-bright)] opacity-0 duration-[var(--duration-fast)] group-hover:opacity-100 hover:bg-[var(--color-raised)] [@media(hover:none)]:opacity-100'
const FLIP_RIGHT =
  'ease-mechanical absolute top-1/2 right-1.5 z-10 flex size-7 -translate-y-1/2 items-center justify-center rounded-full bg-[rgba(18,18,20,0.8)] text-[var(--color-bright)] opacity-0 duration-[var(--duration-fast)] group-hover:opacity-100 hover:bg-[var(--color-raised)] [@media(hover:none)]:opacity-100'

/**
 * A stored framing as the two CSS properties that apply it (`0019_image_framing.sql`).
 *
 * **The browser does the framing**, here and in the gallery's `Framed` alike, so this is one
 * definition rather than two: a picture cropped one way on a card and another way on the part's
 * own page would be two answers to the same question. `null` is a part with no picture, shown
 * whole.
 */
export function pictureStyle(image: PartImage | null): CSSProperties {
  if (image === null) return { objectFit: 'contain' }
  return {
    objectFit: image.fit === 'contain' ? 'contain' : 'cover',
    objectPosition: `${image.focusX * 100}% ${image.focusY * 100}%`,
  }
}

export function Card({
  part,
  layout,
  selecting,
  selected,
  onToggle,
  onOpen,
  onLook,
}: {
  part: PartCard
  layout: Layout
  selecting: boolean
  selected: boolean
  onToggle: (part: PartId, range: boolean) => void
  onOpen: (part: PartCard, from: DOMRect) => void
  /** Space on the name: the part on the quick look's stage, from where its picture sits. */
  onLook?: (part: PartCard, from: DOMRect) => void
}) {
  const nameId = `part-name-${part.id}`
  const well = useRef<HTMLDivElement>(null)
  // Where the name can be clamped (a line in the gallery, two in the detail grid); a list row
  // has room for the whole name and a tooltip repeating it is noise. Decided here rather than
  // in the attribute, because a literal inside a user-visible attribute reads to the
  // bare-strings gate as copy.
  const clampedName = layout === 'list' ? undefined : part.name
  /*
    **The part's own picture, or the honest placeholder — nothing in between.** R1 turned the card
    from a render into a picture and kept the render behind it; R3 stopped rendering, so the
    fallback has no producer and no field on the wire. A part nobody has photographed names its
    format instead of apologising for a picture it does not have.
  */
  /*
    **The rest of a set, only when asked** (M5). The page carries the first picture and how many
    there are; the gallery is fetched on the first arrow press, under the part page's own key, so a
    grid of forty cards costs no request until someone flips one. A list row's well is too small to
    flip in, and the part's page shows every picture to a keyboard.
  */
  const [shown, setShown] = useState(0)
  const flips = layout !== 'list' && part.imageCount > 1
  const gallery = useQuery({
    queryKey: ['part-images', part.id],
    queryFn: () => fetchPartImages(part.id),
    enabled: flips && shown !== 0,
  })
  const set = gallery.data ?? []
  const total = set.length > 0 ? set.length : part.imageCount
  const image = shown === 0 || set.length === 0 ? part.image : (set[shown % set.length] ?? part.image)
  const flip = (step: number) => setShown((index) => (index + step + total) % total)
  const src = image?.src ?? null
  return (
    <article
      aria-labelledby={nameId}
      /*
        **No hover handler at all, and that is a rule rather than an omission** (R1). A card's
        picture is the picture; nothing loads, fetches, draws or warms because a pointer crossed a
        tile. The spin that used to live here fetched a mesh per hovered card, which is fifty
        original files for one sweep down a grid — and `Grid.test.tsx` asserts the absence.
      */
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
      draggable
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
        The well the picture sits in, one step *down* from the card.

        Until A15 it was inset 7% (`v2`'s `center/86%`), which suited renders floating on a shelf. The owner's
        pictures are photographs and store renders with their own backgrounds, and the inset only made them a
        third smaller, so the picture now fills the well and its framing (`pictureStyle`) decides the crop.
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
      <div ref={well} className={src === null ? BARE_WELL[layout] : WELL[layout]}>
        {src === null ? (
          // Never an <img> with an empty src: a broken-image glyph reads as a failure, and
          // "nobody has given this part a picture" is not one. The still lattice stands in (A21, DESIGN.md §
          // Lattice): the settled frame of the sign-in drawing over the lamp, one shared path tree, never moving.
          // The format is the footer's chip to the eye; the word here is what a reader hears, once.
          <>
            <Lattice still className={layout === 'list' ? 'size-full' : 'size-[76%]'} />
            <span className="sr-only">{strings.parts.noPicture(part.sourceFormat)}</span>
          </>
        ) : (
          <img
            src={src}
            alt={strings.parts.pictureAlt(part.name)}
            className="h-full w-full"
            style={pictureStyle(image)}
          />
        )}
        {flips ? (
          <>
            {/*
              Out of the tab order, like everything on a card but its name: forty cards with two
              arrows each is eighty stops between a person and the fiftieth part. The arrows are a
              pointer's shortcut; the part's page is where every picture is for a keyboard.
            */}
            <button
              type="button"
              tabIndex={-1}
              aria-label={strings.parts.previousPicture}
              onClick={() => flip(-1)}
              className={FLIP_LEFT}
            >
              <Icon name="caretLeft" />
            </button>
            <button
              type="button"
              tabIndex={-1}
              aria-label={strings.parts.nextPicture}
              onClick={() => flip(1)}
              className={FLIP_RIGHT}
            >
              <Icon name="caretRight" />
            </button>
            <span className="tabular absolute top-2 right-2 z-10 rounded-[var(--radius-ctl)] bg-[rgba(18,18,20,0.8)] px-1.5 py-0.5 text-[11px] text-[var(--color-muted)]">
              {strings.parts.pictureOf((shown % total) + 1, total)}
            </span>
          </>
        ) : null}
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
              onKeyDown={(event) => {
                // Space, which on a link would only scroll the page. Enter still follows the link.
                if (event.key !== ' ' || onLook === undefined) return
                event.preventDefault()
                const picture = well.current?.querySelector('img')
                onLook(part, picture == null ? DEFAULT_ORIGIN : picture.getBoundingClientRect())
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
        <Figures part={part} tight={layout === 'gallery'} pictured={src !== null} />
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

/** The pack formats (A13), as `lapidary-core`'s `PACK_EXTENSIONS`. */
const PACKS = ['zip', '7z', 'rar']

/**
 * The card's one line of figures: the format chip, the part number, then the triangle count with its ≈ before it.
 *
 * A triangle count is tessellation-derived by construction, so a card showing one is
 * showing a mesh-derived figure whatever the wire's `approximate` says. CLAUDE.md
 * forbids such a figure appearing unlabelled, so the label is not a sibling conditional
 * that the count can drift away from: either the count and its ≈ render together or
 * neither does, and the label also renders alone — as the word, since a ≈ before nothing says nothing — when the flag
 * says some other figure on the part is mesh-derived. No branch here can emit a count without a label.
 *
 * The ≈ is the mark the part page's figures carry (`strings.detail.approximate`), with its dotted Edge underline and a
 * title that explains it; a screen reader hears the word. It is on every card at rest, never behind a hover.
 *
 * **The format chip** (A21) is a fact, not a control: a 4px Bench Grey label, mono Graphite, no edge. Uppercased in
 * CSS rather than in the string, so the well's own spoken format word (`strings.parts.noPicture`) stays the one exact
 * match a reader or a test finds for it.
 */
function Figures({ part, tight = false, pictured }: { part: PartCard; tight?: boolean; pictured: boolean }) {
  // Narrowed with typeof rather than compared to null: the binding says `number | null`,
  // but the response is cast rather than validated, so a field that disappears upstream
  // arrives here as undefined and would reach .toLocaleString() as one.
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  const labelled = part.approximate || count !== null
  // A pack (A13) has no figures; its size is what a person deciding to download it wants.
  const pack = part.sourceFormat !== null && PACKS.includes(part.sourceFormat) ? part.sourceBytes : null
  const format = part.sourceFormat
  if (format === null && part.partNumber === null && !labelled && pack === null) {
    return null
  }
  return (
    <p
      // Without the top padding over a gallery render: the overlay is the lower third of the
      // card, and eight pixels of it spent on air is eight pixels more of the part hidden.
      className={
        tight
          ? 'tabular mt-auto flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[11px] text-[var(--color-muted)]'
          : 'tabular mt-auto flex flex-wrap items-center gap-x-1.5 gap-y-0.5 pt-1 text-[11px] text-[var(--color-muted)]'
      }
    >
      {format === null ? null : (
        // Unheard on a card with no picture, whose well already says the format to a reader.
        <span
          aria-hidden={pictured ? undefined : true}
          className="rounded-[var(--radius-sm)] bg-[var(--color-raised)] px-1 py-px text-[10px] leading-tight uppercase"
        >
          {format}
        </span>
      )}
      {part.partNumber === null ? null : <span className="text-[var(--color-dim)]">{part.partNumber}</span>}
      {pack === null ? null : <span>{strings.parts.pack(pack)}</span>}
      {count === null ? (
        !labelled ? null : (
          <span
            title={strings.parts.approximateDetail}
            className="underline decoration-[var(--color-edge)] decoration-dotted underline-offset-2"
          >
            {strings.parts.approximate}
          </span>
        )
      ) : (
        <span>
          <span
            aria-hidden="true"
            title={strings.parts.approximateDetail}
            className="mr-0.5 underline decoration-[var(--color-edge)] decoration-dotted underline-offset-2"
          >
            {strings.detail.approximate}
          </span>
          <span className="sr-only">{strings.parts.approximate}</span>
          {strings.parts.triangles(count)}
        </span>
      )}
    </p>
  )
}
