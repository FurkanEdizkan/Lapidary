import { createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { DEFAULT_LIBRARY_ID, fetchParts, movePart } from '../lib/api'
import { FolderTree, PART_DRAG_TYPE, partDragPayload, useFolders } from '../components/FolderTree'
import { TopBar } from '../components/TopBar'
import { strings } from '../lib/strings'
import type { FolderId, LibraryId, PartCard, PartId } from '../lib/types'

/**
 * Filing — `v2`'s Groups screen.
 *
 * # What it is for, and why the grid is not it
 *
 * The grid files one model at a time: drag a card onto a category in the rail. That is the
 * right gesture for the model you are already looking at and the wrong one for the forty
 * that came out of one scan, because it costs forty drags and the grid is not laid out to
 * be dragged *from* — a wall of 11rem renders scrolls under the pointer while the rail
 * stays put.
 *
 * So this is the same two things side by side at a size you can work with: the tree on the
 * left, its contents as a checkable list in the middle. Select a run of models, drop them on
 * a category, done.
 *
 * # What the design has here that this does not
 *
 * A third column of tags — rename, recolour, merge, and a star that features a tag as a row
 * on Home. There is no tag table (`ROADMAP.md` Phase 5) and no Home feed, so all three
 * controls would write nowhere. The two columns that ship are the two backed by routes that
 * exist: `GET /api/libraries/{id}/folders` and `PATCH /api/parts/{id}`.
 */
export const Route = createFileRoute('/organize')({
  component: RouteComponent,
  validateSearch: (search: Record<string, unknown>): { library?: string; folderId?: string } => ({
    ...(typeof search.library === 'string' && search.library.length > 0
      ? { library: search.library }
      : {}),
    ...(typeof search.folderId === 'string' && search.folderId.length > 0
      ? { folderId: search.folderId }
      : {}),
  }),
})

/**
 * Reads the search params and hands them to `Organize` as props — the same split
 * `index.tsx` makes, and for the same reason: the component that does the work takes plain
 * props, so a test can drive it without a router in scope.
 */
function RouteComponent() {
  const { library, folderId } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Organize
      library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID}
      folderId={folderId as FolderId | undefined}
      onSelectFolder={(folder) =>
        void navigate({ search: (previous) => ({ ...previous, folderId: folder ?? undefined }) })
      }
    />
  )
}

export function Organize({
  library,
  folderId,
  onSelectFolder,
}: {
  library: LibraryId
  folderId?: FolderId
  onSelectFolder?: (folder: FolderId | null) => void
}) {
  const queryClient = useQueryClient()
  const folders = useFolders(library)
  const folderName =
    folderId === undefined
      ? null
      : (folders.data?.find((folder) => folder.id === folderId)?.name ?? null)

  /**
   * Which models are ticked. A `Set` of ids and not a flag on each card: the cards come from
   * a paged query whose pages are replaced wholesale on a refetch, so a selection stored in
   * the rows would be dropped every time the list refilled — including by this page's own
   * move, which is the moment it matters most.
   */
  const [picked, setPicked] = useState<ReadonlySet<PartId>>(new Set())
  /** What the last bulk move did, when it has something to report. See `filed` below. */
  const [outcome, setOutcome] = useState<string | null>(null)

  /*
    The same infinite query the grid runs, under the same key — so opening this page after
    the grid costs nothing, and a move made here refills both. The page size is the route's
    default rather than the browser's stored one: this list is a working surface rather than
    a wall to scan, and the stored preference belongs to the grid it was set on.
  */
  const parts = useInfiniteQuery({
    queryKey: ['parts', library, folderId ?? null, null, 50],
    queryFn: ({ pageParam }) => fetchParts(library, pageParam, undefined, folderId, undefined, 50),
    initialPageParam: undefined as PartId | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
  const loaded = parts.data?.pages.flatMap((page) => page.parts) ?? []

  /**
   * Move everything ticked into one category.
   *
   * # One request per model, deliberately, and no confirmation dialog
   *
   * There is no bulk move route — `PATCH /api/parts/{id}` takes one model — so this is a
   * loop, and `Promise.all` would fire fifty PATCHes at once against a route that renames a
   * directory per call. Sequential is slower and is the shape the server was built for.
   *
   * A model whose name already exists in the target is **refused and left where it was**.
   * The single-model drag answers that with a dialog offering to keep both, and that dialog
   * is exactly what must not appear here: a run of fifty could raise it eight times, and a
   * person clicking through eight identical prompts is a person not reading them. So a bulk
   * move never acknowledges a duplicate. It reports how many were refused and leaves those
   * models selected, so the ones needing a decision are the ones still ticked and can be
   * dragged over individually.
   */
  const file = useMutation({
    mutationFn: async (target: FolderId | null) => {
      const refused: PartCard[] = []
      let moved = 0
      for (const part of loaded) {
        if (!picked.has(part.id)) continue
        // `acknowledge: false`, always — see above. A refusal is an answer here, not a
        // failure, so it is collected rather than thrown.
        const result = await movePart(part.id, target, false)
        if (result.kind === 'refused') refused.push(part)
        else moved += 1
      }
      return { moved, refused }
    },
    onSuccess: ({ moved, refused }) => {
      setOutcome(strings.organize.filed(moved, refused.length))
      // Only what could not go stays ticked. Clearing everything would lose the one piece of
      // information the refusals produced — which models still need a decision.
      setPicked(new Set(refused.map((part) => part.id)))
      void queryClient.invalidateQueries({ queryKey: ['parts', library] })
      void queryClient.invalidateQueries({ queryKey: ['folders', library] })
    },
    onError: () => setOutcome(strings.organize.filedFailed),
  })

  const toggle = (part: PartId) =>
    setPicked((current) => {
      const next = new Set(current)
      if (!next.delete(part)) next.add(part)
      return next
    })

  return (
    <div className="flex h-screen flex-col">
      <title>{strings.titles.organize}</title>
      <TopBar library={library} sidebar={null} />
      <div className="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[240px_1fr]">
        {/*
          The tree, in its own scrolling column. The same component the grid's rail uses, so
          creating, renaming, deleting and single-model drops all behave identically on both
          screens — and a fix to any of them lands on both.
        */}
        <div className="min-h-0 overflow-y-auto border-r border-[var(--color-border)] bg-[var(--color-raised)] px-[10px] py-[14px]">
          <FolderTree
            library={library}
            selected={folderId ?? null}
            onSelect={(folder) => onSelectFolder?.(folder)}
          />
        </div>

        <main className="flex min-h-0 min-w-0 flex-col overflow-y-auto px-[15px] py-[12px]">
          <div className="mb-3 flex flex-wrap items-baseline gap-x-3 gap-y-2">
            <h2 className="text-sm font-semibold text-[var(--color-bright)]">
              {folderName ?? strings.folders.root}
            </h2>
            <p className="tabular text-[10.5px] text-[var(--color-muted)]">
              {parts.hasNextPage
                ? strings.parts.showingSoFar(loaded.length)
                : strings.parts.showingAll(loaded.length)}
            </p>
            <span className="flex-1" />
            {picked.size === 0 ? (
              <p className="text-[11.5px] text-[var(--color-muted)]">{strings.organize.hint}</p>
            ) : (
              <>
                <p className="tabular text-[11px] text-[var(--color-accent)]">
                  {strings.organize.selected(picked.size)}
                </p>
                <button
                  type="button"
                  onClick={() => setPicked(new Set())}
                  className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]"
                >
                  {strings.organize.clear}
                </button>
                {/*
                  The keyboard route to a bulk move, and it has to exist: the pointer route
                  is a drag onto a row in the tree, and SC 2.1.1 asks whether a function is
                  available from the keyboard, not whether the pointer has a nicer way to
                  reach it. A select of every category plus the library root, which is the
                  same set of targets the drag offers.
                */}
                <label className="flex items-center gap-2 text-[11px] text-[var(--color-muted)]">
                  {strings.organize.fileInto}
                  <select
                    // Never holds a value: this is an action, not a setting, and leaving the
                    // last target selected would make the next bulk move one keystroke from
                    // repeating it into a category nobody re-read.
                    value=""
                    disabled={file.isPending}
                    onChange={(event) => {
                      const target = event.target.value
                      if (target !== '') file.mutate(target === ROOT ? null : (target as FolderId))
                    }}
                    className="rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] px-2 py-1 text-[11px] text-[var(--color-dim)]"
                  >
                    <option value="">{strings.organize.chooseCategory}</option>
                    <option value={ROOT}>{strings.folders.root}</option>
                    {(folders.data ?? []).map((folder) => (
                      <option key={folder.id} value={folder.id}>
                        {folder.name}
                      </option>
                    ))}
                  </select>
                </label>
              </>
            )}
          </div>

          {outcome === null ? null : (
            <p role="status" className="mb-3 text-[11.5px] text-[var(--color-muted)]">
              {outcome}
            </p>
          )}

          {parts.isPending ? (
            <p className="text-[var(--color-muted)]">{strings.parts.loading}</p>
          ) : parts.isError ? (
            <p className="max-w-prose text-[var(--color-muted)]">{strings.parts.failed}</p>
          ) : loaded.length === 0 ? (
            <p className="rounded-[var(--radius-md)] border border-dashed border-[var(--color-border)] px-4 py-9 text-center text-[12.5px] text-[var(--color-muted)]">
              {strings.organize.empty}
            </p>
          ) : (
            <ul role="list" className="flex flex-col gap-1">
              {loaded.map((part) => (
                <Row
                  key={part.id}
                  part={part}
                  picked={picked.has(part.id)}
                  onToggle={() => toggle(part.id)}
                  /*
                    What a drag from this row carries. When the row is ticked the drag is the
                    whole selection — but `DataTransfer` carries one part, so what actually
                    travels is this row, and the tree's drop handler moves that one. Dragging
                    the selection would need the drop target to read this page's state, which
                    a component shared with the grid must not do. The select above is the
                    route for many; the drag is the route for one, on both screens.
                  */
                />
              ))}
            </ul>
          )}
          {!parts.hasNextPage ? null : (
            <button
              type="button"
              onClick={() => void parts.fetchNextPage()}
              disabled={parts.isFetchingNextPage}
              className="ease-mechanical mt-3 self-start rounded-[var(--radius-ctl)] border border-[var(--color-border)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)] disabled:opacity-50"
            >
              {parts.isFetchingNextPage ? strings.parts.loadingMore : strings.parts.loadMore}
            </button>
          )}
        </main>
      </div>
    </div>
  )
}

/**
 * The value the "no category" option carries in the select.
 *
 * A sentinel rather than `''`, because the empty string is already the select's own resting
 * value — the one that means "no action chosen". Without a distinct value, choosing the
 * library root would be indistinguishable from choosing nothing, and a bulk move to the root
 * would silently do nothing.
 */
const ROOT = 'root'

function Row({
  part,
  picked,
  onToggle,
}: {
  part: PartCard
  picked: boolean
  onToggle: () => void
}) {
  return (
    <li>
      {/*
        A real checkbox, not a div with a tick in it. It gets the keyboard, the announced
        checked state, and shift-click range behaviour from the platform — and this is a
        list somebody works down, which is where those matter most.

        The label wraps the whole row, so the thumbnail and the name are part of the target:
        SC 2.5.8 asks for 24px and a bare checkbox is 13.
      */}
      <label
        draggable={part.directory !== null}
        onDragStart={(event) =>
          event.dataTransfer.setData(
            PART_DRAG_TYPE,
            partDragPayload({ id: part.id, name: part.name }),
          )
        }
        className={`ease-mechanical flex cursor-pointer items-center gap-3 rounded-[var(--radius-ctl)] border px-2.5 py-2 duration-[var(--duration-fast)] ${
          picked
            ? 'border-[var(--color-accent)] bg-[color-mix(in_oklab,var(--color-accent)_8%,transparent)]'
            : 'border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-edge)]'
        }`}
      >
        <input
          type="checkbox"
          checked={picked}
          onChange={onToggle}
          className="size-4 flex-none accent-[var(--color-accent)]"
        />
        <span className="grid size-[38px] flex-none place-items-center overflow-hidden rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-raised)] p-[4px]">
          {part.thumbnail === null ? null : (
            <img
              src={part.thumbnail}
              alt={strings.parts.thumbnailAlt(part.name)}
              className="h-full w-full object-contain"
            />
          )}
        </span>
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-semibold">{part.name}</span>
        {/*
          The path, not the part number: this screen is about where a model *is*, and two
          models with the same name in different folders are told apart by exactly this.
        */}
        <span className="tabular hidden max-w-[40%] flex-none truncate text-[10px] text-[var(--color-muted)] sm:block">
          {part.sourcePath}
        </span>
      </label>
    </li>
  )
}
