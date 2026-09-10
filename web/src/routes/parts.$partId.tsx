import { Link, createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  DEFAULT_LIBRARY_ID,
  blobUrl,
  downloadUrl,
  fetchInstanceStorage,
  fetchPartDetail,
  removePart,
  renderPartThumbnail,
} from '../lib/api'
import { Detail } from '../components/PartDetail'
import { MovePartDialog } from '../components/FolderTree'
import { ShowInFolder } from '../components/ShowInFolder'
import { strings } from '../lib/strings'
import { TopBar } from '../components/TopBar'
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
    <>
      {/*
        The bar, which `__root` used to draw for every route and now does not — see there
        for why. `sidebar={null}`: this page has no rail to toggle.

        The library comes off the part once the fetch lands, and falls back to the seeded
        one until it does. That fallback only decides where the bar's own tabs point for the
        moment before the part arrives — it is never used to fetch anything, which is the
        thing it would be wrong for.
      */}
      <TopBar library={part.data?.library ?? DEFAULT_LIBRARY_ID} sidebar={null} />
      <section className="px-[18px] py-[13px]">
      {/*
        The part's own name, once the fetch has it. Rendered rather than assigned: React 19
        hoists a `<title>` from wherever it is written and removes it on unmount, so this
        needs neither a router head option nor a loader — which is what it would have taken
        to get the name into the title, since this page fetches inside the component.

        SC 2.4.2, Level A. `null` while it loads, because a title is not a place to guess:
        a tab that says `LP-1042-03` before the page knows the name would be lying on the
        one part page that turns out to 404.
      */}
      <title>{strings.titles.part(part.data?.name ?? null)}</title>
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
        <Detail
          part={part.data}
          // The page, not the dialog, is where a form that takes typing belongs — the same
          // line `actions` draws below.
          recordable
          actions={
            <>
              {/*
                Render and Move live here as well as in the grid's panel, and that is not
                duplication for its own sake.

                The panel opens on a click of the tile and the tile has no keyboard path to
                it, so for a while these two controls — and the storage path below — existed
                nowhere a keyboard could reach. That is WCAG 2.2 SC 2.1.1, Level A, and it is
                about whether a *function* is available at all, not about which surface
                offers it. This page is the surface a keyboard reaches: the card's name is a
                real link, and it comes here.
              */}
              <PartTools part={part.data} />
              <Remove part={part.data} />
              {/*
                The reassurance sits beside the button rather than behind a confirmation
                dialog. Removing is reversible and touches nothing on disk, so a modal would
                spend on this action the alarm that purge is going to need — and purge is one
                deliberate step further away, on the removed list this sends you to.

                Inside `actions` and not inside `Detail`, because it is a sentence about a
                control: the grid's quick-look shows the same article without the remove
                button, and it was telling people they could restore something from a panel
                that offers no way to remove it.
              */}
              <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
                {strings.removal.removeHint}
              </p>
            </>
          }
        />
      )}
      </section>
    </>
  )
}

/**
 * The per-part actions that are not destructive: render a preview, file it in a category.
 *
 * Download lives in `Detail` already and Remove is beside this in the page's `actions`,
 * deliberately kept out of here — this component is the pair of controls the grid's panel
 * carries, put where a keyboard can reach them.
 */
function PartTools({ part }: { part: PartDetail }) {
  const queryClient = useQueryClient()
  const [moving, setMoving] = useState(false)
  // The host's own view of the store, so the path this page prints is one a person can
  // paste. `false` because the walk behind `onDisk` is the expensive figure and this page
  // wants only the root.
  const instance = useQuery({
    queryKey: ['instance-storage', false],
    queryFn: () => fetchInstanceStorage(false),
  })
  const render = useMutation({
    mutationFn: () => renderPartThumbnail(part.id),
    // The preview arrives through the worker, so there is nothing to refetch here except
    // this part — the grid picks its own up on the next poll.
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['part', part.id] }),
  })

  return (
    <>
      <button
        type="button"
        onClick={() => render.mutate()}
        disabled={render.isPending}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.render.part}
      </button>
      <button
        type="button"
        onClick={() => setMoving(true)}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.folders.moveTo}
      </button>
      {render.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.render.queueFailed}</span>
      ) : null}
      <ShowInFolder part={part} hostRoot={instance.data?.hostStorageRoot ?? null} />
      {moving ? (
        <MovePartDialog
          part={{ id: part.id, name: part.name }}
          library={part.library}
          onClose={() => setMoving(false)}
        />
      ) : null}
    </>
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
      // This part's own library, not the seeded one. `PartDetail` carries it, so the key
      // needs no new field — the hard-coded id was simply the wrong library on any
      // deployment with more than one.
      await queryClient.invalidateQueries({ queryKey: ['parts', part.library] })
      await navigate({ to: '/' })
    },
  })

  return (
    <>
      <button
        type="button"
        onClick={() => remove.mutate()}
        disabled={remove.isPending}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {remove.isPending ? strings.removal.removing : strings.removal.remove}
      </button>
      {remove.isError ? (
        <span className="text-xs text-[var(--color-muted)]">{strings.removal.removeFailed}</span>
      ) : null}
    </>
  )
}
