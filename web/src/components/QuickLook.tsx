import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { downloadUrl, fetchPartDetail } from '../lib/api'
import { flipFrom } from '../lib/flip'
import { Dialog } from './Dialog'
import { ShowInFolder } from './ShowInFolder'
import { Detail, Preview } from './PartDetail'
import { Figure } from './Figure'
import { strings } from '../lib/strings'
import type { PartCard, PartId } from '../lib/types'
import { Icon } from './Icon'

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
          <Icon name="close" />
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

/** Keys that belong to a control: a stage's arrows must not steal them from a field or a toolbar. */
function ownsArrows(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || target.closest('input, select, textarea, [role="toolbar"]') !== null)
  )
}

/**
 * The part on a stage: Space on a card's name.
 *
 * The side pane and its dialog are for reading a part; this is for looking at it. The live 3D view
 * takes most of the window, on the lamp's ground, with only what a decision needs beside it: the
 * part number, its figures, Download and the way to the full page. The arrow keys step to the next
 * card and the one before, so a library can be walked part by part without leaving the stage, and
 * Escape puts focus back on the card now on screen, scrolled into view.
 *
 * The render flies in from its card the first time (`flipFrom`, the quick look's authored moment);
 * a step swaps the part in place.
 */
export function StageLook({
  parts,
  index,
  from,
  onStep,
  onClose,
}: {
  parts: readonly PartCard[]
  index: number
  /** Where the first part's render sat on the grid; a zero rectangle after a step. */
  from: DOMRect
  onStep: (index: number) => void
  onClose: () => void
}) {
  const part = parts[index]
  const detail = useQuery({
    queryKey: ['part', part?.id],
    queryFn: () => fetchPartDetail(part!.id),
    enabled: part !== undefined,
  })
  const stage = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const image = stage.current?.querySelector('img')
    if (image != null) flipFrom(image, from)
  }, [detail.data, from])

  const step = useRef({ index, count: parts.length, onStep })
  useEffect(() => {
    step.current = { index, count: parts.length, onStep }
  })
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return
      if (event.defaultPrevented || ownsArrows(event.target)) return
      const { index: at, count, onStep: go } = step.current
      const next = at + (event.key === 'ArrowRight' ? 1 : -1)
      if (next < 0 || next >= count) return
      event.preventDefault()
      go(next)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  if (part === undefined) return null
  const data = detail.data?.id === part.id ? detail.data : undefined
  return (
    <Dialog
      title={part.name}
      size="stage"
      onClose={onClose}
      returnFocus={() => document.getElementById(`part-name-${part.id}`)?.querySelector('a') ?? null}
    >
      <div className="mt-3 grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div ref={stage} className="flex min-h-[18rem] min-w-0">
          {data === undefined ? (
            <div className="stage-lamp grid w-full place-items-center rounded-md">
              <p className="text-sm text-[var(--color-muted)]">
                {detail.isError ? strings.quickLook.failed : strings.quickLook.loading}
              </p>
            </div>
          ) : (
            <Preview part={data} stage />
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-4 text-sm">
          <p className="tabular text-xs text-[var(--color-muted)]">
            {strings.quickLook.position(index + 1, parts.length)}
          </p>
          {part.partNumber === null ? null : (
            <p className="tabular text-[var(--color-dim)]">{part.partNumber}</p>
          )}
          {data === undefined ? null : (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 border-t border-[var(--color-border)] pt-3">
              <dt className="text-[var(--color-muted)]">{strings.detail.triangles}</dt>
              <dd className="tabular">
                {data.triangleCount === null ? strings.detail.unknown : strings.detail.trianglesValue(data.triangleCount)}
              </dd>
              <dt className="text-[var(--color-muted)]">{strings.detail.boundingBox}</dt>
              <dd className="tabular">
                {data.bboxMm === null ? (
                  strings.detail.unknown
                ) : (
                  <Figure figure={data.bboxMm} render={(mm) => strings.detail.boundingBoxValue(mm)} />
                )}
              </dd>
              <dt className="text-[var(--color-muted)]">{strings.detail.volume}</dt>
              <dd className="tabular">
                {data.volumeMm3 === null ? (
                  data.isWatertight === false ? strings.detail.volumeUnavailable : strings.detail.unknown
                ) : (
                  <Figure figure={data.volumeMm3} render={strings.detail.volumeValue} />
                )}
              </dd>
            </dl>
          )}
          {/*
            The key, in words, whenever a figure here carries the mark: CLAUDE.md labels a mesh-derived
            measurement approximate, always, and a lone ≈ is a symbol a reader has to already know.
          */}
          {data !== undefined && [data.bboxMm, data.volumeMm3].some((figure) => figure?.approximate === true) ? (
            <p className="text-xs text-[var(--color-muted)]">
              <span aria-hidden="true" className="mr-1">
                {strings.detail.approximate}
              </span>
              {strings.detail.approximateKey}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            {data === undefined || data.sourceHash === null ? null : (
              <a
                href={downloadUrl(data.revision)}
                download
                className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 py-1.5 font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
              >
                {strings.download.original}
              </a>
            )}
            <Link
              to="/parts/$partId"
              params={{ partId: part.id }}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px hover:text-[var(--color-text)]"
            >
              {strings.quickLook.fullPage}
            </Link>
          </div>
          <p className="mt-auto text-xs text-[var(--color-muted)] max-md:hidden">{strings.quickLook.steps}</p>
        </div>
      </div>
    </Dialog>
  )
}
