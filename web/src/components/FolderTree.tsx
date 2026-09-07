import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useId, useRef, useState, type DragEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import {
  createFolder,
  deleteFolder,
  fetchFolders,
  movePart,
  renameFolder,
  type FolderWriteRefusal,
  type MoveRefusalReason,
} from '../lib/api'
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

/** The three `409` reasons that cannot be fixed by acknowledging — see `refusalMessage`. */
type TerminalRefusal = Exclude<MoveRefusalReason, 'duplicateName'>

/**
 * Which string names a terminal refusal. `duplicateName` never reaches here — it is the
 * one reason `useMovePart` turns into `duplicate` instead — so this switch is exhaustive
 * over what is left without needing a fallback case that could paper over a fifth reason
 * arriving later unnoticed.
 */
function refusalMessage(reason: TerminalRefusal): string {
  switch (reason) {
    case 'migrationPending':
      // Same wording the card's own "not movable yet" state uses — the two places a user
      // meets this state say the same thing.
      return strings.folders.notMigrated
    case 'crossLibrary':
      return strings.folders.crossLibraryRefusal
    case 'noSuchFolder':
      return strings.folders.noSuchFolderRefusal
    case 'unknown':
      return strings.folders.moveRefused
  }
}

/**
 * Moving one model, including the four `409` reasons the move can come back with.
 *
 * `409` is an answer, not a failure — but only one of its four reasons is fixed by trying
 * again: `duplicateName`, where two models sharing a name is slice 6a's decided truth, so
 * the API warns once and the client re-sends the SAME target with `acknowledgeDuplicate:
 * true`. Both requests spell the flag out; the route defaults it, but a move that silently
 * omitted it would be indistinguishable on the wire from one that meant `false`.
 *
 * The other three — `migrationPending`, `crossLibrary`, `noSuchFolder` — are dead ends for
 * this attempt no matter which request carried them: acknowledging cannot finish a
 * migration, move a category into this library, or bring back a folder that is gone. Those
 * become a note instead of the dialog, on the first `409` as readily as a second — nothing
 * here waits for an acknowledgement round trip to find out.
 */
function useMovePart(library: LibraryId, onMoved?: () => void) {
  const queryClient = useQueryClient()
  const [duplicate, setDuplicate] = useState<{ part: DraggedPart; folderId: FolderId | null } | null>(
    null,
  )
  // The target rides along with the reason. The sidebar renders a refusal under the row it
  // was dropped on rather than at the foot of the whole tree, and the row is the only thing
  // that says which category the sentence is about.
  const [refusal, setRefusal] = useState<{
    reason: TerminalRefusal
    folderId: FolderId | null
  } | null>(null)
  const move = useMutation({
    mutationFn: (input: { part: DraggedPart; folderId: FolderId | null; acknowledge: boolean }) =>
      movePart(input.part.id, input.folderId, input.acknowledge),
    onSuccess: (outcome, input) => {
      if (outcome.kind === 'refused') {
        if (outcome.reason === 'duplicateName') {
          setDuplicate({ part: input.part, folderId: input.folderId })
          setRefusal(null)
        } else {
          setDuplicate(null)
          setRefusal({ reason: outcome.reason, folderId: input.folderId })
        }
        return
      }
      setDuplicate(null)
      setRefusal(null)
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
    refusal,
    start: (part: DraggedPart, folderId: FolderId | null) => {
      setRefusal(null)
      move.mutate({ part, folderId, acknowledge: false })
    },
    confirm: () => {
      if (duplicate !== null) {
        move.mutate({ part: duplicate.part, folderId: duplicate.folderId, acknowledge: true })
      }
    },
    dismiss: () => setDuplicate(null),
    /**
     * Drop the last refusal without starting anything. A mutation's error state lives until
     * its own next run, so a move that was refused keeps its note on that row forever —
     * including under a later delete of the same row, whose failure would then be the thing
     * nobody is told about. Whatever acts on a row next clears what the last action said.
     */
    forget: () => {
      setRefusal(null)
      move.reset()
    },
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
  const { move, duplicate, refusal, start, confirm, dismiss, forget } = useMovePart(library)
  const [pendingDelete, setPendingDelete] = useState<FolderNode | null>(null)
  /**
   * Whether the create dialog is open. A boolean and not a parent id: the new category goes
   * under whatever the sidebar has selected, which is state this component already has and
   * which the dialog's own title names — so holding a second copy of it here would be two
   * answers to "where does this go" that a click between opening and confirming could
   * disagree about.
   */
  const [creating, setCreating] = useState(false)
  const [renaming, setRenaming] = useState<FolderNode | null>(null)
  /** A refused create or rename, shown inside the dialog that caused it — see `note` below. */
  const [writeRefusal, setWriteRefusal] = useState<string | null>(null)
  /**
   * What a finished delete has to say, when it has something to say. Held at the level of
   * the tree rather than under a row, because both messages are about a row that is on its
   * way out: the refetch this same handler fires is what takes it off screen.
   */
  const [deleteOutcome, setDeleteOutcome] = useState<string | null>(null)

  /**
   * Create and rename share everything after the request: both invalidate the same tree,
   * both keep their dialog open on a refusal so the user can edit what they typed, and both
   * close it on success. Written once and given the two mutation functions rather than
   * twice — the halves that differ are the request and the dialog, and neither is here.
   */
  const written = (close: () => void) => ({
    onMutate: () => setWriteRefusal(null),
    onSuccess: (result: { kind: 'written' } | { kind: 'refused'; reason: FolderWriteRefusal }) => {
      if (result.kind === 'refused') {
        setWriteRefusal(folderWriteMessage(result.reason))
        return
      }
      close()
      void queryClient.invalidateQueries({ queryKey: ['folders', library] })
    },
  })

  const add = useMutation({
    mutationFn: (name: string) => createFolder(library, selected, name),
    ...written(() => setCreating(false)),
  })

  const rename = useMutation({
    mutationFn: ({ folder, name }: { folder: FolderNode; name: string }) =>
      renameFolder(folder.id, name),
    ...written(() => setRenaming(null)),
  })

  const remove = useMutation({
    mutationFn: (folder: FolderNode) => deleteFolder(folder.id),
    // What the last action on this row had to say is not about this one. Both halves
    // matter: a refused move keeps its note until the move mutation runs again, so without
    // this a delete that fails on the same row would be the silent one.
    onMutate: () => {
      setDeleteOutcome(null)
      forget()
    },
    onSuccess: (result, folder) => {
      setPendingDelete(null)
      // Fired for the refusal too, and that is the point of handling it: a `404` means the
      // category is already gone, so the row still on screen is the stale thing and the
      // tree is what has to be re-read.
      void queryClient.invalidateQueries({ queryKey: ['folders', library] })
      void queryClient.invalidateQueries({ queryKey: ['parts', library] })
      if (result.kind === 'refused') {
        setDeleteOutcome(strings.folders.deleteGone)
      } else {
        // `foldersHidden` counts the category along with its descendants, so the one is
        // taken off before it is compared against a subcategory count that never included
        // it. Both figures are checked against what the confirmation actually claimed —
        // that count was read when the dialog opened, and the library can move underneath
        // an open dialog.
        const subcategories = result.foldersHidden === null ? null : Math.max(result.foldersHidden - 1, 0)
        const differs =
          (result.partsHidden !== null && result.partsHidden !== folder.partCount) ||
          (subcategories !== null && subcategories !== subcategoryCount(folders.data ?? [], folder.id))
        setDeleteOutcome(
          differs
            ? strings.folders.deleteCountsDiffered(result.partsHidden ?? 0, subcategories ?? 0)
            : null,
        )
      }
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

  /**
   * The note that belongs under one row, if any belongs under this one.
   *
   * Under the row, not at the foot of the `<nav>`: a refusal is about the category it was
   * dropped on, and the previous version rendered it arbitrarily far from that row, below
   * however many hundred categories the library has.
   *
   * Only the move's notes. A delete that fails leaves its confirmation open, so its note
   * belongs inside that dialog — a row behind an opaque scrim is not where the action
   * happened. A delete that is refused closes it, and that note is the one held at the
   * level of the tree.
   */
  const noteFor = (folder: FolderId | null): string | null => {
    if (move.isError && (move.variables?.folderId ?? null) === folder) {
      return strings.folders.moveFailed
    }
    if (refusal !== null && refusal.folderId === folder) {
      return refusalMessage(refusal.reason)
    }
    return null
  }

  return (
    <nav aria-label={strings.folders.title} className="w-56 shrink-0">
      <div className="mb-2 flex items-baseline justify-between gap-2">
        <h2 className="text-xs tracking-wider text-[var(--color-muted)] uppercase">
          {strings.folders.title}
        </h2>
        {/*
          Always visible, unlike the per-row delete and rename. Those are actions on a row
          you are already pointing at; this is the only way to get a first category into an
          empty library, and a control that appears on hover is one an empty sidebar never
          reveals.
        */}
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="ease-mechanical rounded border border-[var(--color-border)] px-1.5 py-0.5 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.folders.newCategory}
        </button>
      </div>
      <ul className="space-y-0.5">
        <li>
          <FolderButton
            name={strings.folders.root}
            selected={selected === null}
            onSelect={() => onSelect(null)}
            onDrop={(event) => drop(event, null)}
          />
          <RowNote note={noteFor(null)} />
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
          onRename={setRenaming}
          noteFor={noteFor}
        />
      )}
      <RowNote note={deleteOutcome} />
      {duplicate === null ? null : (
        <DuplicateDialog
          name={duplicate.part.name}
          busy={move.isPending}
          onConfirm={confirm}
          onCancel={dismiss}
        />
      )}
      {!creating ? null : (
        <NameDialog
          title={strings.folders.createTitle(
            folders.data?.find((folder) => folder.id === selected)?.name ?? null,
          )}
          confirm={strings.folders.createConfirm}
          initial=""
          busy={add.isPending}
          note={writeRefusal ?? (add.isError ? strings.folders.createFailed : null)}
          onConfirm={(name) => add.mutate(name)}
          onCancel={() => {
            setCreating(false)
            setWriteRefusal(null)
          }}
        />
      )}
      {renaming === null ? null : (
        <NameDialog
          title={strings.folders.renameTitle(renaming.name)}
          confirm={strings.folders.renameConfirm}
          initial={renaming.name}
          hint={strings.folders.renameKeepsDirectory(renaming.slug)}
          busy={rename.isPending}
          note={writeRefusal ?? (rename.isError ? strings.folders.renameFailed : null)}
          onConfirm={(name) => rename.mutate({ folder: renaming, name })}
          onCancel={() => {
            setRenaming(null)
            setWriteRefusal(null)
          }}
        />
      )}
      {pendingDelete === null ? null : (
        <DeleteDialog
          folder={pendingDelete}
          subcategories={subcategoryCount(folders.data ?? [], pendingDelete.id)}
          busy={remove.isPending}
          note={remove.isError ? strings.folders.deleteFailed : null}
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

/**
 * How many categories sit under `root`, not counting `root` itself.
 *
 * `FolderNode` carries a part count and no folder count, and the delete confirmation has to
 * name both — `soft_delete_subtree` marks every descendant folder deleted as well. The tree
 * is already in hand, so this is a walk over what one request answered rather than a second
 * request per folder, and it reuses `isWithin` so there is one definition of "under" here
 * and not two that can disagree.
 */
function subcategoryCount(folders: readonly FolderNode[], root: FolderId): number {
  return folders.filter((folder) => folder.id !== root && isWithin(folders, folder.id, root)).length
}

/**
 * A failure, rendered where the thing that failed is.
 *
 * `role="alert"` because a note nobody is looking at is a note nobody gets: a keyboard user
 * pressing "Move here" has no reason to be reading the foot of the sidebar, and every one of
 * these was a plain muted `<p>` that announced nothing. `alert` rather than `status` — each
 * of these says an action did not happen, which is not a progress update.
 */
function RowNote({ note }: { note: string | null }) {
  if (note === null) return null
  return (
    <p role="alert" className="mt-1 text-sm text-[var(--color-muted)]">
      {note}
    </p>
  )
}

/**
 * The wording for a refused create or rename. Beside `refusalMessage` and deliberately not
 * merged with it: these are different routes refusing different things, and one switch over
 * a nine-value union would be a switch where most arms are unreachable from most callers.
 */
function folderWriteMessage(reason: FolderWriteRefusal): string {
  switch (reason) {
    case 'nameTaken':
      return strings.folders.nameTaken
    case 'slugTaken':
      return strings.folders.slugTaken
    case 'emptyName':
      return strings.folders.emptyName
    case 'gone':
      return strings.folders.writeGone
    case 'unknown':
      return strings.folders.writeUnknown
  }
}

function FolderLevel({
  folders,
  parentId,
  depth,
  selected,
  onSelect,
  onDropPart,
  onDelete,
  onRename,
  noteFor,
}: {
  folders: readonly FolderNode[]
  parentId: FolderId | null
  depth: number
  selected: FolderId | null
  onSelect: (folder: FolderId) => void
  onDropPart: (event: DragEvent<HTMLElement>, folder: FolderId) => void
  onDelete: (folder: FolderNode) => void
  onRename: (folder: FolderNode) => void
  noteFor: (folder: FolderId) => string | null
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

              Transparent means untappable, and on a touch screen it means neither. An
              invisible control that still takes taps is a delete nobody meant to press,
              so `pointer-events` follows the opacity; and a device with no hover has no
              way to reveal it at all, so `pointer-coarse` shows it outright rather than
              leaving the row's only destructive action unreachable there.
            */}
            <button
              type="button"
              onClick={() => onRename(folder)}
              aria-label={strings.folders.renameFor(folder.name)}
              className="ease-mechanical pointer-events-none rounded px-1.5 py-1 text-xs text-[var(--color-muted)] opacity-0 duration-[var(--duration-fast)] group-hover:pointer-events-auto group-hover:opacity-100 pointer-coarse:pointer-events-auto pointer-coarse:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100"
            >
              {strings.folders.renameAction}
            </button>
            <button
              type="button"
              onClick={() => onDelete(folder)}
              aria-label={strings.folders.deleteFor(folder.name)}
              className="ease-mechanical pointer-events-none rounded px-1.5 py-1 text-xs text-[var(--color-muted)] opacity-0 duration-[var(--duration-fast)] group-hover:pointer-events-auto group-hover:opacity-100 pointer-coarse:pointer-events-auto pointer-coarse:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100"
            >
              {strings.folders.deleteAction}
            </button>
          </div>
          <RowNote note={noteFor(folder.id)} />
          <FolderLevel
            folders={folders}
            parentId={folder.id}
            depth={depth + 1}
            selected={selected}
            onSelect={onSelect}
            onDropPart={onDropPart}
            onDelete={onDelete}
            onRename={onRename}
            noteFor={noteFor}
          />
        </li>
      ))}
    </ul>
  )
}

/**
 * Does this drag carry one of our cards? `types` rather than `getData`: a browser withholds
 * the payload for the whole of a drag and hands over only the type list, which is exactly
 * the question being asked. Written defensively because a synthetic drag in a test carries
 * whatever the test put on it.
 */
function carriesPart(event: DragEvent<HTMLElement>): boolean {
  const types: readonly string[] | undefined = event.dataTransfer?.types
  return types?.includes(PART_DRAG_TYPE) ?? false
}

/**
 * One row: the filter, and the drop target for a dragged card.
 *
 * `onDragOver` calls `preventDefault()` because that is what marks an element as willing
 * to accept a drop — without it the browser never fires `drop` at all, and jsdom will not
 * notice the omission. It calls it only for our own type: doing it unconditionally
 * advertised every category as a drop target for anything a desktop can drag — a file, a
 * URL, a text selection — so the row lifted, took the drop, and discarded it in silence.
 *
 * The affordance is 120 ms on transform and opacity, per `CLAUDE.md`: the row lifts while
 * a card is over it. Tailwind emits `-translate-y-px` as the `translate` property, so
 * `styles.css` transitions `translate` alongside `transform` — without it every lift in
 * the app snapped, including this one. The border colour changes with it and is not
 * animated; nothing here widens that list to colours.
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
        if (!carriesPart(event)) return
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
  const { move, duplicate, refusal, start, confirm, dismiss } = useMovePart(library, onClose)
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
      <RowNote
        note={
          move.isError
            ? strings.folders.moveFailed
            : refusal !== null
              ? refusalMessage(refusal.reason)
              : null
        }
      />
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
 * other.
 *
 * Two counts, because a delete takes two kinds of thing. The models come from
 * `FolderNode.partCount`, which the one tree request already carried and which counts the
 * whole subtree; the subcategories are counted off that same tree. Naming only the models
 * let a category holding twelve empty subcategories confirm with "No models are inside it"
 * and then take twelve rows off the sidebar.
 */
function DeleteDialog({
  folder,
  subcategories,
  busy,
  note,
  onConfirm,
  onCancel,
}: {
  folder: FolderNode
  subcategories: number
  busy: boolean
  note: string | null
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <Dialog title={strings.folders.deleteTitle(folder.name)} onClose={onCancel}>
      <p className="mt-2 text-sm text-[var(--color-muted)]">
        {strings.folders.deleteBody(folder.partCount, subcategories)}
      </p>
      {/*
        A delete that fails leaves this dialog open, so this is where the failure has to be
        said — and said out loud, since the user is looking at a confirmation that appears
        to have done nothing.
      */}
      <RowNote note={note} />
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
 * One text field, a confirm and a cancel — the whole of both create and rename.
 *
 * One component and not two, because the difference between them is the title, the button
 * label, what the field starts with and one extra sentence; everything else — the trim, the
 * disabled-while-empty confirm, Enter to submit, where a refusal is shown — is identical,
 * and two copies of it would be two places for those to drift.
 *
 * **Confirm is autofocused here, where `DeleteDialog` autofocuses cancel.** The rule is the
 * same in both: focus the safe answer. Naming a category is not destructive and the field is
 * where a user is going anyway; a delete is, so it opens on the way out.
 *
 * The field is a real `<form>` so that Enter submits, which is what anyone typing a name
 * expects and what a bare `<input>` beside a `<button>` does not give.
 */
function NameDialog({
  title,
  confirm,
  initial,
  hint,
  busy,
  note,
  onConfirm,
  onCancel,
}: {
  title: string
  confirm: string
  initial: string
  hint?: string
  busy: boolean
  note: string | null
  onConfirm: (name: string) => void
  onCancel: () => void
}) {
  const [name, setName] = useState(initial)
  // Trimmed here and again at the server. Here so the confirm cannot be pressed on a name
  // made of spaces; there because the client is not what decides what a valid name is.
  const trimmed = name.trim()
  return (
    <Dialog title={title} onClose={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (trimmed !== '' && !busy) onConfirm(trimmed)
        }}
      >
        <input
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label={strings.folders.createLabel}
          autoFocus
          className="mt-3 w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-sm"
        />
        {hint === undefined ? null : (
          <p className="mt-2 text-xs text-[var(--color-muted)]">{hint}</p>
        )}
        <RowNote note={note} />
        <div className="mt-4 flex justify-end gap-2">
          <DialogButton onClick={onCancel}>{strings.folders.cancel}</DialogButton>
          {/*
            `type="submit"` so the form's own handler runs for both Enter and the click, and
            there is one path into `onConfirm` rather than two that could disagree about the
            trim.
          */}
          <button
            type="submit"
            disabled={busy || trimmed === ''}
            className="ease-mechanical rounded border border-[var(--color-border)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {confirm}
          </button>
        </div>
      </form>
    </Dialog>
  )
}

/** Everything inside the box that a Tab can land on. `:not([disabled])` is the point. */
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * The shell every dialog here shares: `role="dialog"` + `aria-modal`, Escape, a focus trap
 * and a portal. The caller autofocuses whichever control the safe answer is — cancel where
 * the action is destructive, confirm where it is not.
 *
 * **Portaled to `<body>`, and that is a layout fix before it is an accessibility one.** A
 * card is `overflow-hidden hover:-translate-y-0.5`; Tailwind 4 emits `-translate-y-*` as
 * the `translate` property, and a `translate` other than `none` makes an element a
 * containing block for fixed-position descendants (CSS Transforms 2 §3, which names
 * `translate` alongside `transform`). Rendered inside the card, this overlay's `fixed
 * inset-0` therefore resolved against the card's padding box and was clipped to it for as
 * long as the pointer stayed on the card — a squashed panel inside an 11rem card that
 * snapped to a full-viewport modal when the mouse left. The keyboard path never showed it,
 * because a keyboard never hovers. jsdom computes no layout, so no test here can see it
 * either; the fix is structural, and the portal is what makes it structural.
 *
 * **Escape is listened for on the document, not on the overlay.** Pressing an action
 * disables the button that had focus, a disabled button loses it, and focus falls to
 * `<body>` — which is not a descendant of the overlay, so an `onKeyDown` there stops
 * receiving keys and the dialog becomes keyboard-undismissable exactly when a request is
 * in flight or has just been refused. The document hears the key wherever focus went.
 *
 * **The trap is Tab-shaped rather than `inert`-shaped** for the same reason: `inert` on
 * the background needs a wrapper this component does not own, while wrapping Tab at the
 * two ends of the box — and pulling focus back in when it is nowhere — is the whole of
 * what `aria-modal="true"` is currently asserting and nothing was enforcing.
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
  const box = useRef<HTMLDivElement>(null)
  // Every caller passes an inline arrow, so `onClose` is a new function each render. Read
  // through a ref rather than depending on it: an effect keyed on the callback would tear
  // down and re-run on every render, and its cleanup would throw focus back at the trigger
  // while the dialog was still open.
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })

  /**
   * Whatever had focus when this dialog was written, read during render and not in an
   * effect: React applies `autoFocus` in the commit phase, before any effect here runs, so
   * an effect reading `document.activeElement` finds the dialog's own cancel button and
   * would then "restore" focus to a control it is about to unmount.
   */
  const opener = useRef<Element | null>(null)
  if (opener.current === null) {
    opener.current = document.activeElement
  }

  useEffect(() => {
    const node = box.current
    // A fallback, never a preference: `autoFocus` has already run by now and put focus on
    // the safe control. This only fires when nothing inside took it.
    if (node !== null && !node.contains(document.activeElement)) {
      node.focus()
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close.current()
        return
      }
      if (event.key !== 'Tab') return
      const active = document.activeElement
      const current = node
      if (current === null) return
      const focusable = Array.from(current.querySelectorAll<HTMLElement>(FOCUSABLE))
      if (!current.contains(active) || active === current) {
        // Focus is not on a control in here: either outside the dialog altogether, or on
        // the box itself, which is where it is parked while every control is disabled by
        // the action one of them started. Put it on a control rather than making the user
        // tab in from the top of the document.
        ;(focusable[0] ?? current).focus()
        event.preventDefault()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (first === undefined || last === undefined) return
      if (event.shiftKey && active === first) {
        last.focus()
        event.preventDefault()
      } else if (!event.shiftKey && active === last) {
        first.focus()
        event.preventDefault()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      // Back to whatever opened this. A dialog that closes onto `<body>` costs a keyboard
      // user their place in the page, and there is no reason for them to hunt for it.
      const trigger = opener.current
      if (trigger instanceof HTMLElement && document.contains(trigger)) {
        trigger.focus()
      }
    }
  }, [])

  return createPortal(
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6">
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        // Focusable only programmatically: it holds focus while every control inside is
        // disabled, which is the window in which focus would otherwise be nowhere.
        tabIndex={-1}
        onBlur={(event) => {
          const current = box.current
          if (current !== null && !current.contains(event.relatedTarget)) {
            current.focus()
          }
        }}
        className="w-full max-w-md rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      >
        <h2 id={titleId} className="text-sm font-medium">
          {title}
        </h2>
        {children}
      </div>
    </div>,
    document.body,
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
