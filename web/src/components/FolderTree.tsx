import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useId, useState, type DragEvent, type ReactNode } from 'react'
import { deleteFolder, fetchFolders, movePart } from '../lib/api'
import { strings } from '../lib/strings'
import type { FolderId, FolderNode, LibraryId, PartId } from '../lib/types'

/**
 * What a dragged card carries, and the only thing a folder row will accept.
 *
 * A private MIME type rather than `text/plain`: a category must not swallow whatever text
 * someone dragged in from another window, and the drop handler has no other way to tell
 * the two apart. The payload carries the name as well as the id because the duplicate
 * dialog names the model, and a drop is the one path where nothing else on screen knows
 * which card was let go of.
 */
export const PART_DRAG_TYPE = 'application/x-lapidary-part'

/** The model a drag or a menu is moving: the id to PATCH, the name to put in a dialog. */
type DraggedPart = { id: PartId; name: string }

export function partDragPayload(part: DraggedPart): string {
  return JSON.stringify(part)
}

function readDragPayload(data: string): DraggedPart | null {
  try {
    const value: unknown = JSON.parse(data)
    if (value === null || typeof value !== 'object') return null
    const { id, name } = value as { id?: unknown; name?: unknown }
    return typeof id === 'string' && typeof name === 'string' ? { id, name } : null
  } catch {
    // Anything but our own payload — a file, a URL, a selection from another app.
    return null
  }
}

/**
 * The whole tree in one query, keyed per library so the sidebar and the per-card move
 * chooser share one cache entry and one request. The API answers the tree whole rather
 * than a level at a time (design §10), so there is no per-expand round trip to key on.
 */
export function useFolders(library: LibraryId) {
  return useQuery({ queryKey: ['folders', library], queryFn: () => fetchFolders(library) })
}

/**
 * Moving one model, including the collision the move can come back with.
 *
 * `409` is an answer, not a failure: two models may share a name — slice 6a decided that
 * is the truth — so the API warns once and the client re-sends the SAME target with
 * `acknowledgeDuplicate: true`. Both requests spell the flag out; the route defaults it,
 * but a move that silently omitted it would be indistinguishable on the wire from one that
 * meant `false`.
 *
 * A second `409` after the acknowledgement is not the collision again — the route returns
 * that status for a model that has not finished migrating and for a cross-library target
 * as well — so it becomes a note rather than the same dialog a second time. Re-opening it
 * would be a loop with no exit.
 */
function useMovePart(library: LibraryId, onMoved?: () => void) {
  const queryClient = useQueryClient()
  const [duplicate, setDuplicate] = useState<{ part: DraggedPart; folderId: FolderId | null } | null>(
    null,
  )
  const [refused, setRefused] = useState(false)
  const move = useMutation({
    mutationFn: (input: { part: DraggedPart; folderId: FolderId | null; acknowledge: boolean }) =>
      movePart(input.part.id, input.folderId, input.acknowledge),
    onSuccess: (outcome, input) => {
      if (outcome === 'duplicate') {
        setDuplicate(input.acknowledge ? null : { part: input.part, folderId: input.folderId })
        setRefused(input.acknowledge)
        return
      }
      setDuplicate(null)
      setRefused(false)
      // Both move with the model: the grid it left and the grid it landed in are the same
      // query under different keys, and `partCount` is what the delete dialog names.
      void queryClient.invalidateQueries({ queryKey: ['parts', library] })
      void queryClient.invalidateQueries({ queryKey: ['folders', library] })
      onMoved?.()
    },
  })
  return {
    move,
    duplicate,
    refused,
    start: (part: DraggedPart, folderId: FolderId | null) => {
      setRefused(false)
      move.mutate({ part, folderId, acknowledge: false })
    },
    confirm: () => {
      if (duplicate !== null) {
        move.mutate({ part: duplicate.part, folderId: duplicate.folderId, acknowledge: true })
      }
    },
    dismiss: () => setDuplicate(null),
  }
}

/**
 * The category tree beside the grid.
 *
 * Presentational about its selection and not about its data: `selected` and `onSelect` are
 * props because the selection lives in the URL (`?folderId=`), which only the route
 * component can read — the same split `Index` already uses for `batch`, and the reason
 * every test in `index.test.tsx` can render the page with no router in scope.
 *
 * Rendered expanded, with no disclosure control. A category the user cannot see is a
 * category they cannot drop onto, and the tree is hundreds of rows at corpus scale, not
 * thousands. Collapsing arrives with the count that makes it worth having.
 */
export function FolderTree({
  library,
  selected,
  onSelect,
}: {
  library: LibraryId
  selected: FolderId | null
  onSelect: (folder: FolderId | null) => void
}) {
  const folders = useFolders(library)
  const queryClient = useQueryClient()
  const { move, duplicate, refused, start, confirm, dismiss } = useMovePart(library)
  const [pendingDelete, setPendingDelete] = useState<FolderNode | null>(null)

  const remove = useMutation({
    mutationFn: (folder: FolderNode) => deleteFolder(folder.id),
    onSuccess: (_result, folder) => {
      setPendingDelete(null)
      void queryClient.invalidateQueries({ queryKey: ['folders', library] })
      void queryClient.invalidateQueries({ queryKey: ['parts', library] })
      // The delete cascades through subcategories (design §7), so the filter has to be
      // dropped for a descendant too — otherwise the grid keeps asking about a category
      // that is gone and shows nothing, with no visible reason why.
      if (selected !== null && isWithin(folders.data ?? [], selected, folder.id)) {
        onSelect(null)
      }
    },
  })

  const drop = (event: DragEvent<HTMLElement>, folderId: FolderId | null) => {
    event.preventDefault()
    const dragged = readDragPayload(event.dataTransfer.getData(PART_DRAG_TYPE))
    if (dragged !== null) {
      start(dragged, folderId)
    }
  }

  return (
    <nav aria-label={strings.folders.title} className="w-56 shrink-0">
      <h2 className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.folders.title}
      </h2>
      <ul className="space-y-0.5">
        <li>
          <FolderButton
            name={strings.folders.root}
            selected={selected === null}
            onSelect={() => onSelect(null)}
            onDrop={(event) => drop(event, null)}
          />
        </li>
      </ul>
      {folders.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.loading}</p>
      ) : folders.isError ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.failed}</p>
      ) : folders.data.length === 0 ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.empty}</p>
      ) : (
        <FolderLevel
          folders={folders.data}
          parentId={null}
          depth={0}
          selected={selected}
          onSelect={onSelect}
          onDropPart={drop}
          onDelete={setPendingDelete}
        />
      )}
      {move.isError ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.moveFailed}</p>
      ) : refused ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.moveRefused}</p>
      ) : null}
      {remove.isError ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.deleteFailed}</p>
      ) : null}
      {duplicate === null ? null : (
        <DuplicateDialog
          name={duplicate.part.name}
          busy={move.isPending}
          onConfirm={confirm}
          onCancel={dismiss}
        />
      )}
      {pendingDelete === null ? null : (
        <DeleteDialog
          folder={pendingDelete}
          busy={remove.isPending}
          onConfirm={() => remove.mutate(pendingDelete)}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </nav>
  )
}

/**
 * Is `node` the category `root`, or one of its descendants?
 *
 * The same bounded ancestor walk the scan and the reparent check use, at the same depth of
 * 16 (design §7): a parent chain that has been corrupted into a cycle must not hang the
 * sidebar, and no honest tree is deeper than that.
 */
function isWithin(folders: readonly FolderNode[], node: FolderId, root: FolderId): boolean {
  const byId = new Map(folders.map((folder) => [folder.id, folder]))
  let current: FolderId | null = node
  for (let hops = 0; current !== null && hops < 16; hops += 1) {
    if (current === root) return true
    current = byId.get(current)?.parentId ?? null
  }
  return false
}

function FolderLevel({
  folders,
  parentId,
  depth,
  selected,
  onSelect,
  onDropPart,
  onDelete,
}: {
  folders: readonly FolderNode[]
  parentId: FolderId | null
  depth: number
  selected: FolderId | null
  onSelect: (folder: FolderId) => void
  onDropPart: (event: DragEvent<HTMLElement>, folder: FolderId) => void
  onDelete: (folder: FolderNode) => void
}) {
  const children = folders
    .filter((folder) => folder.parentId === parentId)
    .sort((a, b) => a.name.localeCompare(b.name))
  if (children.length === 0) {
    return null
  }
  return (
    <ul className="mt-0.5 space-y-0.5">
      {children.map((folder) => (
        <li key={folder.id}>
          <div
            className="group flex items-center gap-1"
            style={{ paddingLeft: `${depth * 0.75}rem` }}
          >
            <FolderButton
              name={folder.name}
              selected={selected === folder.id}
              onSelect={() => onSelect(folder.id)}
              onDrop={(event) => onDropPart(event, folder.id)}
            />
            {/*
              Present for every category and quiet until it is wanted: opacity only, so it
              costs no layout, and it comes back on keyboard focus as well as on hover —
              a control that only exists under a pointer is a control a keyboard cannot
              reach.
            */}
            <button
              type="button"
              onClick={() => onDelete(folder)}
              aria-label={strings.folders.deleteFor(folder.name)}
              className="ease-mechanical rounded px-1.5 py-1 text-xs text-[var(--color-muted)] opacity-0 duration-[var(--duration-fast)] group-hover:opacity-100 focus-visible:opacity-100"
            >
              {strings.folders.deleteAction}
            </button>
          </div>
          <FolderLevel
            folders={folders}
            parentId={folder.id}
            depth={depth + 1}
            selected={selected}
            onSelect={onSelect}
            onDropPart={onDropPart}
            onDelete={onDelete}
          />
        </li>
      ))}
    </ul>
  )
}

/**
 * One row: the filter, and the drop target for a dragged card.
 *
 * `onDragOver` calls `preventDefault()` because that is what marks an element as willing
 * to accept a drop — without it the browser never fires `drop` at all, and jsdom will not
 * notice the omission.
 *
 * The affordance is 120 ms on transform and opacity, per `CLAUDE.md`: the row lifts while
 * a card is over it. The border colour changes with it and is not animated — the global
 * transition property list is `transform, opacity` and nothing here widens it.
 */
function FolderButton({
  name,
  selected,
  onSelect,
  onDrop,
}: {
  name: string
  selected: boolean
  onSelect: () => void
  onDrop: (event: DragEvent<HTMLElement>) => void
}) {
  const [over, setOver] = useState(false)
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? 'true' : undefined}
      onDragOver={(event) => {
        event.preventDefault()
        setOver(true)
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => {
        setOver(false)
        onDrop(event)
      }}
      className={`ease-mechanical w-full truncate rounded border px-2 py-1 text-left text-sm duration-[var(--duration-fast)] ${
        selected
          ? 'border-[var(--color-border)] bg-[var(--color-surface)]'
          : 'border-transparent'
      } ${over ? '-translate-y-px border-[var(--color-accent)]' : ''}`}
    >
      {name}
    </button>
  )
}

/**
 * The per-card move path, and the reason drag is not the only one.
 *
 * Drag into a scrolled tree is a poor trackpad target and impossible from a keyboard, so
 * this is not a fallback: it is the complete path, reachable by tab and operable by
 * Enter. Plain buttons in a labelled list rather than `role="menu"` — that role promises
 * arrow-key navigation this would then owe.
 */
export function MovePartDialog({
  part,
  library,
  onClose,
}: {
  part: DraggedPart
  library: LibraryId
  onClose: () => void
}) {
  const folders = useFolders(library)
  const { move, duplicate, refused, start, confirm, dismiss } = useMovePart(library, onClose)
  if (duplicate !== null) {
    return (
      <DuplicateDialog
        name={duplicate.part.name}
        busy={move.isPending}
        onConfirm={confirm}
        onCancel={dismiss}
      />
    )
  }
  return (
    <Dialog title={strings.folders.moveTitle(part.name)} onClose={onClose}>
      <ul className="mt-3 max-h-72 space-y-1 overflow-y-auto">
        <li>
          <MoveRow
            name={strings.folders.root}
            depth={0}
            busy={move.isPending}
            onMove={() => start(part, null)}
          />
        </li>
        {folders.isPending ? (
          <li className="text-sm text-[var(--color-muted)]">{strings.folders.loading}</li>
        ) : folders.isError ? (
          <li className="text-sm text-[var(--color-muted)]">{strings.folders.failed}</li>
        ) : folders.data.length === 0 ? (
          <li className="text-sm text-[var(--color-muted)]">{strings.folders.empty}</li>
        ) : (
          <MoveLevel
            folders={folders.data}
            parentId={null}
            depth={1}
            busy={move.isPending}
            onMove={(folder) => start(part, folder.id)}
          />
        )}
      </ul>
      {move.isError ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.moveFailed}</p>
      ) : refused ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.moveRefused}</p>
      ) : null}
      {/*
        Focus lands on cancel, never on a target: every other control here files the model
        somewhere, and a chooser that acts on Enter before a category is picked moves it
        somewhere nobody chose.
      */}
      <div className="mt-4 flex justify-end">
        <DialogButton onClick={onClose} autoFocus>
          {strings.folders.cancel}
        </DialogButton>
      </div>
    </Dialog>
  )
}

function MoveLevel({
  folders,
  parentId,
  depth,
  busy,
  onMove,
}: {
  folders: readonly FolderNode[]
  parentId: FolderId | null
  depth: number
  busy: boolean
  onMove: (folder: FolderNode) => void
}) {
  return (
    <>
      {folders
        .filter((folder) => folder.parentId === parentId)
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((folder) => (
          <li key={folder.id}>
            <MoveRow
              name={folder.name}
              depth={depth}
              busy={busy}
              onMove={() => onMove(folder)}
            />
            <ul className="space-y-1">
              <MoveLevel
                folders={folders}
                parentId={folder.id}
                depth={depth + 1}
                busy={busy}
                onMove={onMove}
              />
            </ul>
          </li>
        ))}
    </>
  )
}

/**
 * A category and the control that moves the model into it. The visible label is the same
 * on every row, so the accessible name says which category — otherwise a screen reader
 * hears "Move here" fourteen times.
 */
function MoveRow({
  name,
  depth,
  busy,
  onMove,
}: {
  name: string
  depth: number
  busy: boolean
  onMove: () => void
}) {
  return (
    <div
      className="flex items-center justify-between gap-2"
      style={{ paddingLeft: `${depth * 0.75}rem` }}
    >
      <span className="truncate text-sm">{name}</span>
      <button
        type="button"
        onClick={onMove}
        disabled={busy}
        aria-label={strings.folders.moveInto(name)}
        className="ease-mechanical shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.folders.moveHere}
      </button>
    </div>
  )
}

/**
 * The name collision. Not an error: two models may be called `bracket`, and the API says
 * so once rather than refusing. Confirming re-sends the same move with the flag set.
 */
function DuplicateDialog({
  name,
  busy,
  onConfirm,
  onCancel,
}: {
  name: string
  busy: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <Dialog title={strings.folders.duplicateTitle(name)} onClose={onCancel}>
      <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.folders.duplicateBody}</p>
      <div className="mt-4 flex justify-end gap-2">
        <DialogButton onClick={onCancel}>{strings.folders.cancel}</DialogButton>
        <DialogButton onClick={onConfirm} disabled={busy} autoFocus>
          {strings.folders.duplicateConfirm}
        </DialogButton>
      </div>
    </Dialog>
  )
}

/**
 * The destructive confirmation, which has to say what it destroys — and, here, what it
 * does not. This is a soft delete: the models inside are marked deleted and hidden, every
 * byte stays where it is on disk, and neither this nor `DATA.md` §1.6's purge is the
 * other. The count comes from `FolderNode.partCount`, which the one tree request already
 * carried, and it counts the whole subtree because the delete cascades through it.
 */
function DeleteDialog({
  folder,
  busy,
  onConfirm,
  onCancel,
}: {
  folder: FolderNode
  busy: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <Dialog title={strings.folders.deleteTitle(folder.name)} onClose={onCancel}>
      <p className="mt-2 text-sm text-[var(--color-muted)]">
        {strings.folders.deleteBody(folder.partCount)}
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <DialogButton onClick={onCancel} autoFocus>
          {strings.folders.cancel}
        </DialogButton>
        <DialogButton onClick={onConfirm} disabled={busy}>
          {strings.folders.deleteConfirm}
        </DialogButton>
      </div>
    </Dialog>
  )
}

/**
 * The shell every dialog here shares. `role="dialog"` + `aria-modal`, Escape closes, and
 * the caller autofocuses whichever control the safe answer is — cancel where the action is
 * destructive, confirm where it is not. No focus trap: that needs an inert background or a
 * library, and neither belongs in this slice.
 */
function Dialog({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  const titleId = useId()
  return (
    <div
      className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6"
      onKeyDown={(event) => {
        if (event.key === 'Escape') onClose()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-full max-w-md rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      >
        <h2 id={titleId} className="text-sm font-medium">
          {title}
        </h2>
        {children}
      </div>
    </div>
  )
}

function DialogButton({
  onClick,
  disabled,
  autoFocus,
  children,
}: {
  onClick: () => void
  disabled?: boolean
  autoFocus?: boolean
  children: ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      autoFocus={autoFocus}
      className="ease-mechanical rounded border border-[var(--color-border)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
    >
      {children}
    </button>
  )
}
