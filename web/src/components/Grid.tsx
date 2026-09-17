import { useEffect, useLayoutEffect, useRef } from 'react'
import { type Density, type Layout } from '../lib/preferences'
import { strings } from '../lib/strings'
import type { PartCard, PartId } from '../lib/types'
import { Card } from './Card'
import { arrive } from '../lib/motion'

/**
 * How much of the library is on screen, and the sentinel that fetches the rest.
 *
 * A grid is a scrolling surface, so the gesture that means "show me more" is scrolling to
 * the end of it. An `IntersectionObserver` on a sentinel after the last card is fifteen
 * lines and no dependency; a "load more" button would make a user click ten times to see
 * a library they can already scroll through.
 *
 * The button is still rendered, and not as a fallback nobody sees. It is what a keyboard
 * user reaches, and what works when `IntersectionObserver` never fires because the grid is
 * short enough that the sentinel is already on screen and never crosses the boundary
 * again. Both paths call the same thing.
 *
 * `hasMore` is the server's own answer, not a length comparison against a limit this
 * component would have to know: a full page hands back a cursor and a short one hands back
 * null.
 */
export function MorePages({
  hasMore,
  fetching,
  onMore,
}: {
  hasMore: boolean
  fetching: boolean
  onMore: () => void
}) {
  const sentinel = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const node = sentinel.current
    if (node === null || !hasMore) {
      return
    }
    // `fetching` is deliberately not in the dependency list. Re-creating the observer on
    // every fetch would disconnect and reconnect it mid-scroll, and an observer that
    // reconnects while its target is already visible fires immediately — which is a
    // second request for the page still in flight. The guard is inside the callback
    // instead, where it reads the current value.
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        onMore()
      }
    })
    observer.observe(node)
    return () => observer.disconnect()
  }, [hasMore, onMore])

  return (
    <>
      <div ref={sentinel} aria-hidden className="h-px" />
      {/*
        The count moved to the scope line above the grid, where `v2` puts it and where it is
        legible before the scanning starts rather than after it. What is left here is the
        control, and only when there is another page — a sentence saying the library is all
        on screen is what the header now says by naming the whole count.
      */}
      {!hasMore ? null : (
        <p className="mt-4 max-w-prose text-xs text-[var(--color-muted)]">
          <button
            type="button"
            onClick={onMore}
            disabled={fetching}
            className="underline underline-offset-2 disabled:opacity-50"
          >
            {fetching ? strings.parts.loadingMore : strings.parts.loadMore}
          </button>
        </p>
      )}
    </>
  )
}

export function Grid({
  parts,
  onRender,
  busyPart,
  hostRoot,
  density,
  layout,
  selecting,
  selected,
  onToggle,
  onSelectAll,
  onOpen,
  onHover,
  spins = false,
  onLook,
}: {
  parts: readonly PartCard[]
  onRender: (part: PartId) => void
  busyPart?: PartId
  /** Passed down rather than fetched per card: it is one fact about the deployment. */
  hostRoot: string | null
  density: Density
  layout: Layout
  selecting: boolean
  selected: ReadonlySet<PartId>
  /** `range` is a shift-click: everything from the last part toggled to this one. */
  onToggle: (part: PartId, range: boolean) => void
  onSelectAll: () => void
  /** A card asks to be looked at, from where its render sits. */
  onOpen: (part: PartCard, from: DOMRect) => void
  /** The pointer is over a card: the moment to warm its rung. */
  onHover: (part: PartCard) => void
  /** Whether a card turns its part under a resting pointer or focus. See `turntable.ts`. */
  spins?: boolean
  /** Space on a card's name. See `StageLook`. */
  onLook?: (part: PartCard, from: DOMRect) => void
}) {
  // Two numbers move together and have to: the column width sets how tall a card ends up,
  // and `contain-intrinsic-size` is the placeholder height for one that has not rendered.
  // Give the compact grid the comfortable card's height and the scrollbar jumps as cards
  // enter and leave — which is what makes `content-visibility` look broken.
  //
  // Whole class strings rather than interpolation: Tailwind scans source for literals, and
  // a class built at runtime is a class that was never generated.
  // A list is one column whatever the density: density sizes a card, and a row has no card
  // to size.
  const columns =
    layout === 'list'
      ? 'grid-cols-1 gap-1.5'
      : density === 'compact'
        ? 'grid-cols-[repeat(auto-fill,minmax(8rem,1fr))] gap-3 max-xs:grid-cols-2 max-xs:gap-2'
        : 'grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 max-xs:grid-cols-2 max-xs:gap-2'
  //
  // **A compact card is TALLER, not shorter**, and guessing the other way was the first
  // thing this got wrong. A narrower column wraps more of the name and more of the "9.7 kB
  // on disk, stored uncompressed" line, so 8rem-wide cards run past 11rem-wide ones.
  // Measured in Chrome over 24 cards of the real 156-part library: comfortable 442–461px
  // (median 27.6rem), compact 478–516px (median 31.1rem). The same mistake the 26rem figure
  // below already records making once — a guess, in the wrong direction, about a height
  // that has to be measured.
  //
  // **Measured again for the two-line card** (name clamped to two lines, one mono line of
  // figures, no size sentence), in Chrome at a 1157px viewport over the example library:
  // comfortable 17.16–18.28rem and compact 14.89rem. The old 26rem and 31rem figures below
  // described a card that no longer exists; the compact card is shorter again now, because
  // the clamp stops a narrow column wrapping the name onto a third and fourth line.
  //
  // Gallery and list, measured in Chrome over the seeded library: a comfortable gallery card
  // rendered 12.89rem tall and a compact one 8.47rem, at a 1157px viewport — square, because a
  // gallery card is its well and nothing else, so these two track column width and are the
  // figures most likely to drift at other widths, which the leading `auto` absorbs after first
  // render. A list row is 4rem at either density, exactly: its height is the 46px well plus
  // padding and nothing about it scales. The first draft of these was guessed at 11rem and
  // 8rem, and was marked as a guess until this replaced it.
  const intrinsic =
    layout === 'list'
      ? '[contain-intrinsic-size:auto_4rem]'
      : layout === 'gallery'
        ? density === 'compact'
          ? '[contain-intrinsic-size:auto_8.5rem]'
          : '[contain-intrinsic-size:auto_13rem]'
        : density === 'compact'
          ? '[contain-intrinsic-size:auto_15rem]'
          : '[contain-intrinsic-size:auto_18.5rem]'
  const list = useRef<HTMLUListElement>(null)
  // Every part this grid has already shown. A card arrives once: a refetch during a scan hands
  // back the same parts as new objects, and replaying the arrival on each of them would make
  // the grid flicker every time the worker finishes a file.
  const shown = useRef(new Set<PartId>())
  useLayoutEffect(() => {
    const items = list.current?.children
    if (items === undefined) return
    const arriving: HTMLElement[] = []
    parts.forEach((part, index) => {
      if (shown.current.has(part.id)) return
      shown.current.add(part.id)
      const item = items[index]
      // Only what is on screen. A card below the fold is skipped by `content-visibility`
      // anyway, and would otherwise sit at opacity 0 for its turn with nobody watching.
      if (item instanceof HTMLElement && item.getBoundingClientRect().top < window.innerHeight) {
        arriving.push(item)
      }
    })
    // Not cancelled when `parts` changes again: a refetch landing mid-arrival would snap the
    // cards visible, and an animation on a card that unmounts simply ends with it.
    if (arriving.length > 0) arrive(arriving)
  }, [parts])

  return (
    <ul
      ref={list}
      role="list"
      className={`grid list-none ${columns}`}
      // Ctrl/Cmd-A inside the grid selects every part loaded, while selecting. Anywhere else
      // it is still the browser's own select-all.
      onKeyDown={(event) => {
        if (selecting && selectsAll(event)) {
          event.preventDefault()
          onSelectAll()
        }
      }}
    >
      {parts.map((part) => (
        // `content-visibility: auto` is the virtualization, and it is one CSS property
        // rather than a dependency. It tells the browser to skip layout, paint and image
        // decode for a card that is off screen, which is what a virtualizer buys — while
        // this grid stays a plain `repeat(auto-fill, …)` CSS grid, whose column count
        // changes with the viewport and which a virtualizer would therefore have to
        // measure and re-measure to know a row height it currently never needs.
        //
        // `contain-intrinsic-size` is not optional beside it. Without a placeholder size
        // a skipped card measures zero, so the page height collapses and the scrollbar
        // jumps as cards enter and leave — which is what makes `content-visibility` look
        // broken.
        //
        // 26rem is 416px, which is a rendered card measured in Chrome (415px, uniform
        // across 1,000 of them) and not arithmetic — the first guess was 20rem from a
        // card measured before its thumbnail had loaded, and it under-reported the page
        // height by 23%. The leading `auto` means the browser substitutes each card's
        // real size once it has rendered one, so this figure only has to be close for the
        // first paint rather than exact forever.
        <li key={part.id} className={`[content-visibility:auto] ${intrinsic}`}>
          <Card
            part={part}
            onRender={onRender}
            busy={part.id === busyPart}
            hostRoot={hostRoot}
            layout={layout}
            selecting={selecting}
            selected={selected.has(part.id)}
            onToggle={onToggle}
            onOpen={onOpen}
            onHover={onHover}
            spins={spins && layout !== 'list'}
            onLook={onLook}
          />
        </li>
      ))}
    </ul>
  )
}

/** How many parts a bulk action changes at once. See `eachAtMost`. */
export const BULK_CONCURRENCY = 4

export type BulkProgress = {
  done: number
  total: number
  failures: { id: PartId; name: string; reason: string }[]
}

function selectsAll(event: { key: string; ctrlKey: boolean; metaKey: boolean }): boolean {
  return (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a'
}

/**
 * The count, the two actions, and afterwards the parts an action did not change.
 *
 * Move and Remove only: purge stays on the Removed page, one part at a time, because it is the
 * one thing in the product that cannot be undone.
 */
export function SelectionBar({
  count,
  bulk,
  onMove,
  onRemove,
  onExport,
  note,
  onClear,
}: {
  count: number
  bulk: BulkProgress | null
  onMove: () => void
  onRemove: () => void
  /** A bundle of the selection: planned, then downloaded. */
  onExport: () => void
  /** What the last export said: what it holds, or why the server refused it. */
  note: string | null
  onClear: () => void
}) {
  const busy = bulk !== null && bulk.done < bulk.total
  const button =
    'ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-60'
  return (
    <section
      aria-label={strings.selection.bar}
      // Pinned to the foot of the window while you scroll a long grid picking parts: the count
      // and the actions stay in reach of the fiftieth card. It floats over the cards, so it is
      // the one bar that takes the overlay shadow.
      className="sticky bottom-4 z-10 mb-3 flex flex-col gap-2 rounded-md border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-2 text-sm shadow-overlay"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span aria-live="polite" className="tabular mr-2">
          {bulk !== null && bulk.done < bulk.total
            ? strings.selection.working(bulk.done, bulk.total)
            : strings.selection.count(count)}
        </span>
        <button type="button" className={button} disabled={count === 0 || busy} onClick={onMove}>
          {strings.folders.moveTo}
        </button>
        <button
          type="button"
          className={button}
          disabled={count === 0 || busy}
          onClick={onRemove}
          title={strings.removal.removeHint}
        >
          {strings.removal.remove}
        </button>
        <button type="button" className={button} disabled={count === 0 || busy} onClick={onExport}>
          {strings.selection.exportBundle}
        </button>
        <button type="button" className={button} disabled={count === 0 || busy} onClick={onClear}>
          {strings.selection.clear}
        </button>
      </div>
      {note === null ? null : (
        <p aria-live="polite" className="text-[var(--color-muted)]">
          {note}
        </p>
      )}
      {bulk === null || busy || bulk.failures.length === 0 ? null : (
        <div role="alert">
          <p>{strings.selection.failedHeading(bulk.failures.length)}</p>
          <ul role="list" className="mt-1 space-y-1 text-[var(--color-muted)]">
            {bulk.failures.map((failure) => (
              <li key={failure.id}>{strings.selection.failure(failure.name, failure.reason)}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}

/**
 * The grid while its first page is on the way: twelve card-shaped blocks, so the page has its
 * shape before it has its parts and nothing jumps when they land. The sentence is for a screen
 * reader; the blocks say it to everyone else.
 */
export function GridSkeleton({ label }: { label: string }) {
  return (
    <div role="status">
      <span className="sr-only">{label}</span>
      <div aria-hidden="true" className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 max-xs:grid-cols-2 max-xs:gap-2">
        {Array.from({ length: 12 }, (_, index) => (
          <div key={index} className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
            <div className="aspect-square bg-[var(--color-raised)]" />
            <div className="flex flex-col gap-2 p-3">
              <div className="h-3 w-3/4 rounded-sm bg-[var(--color-border)]" />
              <div className="h-2.5 w-1/2 rounded-sm bg-[var(--color-border)]" />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
