import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { fetchPartDetail } from '../lib/api'
import { flipFrom } from '../lib/flip'
import { Dialog } from './Dialog'
import { ShowInFolder } from './ShowInFolder'
import { Detail } from './PartDetail'
import { strings } from '../lib/strings'
import type { PartCard, PartId } from '../lib/types'

/** Where the quick look sits beside the grid rather than over it: 1280px and up. */
const PANE_QUERY = '(min-width: 80rem)'

/**
 * Whether the screen is wide enough for the pane. `false` where `matchMedia` does not exist —
 * jsdom, which is where every test that expects the dialog runs.
 */
export function useWide(): boolean {
  const [wide, setWide] = useState(
    () => typeof window.matchMedia === 'function' && window.matchMedia(PANE_QUERY).matches,
  )
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const query = window.matchMedia(PANE_QUERY)
    const onChange = () => setWide(query.matches)
    query.addEventListener?.('change', onChange)
    return () => query.removeEventListener?.('change', onChange)
  }, [])
  return wide
}

/**
 * The quick look beside the grid.
 *
 * Not modal, so it traps nothing: the grid stays reachable and the next card swaps what this
 * shows. Opening moves focus to Close, the pane's first control, rather than to its heading:
 * the application's one focus ring is unlayered and cannot be taken off a heading, and a ring
 * round text points at nothing a key can operate — `Dialog` moved off its box for the same
 * reason. The pane is labelled by the part's name, so arriving inside it still announces the
 * part. Escape closes it, but not out from under a dialog opened on top of it, whose own
 * Escape comes first.
 */
function QuickLookPane({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  const titleId = useId()
  const closeButton = useRef<HTMLButtonElement>(null)
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })
  useEffect(() => {
    closeButton.current?.focus()
  }, [title])
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      if (document.querySelector('[aria-modal="true"]') !== null) return
      close.current()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])
  return (
    <aside
      aria-labelledby={titleId}
      className="panel-in sticky top-4 max-h-[calc(100vh-2rem)] w-[26rem] shrink-0 overflow-y-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
    >
      <div className="flex items-start justify-between gap-4">
        <h2 id={titleId} className="text-sm font-medium">
          {title}
        </h2>
        <button
          ref={closeButton}
          type="button"
          onClick={onClose}
          aria-label={strings.dialog.close}
          className="ease-mechanical -m-1 flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="none"
               stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
      {children}
    </aside>
  )
}

/**
 * The part, in a panel, without leaving the grid.
 *
 * Scanning a library means looking at one part and then the next, and a round trip through
 * a full page and the back button for each of them is what makes that tiring. So this is a
 * look; the page is where the controls that change something live, and where a URL someone
 * can share lives.
 *
 * **It renders `Detail`, the detail page's own article, rather than a version of it.** Two
 * renderings of one measurement that can disagree is a defect here, and measurements are
 * the case that matters: every figure goes through `Figure`, which cannot render a value
 * without its `approximate` flag, because a mesh-derived number must be labelled wherever
 * it appears.
 *
 * **And it fetches under the detail route's own query key**, so opening the panel and then
 * the page costs one request rather than two — the look warms the cache for the page it
 * links to.
 */
export function QuickLook({
  part,
  from,
  hostRoot,
  busy,
  onRender,
  onMove,
  onClose,
  pane,
}: {
  part: PartCard
  /** Where this part's render sat on the grid when it was clicked. */
  from: DOMRect
  hostRoot: string | null
  busy: boolean
  onRender: (id: PartId) => void
  /** `null` for a model still in the shared store, which has no directory to rename. */
  onMove: (() => void) | null
  onClose: () => void
  /** Beside the grid on a wide screen; over it, in a dialog, otherwise. */
  pane: boolean
}) {
  const detail = useQuery({
    queryKey: ['part', part.id],
    queryFn: () => fetchPartDetail(part.id),
  })
  const panel = useRef<HTMLDivElement>(null)
  /*
    The authored moment, and the only one in the application.

    `useLayoutEffect` and not `useEffect`: the render has to be measured and moved in the
    same frame it is painted, or it lands at its destination first and then jumps back to
    begin the flight. Keyed on the detail arriving, because the image does not exist until
    then — the panel is open and empty for as long as the fetch takes.
  */
  useLayoutEffect(() => {
    const image = panel.current?.querySelector('img')
    if (image != null) flipFrom(image, from)
  }, [detail.data, from])
  const Frame = pane ? QuickLookPane : Dialog
  return (
    <Frame title={part.name} onClose={onClose}>
      <div ref={panel}>
      {detail.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.quickLook.loading}</p>
      ) : detail.isError ? (
        <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.quickLook.failed}
        </p>
      ) : (
        <Detail
          titled
          part={detail.data}
          /*
            The tools the card used to carry. They are here because the card is a picture and
            a name now: a control that hides the render it sits on is a control fighting the
            one job this product has. Owner decision, 2026-09-08 — click gives you the
            essential information and the tools; the full page gives you depth.
          */
          actions={
            <>
              <button
                type="button"
                onClick={() => onRender(part.id)}
                disabled={busy}
                aria-label={strings.render.partFor(part.name)}
                className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
              >
                {strings.render.part}
              </button>
              {onMove === null ? (
                <span className="text-xs text-[var(--color-muted)]">
                  {strings.folders.notMigrated}
                </span>
              ) : (
                <button
                  type="button"
                  onClick={onMove}
                  aria-label={strings.folders.moveToFor(part.name)}
                  className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
                >
                  {strings.folders.moveTo}
                </button>
              )}
            </>
          }
        />
      )}
      </div>
      <ShowInFolder part={part} hostRoot={hostRoot} />
      <div className="mt-4 flex justify-end gap-2">
        <Link
          to="/parts/$partId"
          params={{ partId: part.id }}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.quickLook.fullPage}
        </Link>
      </div>
    </Frame>
  )
}
