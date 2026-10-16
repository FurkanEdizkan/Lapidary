import { useState } from 'react'
import type { FileRoot } from '../bindings/FileRoot'
import type { InstanceStorageView } from '../bindings/InstanceStorageView'
import { strings } from '../lib/strings'

/**
 * "Where is this model on my disk?" — answered as selectable text, never by opening a file
 * manager, because a browser cannot and pretending otherwise would be a control that lies.
 *
 * Lives here rather than beside the grid because both surfaces need it. It used to be
 * module-local to `routes/index.tsx` and rendered only inside the quick-look panel, which a
 * keyboard cannot open — so the one control that answers this product's "the store stays
 * legible" promise was mouse-only. WCAG 2.2 SC 2.1.1 is about whether a function is
 * available at all.
 */
/** Where each folder a file can be in is on the host, when the deployment has said (`null` otherwise). */
export type HostRoots = Record<FileRoot, string | null>

const NO_HOST_ROOTS: HostRoots = { library: null, inbox: null, store: null }

/** The host roots `GET /api/storage` reports, or none while it has not answered. */
export function hostRootsOf(view: InstanceStorageView | undefined): HostRoots {
  if (view === undefined) return NO_HOST_ROOTS
  return {
    library: view.hostIngestRoot ?? null,
    inbox: view.hostInboxRoot ?? null,
    store: view.hostStorageRoot ?? null,
  }
}

/** The `deploy/.env` variable that names each folder, for the copy that says how to see it whole. */
const ROOT_VARIABLE: Record<FileRoot, string> = {
  library: 'LAPIDARY_INGEST_DIR',
  inbox: 'LAPIDARY_INBOX_DIR',
  store: 'LAPIDARY_STORAGE_ROOT',
}

export function ShowInFolder({
  part,
  hostRoots,
}: {
  /**
   * Anything that knows its own name, where its bytes sit and which folder that path is in.
   * `PartCard` and `PartDetail` both satisfy it, which is the point — this control has to exist
   * on the grid's panel and on the part's own page, and the page is the surface a keyboard reaches.
   */
  part: { name: string; storagePath: string | null; fileRoot: FileRoot | null }
  /**
   * Where each folder is on the host, each `null` when the deployment has not said.
   *
   * Never derived here and never guessed. The api sees the folders at container paths that
   * exist on nobody's machine, so when a root is `null` the honest answer is the path within it
   * — which is what the copy then says, along with how to fix it.
   */
  hostRoots: HostRoots
}) {
  const [open, setOpen] = useState(false)
  // A row from before M4 has no recorded root and is the store's.
  const root: FileRoot = part.fileRoot ?? STORE
  const hostRoot = hostRoots[root]
  // The file, not its directory: "where is this model" is answered by the path to the
  // model, and the directory is one `rsplit` away for anyone who wants it. Narrowed with
  // `typeof` rather than in the JSX because a `!== 'string'` inside a child expression puts
  // the literal `'string'` where `no-bare-strings.test.ts` reads it — correctly — as a
  // label reaching the screen.
  const relative = typeof part.storagePath === 'string' ? part.storagePath : null
  // Joined with a single slash and no path library: a host root is absolute or absent (the
  // server drops a relative one), and a relative path never starts with one, so the only
  // case to handle is a trailing slash on the root.
  const path =
    relative === null ? null : hostRoot === null ? relative : `${hostRoot.replace(/\/$/, '')}/${relative}`
  return (
    <div className="mt-2 text-xs text-[var(--color-muted)]">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-label={strings.folders.showInFolderFor(part.name)}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.folders.showInFolder}
      </button>
      {!open ? null : path === null ? (
        <p className="mt-2">{strings.folders.directoryPending}</p>
      ) : (
        <div className="mt-2 space-y-2">
          {/* `select-all` so one click takes the whole path, which is what a person does
              with it — and `break-all` because a nested category path is longer than a
              card is wide. */}
          <code className="block font-mono break-all select-all text-[var(--color-text)]">
            {path}
          </code>
          <button
            type="button"
            onClick={() => {
              // Absent in an insecure context, and a rejected permission is not worth an
              // error state: the path is on screen and selectable either way.
              void navigator.clipboard?.writeText(path).catch(() => undefined)
            }}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.copyPath}
          </button>
          <p>{hostRoot === null ? strings.folders.directoryHint : strings.folders.directoryHintAbsolute}</p>
          {hostRoot === null ? (
            <p>{strings.folders.directoryPartial(strings.folders.rootFolder[root], ROOT_VARIABLE[root])}</p>
          ) : null}
        </div>
      )}
    </div>
  )
}

const STORE: FileRoot = 'store'
