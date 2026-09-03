import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { DEFAULT_LIBRARY_ID, fetchBatchStatus, fetchHealth, fetchParts } from '../lib/api'
import { strings } from '../lib/strings'
import type { BatchStatus, PartCard, PartsPage } from '../lib/types'

export const Route = createFileRoute('/')({
  component: RouteComponent,
  /**
   * `?batch=<id>` — the batch a scan returned, so this page can watch it drain.
   *
   * The id arrives in the URL rather than from a mutation this page issued, because this
   * page cannot start a scan: `POST /api/libraries/{id}/scan` is mounted under the worker
   * role only (port 8081), while `deploy/web/Caddyfile` and vite's dev proxy both forward
   * `/api/*` to the api service. Proxying the worker to the browser instead would put the
   * public web surface inside the boundary the api/worker split exists to hold. So the
   * operator who ran the scan opens `/?batch=<id>`. A scan the browser can start belongs
   * with the upload path, which is a later slice.
   *
   * Without `validateSearch` the search params are not typed at all and `useSearch()`
   * hands back nothing, so the poll below would simply never enable — silently, and
   * identically to there being no scan.
   */
  validateSearch: (search: Record<string, unknown>): { batch?: string } => {
    const batch = search.batch
    return typeof batch === 'string' && batch.length > 0 ? { batch } : {}
  },
})

/**
 * Reads the search param and hands it to `Index` as a prop. `Index` takes the batch
 * rather than calling `useSearch` itself so it stays renderable without a router — which
 * is how `index.test.tsx` renders it.
 */
function RouteComponent() {
  const { batch } = Route.useSearch()
  return <Index batch={batch} />
}

/** Files the worker is finished with, however it finished with them. */
function filesSettled(status: BatchStatus): number {
  return status.ingested + status.skipped + status.failedTotal
}

export function Index({ batch }: { batch?: string }) {
  const queryClient = useQueryClient()
  const health = useQuery({ queryKey: ['health'], queryFn: fetchHealth })
  const parts = useQuery({
    queryKey: ['parts', DEFAULT_LIBRARY_ID],
    queryFn: () => fetchParts(DEFAULT_LIBRARY_ID),
  })
  const scan = useQuery({
    queryKey: ['batch', DEFAULT_LIBRARY_ID, batch],
    queryFn: () => fetchBatchStatus(DEFAULT_LIBRARY_ID, batch as string),
    enabled: batch !== undefined,
    // The poll stops itself. A batch that finishes while the tab is backgrounded must not
    // leave a closed laptop asking about a completed scan forever — spec §11's last risk,
    // which is easy to forget and so has its own test.
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })

  // The grid is a separate query with its own cache, and nothing else would tell it the
  // library changed underneath it while the worker commits parts. Keyed on files settled
  // rather than on the poll tick, so a second in which nothing finished costs no refetch.
  const settled = scan.data === undefined ? 0 : filesSettled(scan.data)
  useEffect(() => {
    if (settled > 0) {
      void queryClient.invalidateQueries({ queryKey: ['parts', DEFAULT_LIBRARY_ID] })
    }
  }, [settled, queryClient])

  return (
    <section>
      {batch === undefined ? null : (
        <ScanProgress status={scan.data} isError={scan.isError} />
      )}
      {parts.isPending ? (
        <p className="text-[var(--color-muted)]">{strings.parts.loading}</p>
      ) : parts.isError ? (
        <p className="max-w-prose text-[var(--color-muted)]">{strings.parts.failed}</p>
      ) : parts.data.parts.length === 0 ? (
        // An empty page and a page still in flight are different facts, so only a page
        // that came back empty gets the empty state.
        <EmptyLibrary />
      ) : (
        <>
          <Grid parts={parts.data.parts} />
          <PageExtent page={parts.data} />
        </>
      )}
      <p className="mt-6 text-sm text-[var(--color-muted)]">
        {health.isPending
          ? strings.health.checking
          : health.isError
            ? strings.health.failed
            : strings.health.ok(health.data.database.major)}
      </p>
    </section>
  )
}

/**
 * The scan line: how far a batch has got, and how it ended.
 *
 * Nothing renders while the first poll is in flight. A batch whose status has not
 * arrived yet is not a fact about the library, and the grid below is the page — a
 * placeholder here would push it down for one tick and then move it back.
 */
function ScanProgress({ status, isError }: { status?: BatchStatus; isError: boolean }) {
  if (isError) {
    return <p className="mb-4 max-w-prose text-[var(--color-muted)]">{strings.scan.unknown}</p>
  }
  if (status === undefined) {
    return null
  }
  return (
    <p className="mb-4 flex max-w-prose flex-wrap gap-2 text-[var(--color-muted)]">
      <span>
        {status.finishedAt === null
          ? strings.scan.running(filesSettled(status), status.total)
          : strings.scan.finished(status.ingested, status.skipped)}
      </span>
      {status.failedTotal === 0 ? null : <span>{strings.scan.failed(status.failedTotal)}</span>}
    </p>
  )
}

function EmptyLibrary() {
  return (
    <div className="max-w-prose">
      <h2 className="text-lg">{strings.emptyLibrary.title}</h2>
      <p className="mt-2 text-[var(--color-muted)]">{strings.emptyLibrary.body}</p>
    </div>
  )
}

/**
 * How much of the library is actually on screen.
 *
 * `fetchParts` asks for one page and renders it; the server caps a page at 50 parts by
 * default. So a library of 200 shows 50 cards, and without this line nothing on the page
 * says so — a user scans the grid, does not find the part they came for, and concludes it
 * was never ingested. Virtualized scrolling is a later slice; being honest about the
 * truncation is not something to defer along with it.
 *
 * `next` is the server's own answer to "is there more", not a length comparison against a
 * limit this component would otherwise have to know: a full page hands back a cursor, a
 * short one hands back null.
 */
function PageExtent({ page }: { page: PartsPage }) {
  return (
    <p className="mt-4 max-w-prose text-xs text-[var(--color-muted)]">
      {page.next === null
        ? strings.parts.showingAll(page.parts.length)
        : strings.parts.showingFirstPage(page.parts.length)}
    </p>
  )
}

function Grid({ parts }: { parts: readonly PartCard[] }) {
  return (
    <ul className="grid list-none grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4">
      {parts.map((part) => (
        <li key={part.id}>
          <Card part={part} />
        </li>
      ))}
    </ul>
  )
}

function Card({ part }: { part: PartCard }) {
  const nameId = `part-name-${part.id}`
  return (
    <article
      aria-labelledby={nameId}
      className="ease-mechanical flex h-full flex-col overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-0.5"
    >
      <div className="flex aspect-square items-center justify-center bg-[var(--color-bg)]">
        {part.thumbnail === null ? (
          // Never an <img> with an empty src: a broken-image glyph reads as a failure,
          // and "the worker has not rasterized this yet" is not one.
          <span className="text-xs text-[var(--color-muted)]">{strings.parts.noThumbnail}</span>
        ) : (
          <img
            src={part.thumbnail}
            alt={strings.parts.thumbnailAlt(part.name)}
            className="h-full w-full object-contain"
          />
        )}
      </div>
      <div className="flex flex-1 flex-col gap-1 p-3">
        <h2 id={nameId} className="text-sm leading-snug">
          {part.name}
        </h2>
        {part.partNumber === null ? null : (
          <p className="font-mono text-xs text-[var(--color-muted)]">{part.partNumber}</p>
        )}
        <Measurements part={part} />
      </div>
    </article>
  )
}

/**
 * The card's measurement line, rendered as one indivisible unit.
 *
 * A triangle count is tessellation-derived by construction, so a card showing one is
 * showing a mesh-derived figure whatever the wire's `approximate` says. CLAUDE.md
 * forbids such a figure appearing unlabelled, so the label is not a sibling conditional
 * that the count can drift away from: either the whole line renders or none of it does,
 * and within it the badge is unconditional. No branch here can emit a count without a
 * label, which is the difference between the rule holding and the rule happening to
 * hold because the ingest path currently sets the flag to a constant.
 *
 * The line still renders for a part with no count but the flag set, because the flag
 * means *any* figure on this part is mesh-derived — not that this count is.
 */
function Measurements({ part }: { part: PartCard }) {
  // Narrowed with typeof rather than compared to null: the binding says `number | null`,
  // but the response is cast rather than validated, so a field that disappears upstream
  // arrives here as undefined and would reach .toLocaleString() as one.
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  if (!part.approximate && count === null) {
    return null
  }
  return (
    <p className="mt-auto flex flex-wrap items-center gap-2 pt-2 text-xs text-[var(--color-muted)]">
      {count === null ? null : <span>{strings.parts.triangles(count)}</span>}
      <span
        title={strings.parts.approximateDetail}
        className="rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[0.65rem] tracking-wider uppercase"
      >
        {strings.parts.approximate}
      </span>
    </p>
  )
}
