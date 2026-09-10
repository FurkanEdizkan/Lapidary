import { Link, createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  DEFAULT_LIBRARY_ID,
  fetchParts,
  purgePart,
  restorePart,
} from '../lib/api'
import { strings } from '../lib/strings'
import { TopBar } from '../components/TopBar'
import type { LibraryId, PartCard } from '../lib/types'

/**
 * The parts a person removed, and the only route back to any of them.
 *
 * Without this page, removing would be a one-way door: every other read path filters
 * `deleted_at`, so a removed part has no page, no card and no id anyone could type. The
 * product rule is that we never delete implicitly — a removal nobody can find again is a
 * deletion with extra steps.
 *
 * It is also the only place purge is reachable from, which is what makes purge "separate
 * and explicit" in the interface and not just in the API. Getting here is a deliberate act;
 * the library has no purge button anywhere.
 */
export const Route = createFileRoute('/removed')({
  component: RouteComponent,
  /*
    The library travels in the URL, exactly as it does on the grid.

    This page addressed `DEFAULT_LIBRARY_ID` directly, which was right while there was one
    library and became a wrong-library bug the moment there could be two: an operator who
    removed a part from their second library came here and was shown the first library's
    removed parts, with no indication the page was answering about somewhere else.
  */
  validateSearch: (search: Record<string, unknown>): { library?: string } =>
    typeof search.library === 'string' ? { library: search.library } : {},
})

function RouteComponent() {
  const { library } = Route.useSearch()
  return <RemovedPage library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID} />
}

export function RemovedPage({ library }: { library: LibraryId }) {
  // Not paged. A removed list that needs paging is a library someone has emptied, which is
  // not the case this page is for — and `MAX_LIMIT` on the route caps it regardless. When
  // that stops being true it wants the grid's `useInfiniteQuery`, not a second pager here.
  const removed = useQuery({
    queryKey: ['parts', library, 'removed'],
    queryFn: () => fetchParts(library, undefined, 'removed'),
  })

  return (
    <>
      {/*
        Rendered, not assigned. React 19 hoists a `<title>` into the head from wherever it
        is written and removes it on unmount, so the route that owns the page owns its
        title — and `index.html`'s static one stays as the pre-hydration fallback. SC 2.4.2.
      */}
      <title>{strings.titles.removed}</title>
      {/*
        The bar, which `__root` used to draw for every route and now does not — see there
        for why. `sidebar={null}`: this page has no rail, and a toggle that claims to hide
        something absent is a control that lies about the screen it is on.
      */}
      <TopBar library={library} sidebar={null} />
      <section className="px-[18px] py-[13px]">
      <Link
        to="/"
        className="ease-mechanical text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
      >
        {strings.removal.backToLibrary}
      </Link>
      <h2 className="mt-4 text-xl font-medium">{strings.removal.removedTitle}</h2>
      <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
        {strings.removal.removedLead}
      </p>

      {removed.isPending ? (
        <p className="mt-6 text-[var(--color-muted)]">{strings.detail.loading}</p>
      ) : removed.isError ? (
        <p className="mt-6 max-w-prose text-[var(--color-muted)]">{strings.detail.failed}</p>
      ) : removed.data.parts.length === 0 ? (
        <p className="mt-6 text-[var(--color-muted)]">{strings.removal.removedEmpty}</p>
      ) : (
        <>
          <p className="mt-6 text-sm text-[var(--color-muted)]">
            {strings.removal.removedCount(removed.data.parts.length)}
          </p>
          <ul role="list" className="mt-3 flex flex-col gap-2">
            {removed.data.parts.map((card) => (
              <RemovedRow key={card.id} card={card} />
            ))}
          </ul>
        </>
      )}
      </section>
    </>
  )
}

/**
 * One removed part, with both of the things that can still happen to it.
 *
 * The two buttons are deliberately not alike. Restore is the ordinary one and reads as
 * ordinary; purge names the part in a confirmation and says what survives it and for how
 * long. `CLAUDE.md` requires that difference in wording, and the reason it matters is that
 * these two sit a few pixels apart.
 */
function RemovedRow({ card }: { card: PartCard }) {
  const queryClient = useQueryClient()
  // Both mutations change both lists — the library's and this one — so both invalidate the
  // shared prefix rather than only the key they were read from. A restore that refreshed
  // this page alone would leave the grid missing the part it just brought back.
  // The card's own library, not the seeded one — `PartCard` carries it, so a restore in the
  // second library refreshes the second library's grid.
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['parts', card.library] })

  const restore = useMutation({
    mutationFn: () => restorePart(card.id),
    onSuccess: refresh,
  })
  const purge = useMutation({
    mutationFn: () => purgePart(card.id),
    onSuccess: refresh,
  })

  return (
    <li className="flex flex-wrap items-center gap-3 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <span className="grow">
        <span className="text-sm">{card.name}</span>
        <span className="ml-2 text-xs text-[var(--color-muted)]">
          {card.sourcePath}
        </span>
      </span>

      {/*
        The purge result is reported where the row is, and in the words `strings.removal`
        fixes: how many files are being kept and for how long. Never "freed" — nothing is
        freed today, and the row it describes has already gone from the list.
      */}
      {purge.isSuccess ? (
        <span className="text-xs text-[var(--color-muted)]">
          {purge.data.quarantined === 0
            ? strings.removal.purgedNothing
            : strings.removal.purgedQuarantined(purge.data.quarantined, purge.data.quarantinedBytes)}
        </span>
      ) : null}
      {restore.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.removal.restoreFailed}</span>
      ) : null}
      {purge.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.removal.purgeFailed}</span>
      ) : null}

      <button
        type="button"
        onClick={() => restore.mutate()}
        disabled={restore.isPending || purge.isPending}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {restore.isPending ? strings.removal.restoring : strings.removal.restore}
      </button>
      <button
        type="button"
        onClick={() => {
          // The one confirmation in the app, and it names the part. A dialog that says
          // "are you sure?" is a dialog people clear without reading; this one has to be
          // read to know which part it is about.
          if (window.confirm(strings.removal.purgeConfirm(card.sourcePath))) {
            purge.mutate()
          }
        }}
        disabled={restore.isPending || purge.isPending}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {purge.isPending ? strings.removal.purging : strings.removal.purge}
      </button>
    </li>
  )
}
