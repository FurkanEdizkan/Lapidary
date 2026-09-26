import { createFileRoute } from '@tanstack/react-router'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import { Clusters } from '../components/Likeness'
import type { LibraryId } from '../lib/types'

/**
 * The review queue: every group of parts in one library that look like duplicates of each
 * other, and the three decisions that take a pair out of it.
 *
 * **Nothing here is stored.** The groups are worked out when the page is read — sorted by size,
 * swept within a 2% band, union-find over the pairs nobody has decided about — so there is no
 * queue table to drain, no row to go stale, and closing the page leaves nothing behind. What
 * *is* stored is a decision: `variant` or `distinct` on a pair, which removes it from every
 * future read of this page.
 *
 * No score is on this page, and none is on the wire. A group says which of the two kinds of
 * alike it is — the same bytes, or alike in shape — and that is the whole vocabulary.
 */
export const Route = createFileRoute('/duplicates')({
  component: RouteComponent,
  /*
    `library` travels in the URL exactly as it does on the grid and the removed list, for the
    same reason: addressing the seeded library directly is a wrong-library bug the moment an
    operator has two of them.

    `since` is a batch's start, put there by the finished upload line. With it the page answers
    about what that batch added; without it, about the whole library. Both are real questions —
    which is why it is a search parameter and not two routes — and neither is validated beyond
    its type: an unreadable timestamp is the server's 400 to give, and inventing a client-side
    parse error here would be a second definition of what a timestamp is.
  */
  validateSearch: (search: Record<string, unknown>): { library?: string; since?: string } => ({
    ...(typeof search.library === 'string' ? { library: search.library } : {}),
    ...(typeof search.since === 'string' ? { since: search.since } : {}),
  }),
})

function RouteComponent() {
  const { library, since } = Route.useSearch()
  return (
    <DuplicatesPage library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID} since={since} />
  )
}

export function DuplicatesPage({ library, since }: { library: LibraryId; since?: string }) {
  return (
    /*
      No `current` place. The header has three places — the grid, Removed, Sharing — and this is
      not a fourth: it is reached from a part, from a finished upload, and from nowhere else.
      Adding it to the nav would spend a permanent slot on a page most visits never need.
    */
    <AppFrame library={library}>
      <section className="max-w-3xl">
        {/* Rendered, not assigned: React 19 hoists it, so the route that owns the page owns its title. */}
        <title>{strings.titles.duplicates}</title>
        <h2 className={HEADLINE}>{strings.likeness.queueTitle}</h2>
        <p className={LEAD}>{strings.likeness.queueLead}</p>
        <Clusters library={library} since={since} />
      </section>
    </AppFrame>
  )
}
