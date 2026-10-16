import { Link, createFileRoute, useNavigate } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import {
  blobUrl,
  downloadUrl,
  fetchInstanceStorage,
  fetchPartDetail,
  removePart,
} from '../lib/api'
import { Detail } from '../components/PartDetail'
import { MovePartDialog } from '../components/FolderTree'
import { ShowInFolder, hostRootsOf } from '../components/ShowInFolder'
import { Menu } from '../components/Menu'
import { closeMenu } from '../components/Dialog'
import { ConfirmDelete } from '../components/ConfirmDelete'
import { strings } from '../lib/strings'
import { AppFrame } from '../components/AppFrame'
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
    <AppFrame>
      <section>
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
            layout="page"
            actions={<PartActions part={part.data} />}
          />
        )}
      </section>
    </AppFrame>
  )
}

/** A row in the page's ⋯ menu: a full-width target, at least 32px tall. */
const ITEM =
  'ease-mechanical flex min-h-8 w-full items-center rounded-[var(--radius-ctl)] px-2 text-left text-sm text-[var(--color-text)] duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] disabled:opacity-50'

/**
 * Everything a part's page can do to the part besides downloading it, behind one ⋯ button.
 *
 * Download is the page's one standing control, because getting the file is what a person came
 * for; Move, the storage path and Delete are occasional, and seven buttons in a row made
 * every one of them look as important as Download.
 *
 * They were on the grid's panel alone once, which a keyboard could not open, so for a while they
 * existed nowhere a keyboard could reach — WCAG 2.2 SC 2.1.1, Level A, about whether a function is
 * available at all. This page is the surface a keyboard reaches (the card's name is a real link),
 * and a native popover menu is reachable: Tab to ⋯, Enter, and the rows are in the tab order.
 *
 * A failure is said beside the menu, not inside it: a message in a closed menu is one nobody reads.
 */
function PartActions({ part }: { part: PartDetail }) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const [moving, setMoving] = useState(false)
  const [confirming, setConfirming] = useState(false)
  // The host's own view of the store, so the path this page prints is one a person can
  // paste. `false` because the walk behind `onDisk` is the expensive figure and this page
  // wants only the root.
  const instance = useQuery({
    queryKey: ['instance-storage', false],
    queryFn: () => fetchInstanceStorage(false),
  })
  const remove = useMutation({
    mutationFn: () => removePart(part.id),
    onSuccess: async () => {
      // The part no longer exists, so this page has nothing left to show: back to the grid.
      // Awaited so the navigation lands on a grid that has already dropped the card, rather
      // than showing it for one frame and then blinking it away.
      await queryClient.invalidateQueries({ queryKey: ['parts', part.library] })
      queryClient.removeQueries({ queryKey: ['part', part.id] })
      await navigate({ to: '/' })
    },
  })

  return (
    <>
      <Menu id="part-menu" label={strings.detail.more} icon="more">
        <div className="flex flex-col gap-0.5">
          <button
            type="button"
            onClick={(event) => {
              closeMenu(event.currentTarget)
              setMoving(true)
            }}
            className={ITEM}
          >
            {strings.folders.moveTo}
          </button>
        </div>
        <ShowInFolder part={part} hostRoots={hostRootsOf(instance.data)} />
        <div className="border-t border-[var(--color-border)] pt-2">
          <button
            type="button"
            onClick={(event) => {
              closeMenu(event.currentTarget)
              setConfirming(true)
            }}
            className={ITEM}
          >
            {strings.removal.remove}
          </button>
        </div>
      </Menu>
      {confirming ? (
        <ConfirmDelete
          title={strings.removal.confirmTitle(part.name)}
          body={strings.removal.confirmBody(part.name)}
          busy={remove.isPending}
          failure={remove.isError ? strings.removal.removeFailed : null}
          onConfirm={() => remove.mutate()}
          onCancel={() => {
            remove.reset()
            setConfirming(false)
          }}
        />
      ) : null}
      {moving ? (
        <MovePartDialog part={{ id: part.id, name: part.name }} library={part.library} onClose={() => setMoving(false)} />
      ) : null}
    </>
  )
}
