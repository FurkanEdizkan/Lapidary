import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { DEFAULT_LIBRARY_ID, fetchHealth, fetchParts } from '../lib/api'
import { strings } from '../lib/strings'
import type { PartCard, PartsPage } from '../lib/types'

export const Route = createFileRoute('/')({ component: Index })

export function Index() {
  const health = useQuery({ queryKey: ['health'], queryFn: fetchHealth })
  const parts = useQuery({
    queryKey: ['parts', DEFAULT_LIBRARY_ID],
    queryFn: () => fetchParts(DEFAULT_LIBRARY_ID),
  })

  return (
    <section>
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
