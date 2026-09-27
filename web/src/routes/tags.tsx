import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import {
  LETTER_JUMP_AT,
  OTHER_INITIAL,
  byCount,
  byInitial,
  fetchTagIndex,
  letterId,
} from '../lib/tags'
import type { LibraryId, TagCount } from '../lib/types'

/**
 * Every tag in one library, with how many models carry it — the page a tag can be found from rather
 * than only filtered by.
 *
 * **There is no tag table.** Tags live on the part (`part.tags`, `0022`), so this page is a read and
 * a tag exists exactly as long as a live model carries it. Nothing here can go stale, and nothing
 * has to be cleaned up when the last model with a tag is removed: the tag simply stops being on the
 * list.
 *
 * Not the grid's tag facet. That panel counts within the grid a person has narrowed and stops giving
 * counts past the server's threshold (`docs/DATA.md` §3.4); this counts the whole library and always
 * gives them, because the count is what the list is ordered by.
 */
export const Route = createFileRoute('/tags')({
  component: RouteComponent,
  /*
    `library` travels in the URL as it does on the grid, Removed and the review queue, for the same
    reason: addressing the seeded library directly is a wrong-library bug the moment an operator has
    two of them.

    `order` is here rather than in this browser's storage — unlike the grid's sort, which is a
    preference of how you like to work. This is which of two readings of one list you are looking at,
    and a link to "this library's tags, A to Z" is a thing to send somebody. Anything but `name` is
    the count order, so a mistyped parameter shows the default rather than an error.
  */
  validateSearch: (search: Record<string, unknown>): { library?: string; order?: 'name' } => ({
    ...(typeof search.library === 'string' && search.library.length > 0
      ? { library: search.library }
      : {}),
    ...(search.order === 'name' ? { order: 'name' as const } : {}),
  }),
})

function RouteComponent() {
  const { library, order } = Route.useSearch()
  return (
    <TagsPage library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID} order={order} />
  )
}

export function TagsPage({ library, order }: { library: LibraryId; order?: 'name' }) {
  // One request, and the whole list in it. Every tag has to be reachable from here (P3's "Done
  // when"), so there is nothing to page and no cursor to hold; both orders are then free.
  const tags = useQuery({ queryKey: ['tag-index', library], queryFn: () => fetchTagIndex(library) })
  const byLetter = order === 'name'
  return (
    /*
      No `current` place. The header's places are the grid, the dashboard, Removed and Sharing; this
      is reached from the grid's tag panel and from a tag on a model, which is where somebody is when
      they want it. Nor a reading measure on the list — the lead is prose and keeps its 70ch, the list
      is a directory and fills the window, which is the `/duplicates` finding applied before it could
      happen again.
    */
    <AppFrame library={library} skipTo={{ href: '#tags', label: strings.tagIndex.heading }}>
      {/* Rendered, not assigned: React 19 hoists it, so the route that owns the page owns its title. */}
      <title>{strings.tagIndex.title}</title>
      <div id="tags" tabIndex={-1} className="min-w-0 flex-1">
        <h2 className={HEADLINE}>{strings.tagIndex.heading}</h2>
        <p className={LEAD}>{strings.tagIndex.lead}</p>
        {tags.isError ? (
          <p role="alert" className="mt-6 max-w-[70ch] text-sm text-[var(--color-muted)]">
            {strings.tagIndex.failed}
          </p>
        ) : tags.data === undefined ? null : tags.data.length === 0 ? (
          <p className="mt-6 max-w-[70ch] text-sm text-[var(--color-muted)]">
            {strings.tagIndex.none}
          </p>
        ) : (
          <>
            <Order library={library} order={order} />
            {byLetter ? (
              <ByLetter library={library} tags={tags.data} />
            ) : (
              <TagRows library={library} tags={byCount(tags.data)} />
            )}
          </>
        )}
      </div>
    </AppFrame>
  )
}

/**
 * The two readings of the list, as links rather than as buttons: the order is in the URL, so each one
 * is an address, and a person who wants to send "this library's tags, A to Z" can.
 */
function Order({ library, order }: { library: LibraryId; order?: 'name' }) {
  const search = (next?: 'name') => ({
    ...(library === DEFAULT_LIBRARY_ID ? {} : { library }),
    ...(next === undefined ? {} : { order: next }),
  })
  const quiet =
    'ease-mechanical rounded-[var(--radius-ctl)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'
  const here = `${quiet} bg-[var(--color-raised)] font-semibold text-[var(--color-bright)]`
  return (
    <div role="group" aria-label={strings.tagIndex.order} className="mt-6 flex items-center gap-1">
      {/*
        `exact` covers the search as well as the path, and it has to: both links point at `/tags`, and
        the router's default partial match makes "Most used" — whose search is empty — a subset of
        `?order=name` and therefore current at the same time as "A to Z". Two current links is one
        more than a page has. The marking itself is the router's `aria-current`, not ours.
      */}
      <Link
        to="/tags"
        search={search()}
        activeOptions={{ exact: true, includeSearch: true }}
        className={order === undefined ? here : quiet}
      >
        {strings.tagIndex.byCount}
      </Link>
      <Link
        to="/tags"
        search={search('name')}
        activeOptions={{ exact: true, includeSearch: true }}
        className={order === 'name' ? here : quiet}
      >
        {strings.tagIndex.byName}
      </Link>
    </div>
  )
}

/**
 * A to Z, in letter groups, with a jump bar once the list is long enough that scrolling stops working.
 * The groups are built from the tags rather than from an alphabet, so a letter nothing starts with
 * gets no heading and the bar has no dead links in it.
 */
function ByLetter({ library, tags }: { library: LibraryId; tags: readonly TagCount[] }) {
  const groups = byInitial(tags)
  const jump = tags.length > LETTER_JUMP_AT
  return (
    <>
      {!jump ? null : (
        <nav aria-label={strings.tagIndex.jump} className="mt-4 flex flex-wrap gap-x-1 gap-y-0.5">
          {groups.map(({ initial }) => (
            <a
              key={initial}
              href={`#${letterId(initial)}`}
              className="ease-mechanical tabular min-h-6 min-w-6 rounded-sm px-1 text-center text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] hover:text-[var(--color-bright)]"
            >
              {initial}
            </a>
          ))}
        </nav>
      )}
      {groups.map(({ initial, tags: group }) => (
        <section key={initial} aria-labelledby={letterId(initial)} className="mt-6">
          <h3
            id={letterId(initial)}
            className="mb-2 border-b border-[var(--color-border)] pb-1 text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase"
          >
            {initial === OTHER_INITIAL ? strings.tagIndex.otherInitial : initial}
          </h3>
          <TagRows library={library} tags={group} />
        </section>
      ))}
    </>
  )
}

/**
 * The tags themselves. An auto-filling grid rather than one column: a directory of a few hundred
 * short names in a single column is a page nobody reaches the bottom of, and at 1440 a single column
 * would leave two thirds of the window empty. Under one column's width it collapses to one, with no
 * horizontal scroll, because the track is a minimum and a fraction.
 *
 * No `aria-label` on the row. The whole name a screen reader announces is the tag and its count, and
 * both are visible text — which is what WCAG 2.5.3 asks for and what a label of our own would risk
 * disagreeing with.
 */
function TagRows({ library, tags }: { library: LibraryId; tags: readonly TagCount[] }) {
  return (
    <ul
      role="list"
      className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-x-6 gap-y-0.5"
    >
      {tags.map(({ value, count }) => (
        <li key={value}>
          <Link
            to="/tags/$tag"
            params={{ tag: value }}
            search={library === DEFAULT_LIBRARY_ID ? {} : { library }}
            className="ease-mechanical flex min-h-7 items-baseline justify-between gap-3 rounded-sm px-2 text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] hover:text-[var(--color-bright)]"
          >
            <span className="min-w-0 break-words">{value}</span>
            {/*
              A space, because the accessible name is these two spans' text run together and
              "28 mm41 models" is what a screen reader would otherwise read out. Chrome inserts one
              between flex children and the name computation these tests use does not, so the one
              thing not to do is depend on which engine is asking.
            */}
            {' '}
            <span className="tabular flex-none text-xs text-[var(--color-muted)]">
              {strings.tagIndex.models(count)}
            </span>
          </Link>
        </li>
      ))}
    </ul>
  )
}
