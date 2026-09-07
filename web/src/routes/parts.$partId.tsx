import { Link, createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { DEFAULT_LIBRARY_ID, blobUrl, downloadUrl, fetchPartDetail, removePart } from '../lib/api'
import { Detail } from '../components/PartDetail'
import { strings } from '../lib/strings'
import type { Approximate, PartDetail } from '../lib/types'

/**
 * One part, in full — the second route this app has.
 *
 * The grid answers "which of these is the one I want". This answers "is this one right",
 * which wants figures a card has no room for: the box the part has to fit in, whether the
 * mesh is closed, what the file actually is, and the path that is its identity.
 *
 * # Every figure renders through the same component, and that is the point
 *
 * `Figure` takes an `Approximate<T>` and cannot render the value without consulting the
 * flag beside it. `CLAUDE.md`: *"Mesh-derived measurements are labelled 'approximate' in
 * the UI, always."* A page-level badge would be the easy thing and it is wrong from Phase
 * 2 on — a STEP part carries an analytic volume next to a tessellated triangle count on
 * one revision, and one badge is then wrong about one figure whichever way it is set.
 */
export const Route = createFileRoute('/parts/$partId')({
  component: RouteComponent,
})

/**
 * Reads the path param and hands it to `PartPage` as a prop, the same split `index.tsx`
 * makes with its search param: the component that does the work takes plain props, so a
 * test can drive it without spelling a URL.
 */
function RouteComponent() {
  const { partId } = Route.useParams()
  return <PartPage partId={partId} />
}

export function PartPage({ partId }: { partId: string }) {
  const part = useQuery({
    queryKey: ['part', partId],
    queryFn: () => fetchPartDetail(partId),
  })

  return (
    <section>
      <Link
        to="/"
        className="ease-mechanical text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
      >
        {strings.detail.back}
      </Link>
      {part.isPending ? (
        <p className="mt-6 text-[var(--color-muted)]">{strings.detail.loading}</p>
      ) : part.isError ? (
        <p className="mt-6 max-w-prose text-[var(--color-muted)]">{strings.detail.failed}</p>
      ) : (
        <Detail part={part.data} actions={<Remove part={part.data} />} />
      )}
    </section>
  )
}

function Remove({ part }: { part: PartDetail }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const remove = useMutation({
    mutationFn: () => removePart(part.id),
    onSuccess: async () => {
      // Both lists change: this part leaves the grid and joins the removed list. Awaited
      // so the navigation lands on a grid that has already dropped the card, rather than
      // showing it for one frame and then blinking it away.
      await queryClient.invalidateQueries({ queryKey: ['parts', DEFAULT_LIBRARY_ID] })
      await navigate({ to: '/' })
    },
  })

  return (
    <>
      <button
        type="button"
        onClick={() => remove.mutate()}
        disabled={remove.isPending}
        className="ease-mechanical rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {remove.isPending ? strings.removal.removing : strings.removal.remove}
      </button>
      {remove.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.removal.removeFailed}</span>
      ) : null}
    </>
  )
}
