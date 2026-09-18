/*
  Ported from the three real callers in `src/components/FolderTree.tsx` — DeleteDialog,
  NameDialog and their refusal path. The children are the callers' own markup, class for
  class, because `Dialog` renders nothing but the scrim, the panel, the title row and the
  close button: everything below the title is composed by whoever opens it, and a preview
  that invented its own body would teach the wrong shape.

  `DialogButton` is internal to FolderTree, so the buttons here carry its classes directly —
  the same string the app renders.
*/
import { Dialog } from 'lapidary-web'

import type React from 'react'

/**
 * The app's page ground. Lapidary paints it on `:root`, and the preview page's own white `body`
 * covers that — so every preview lays its ground down itself, filling the cell, as the app's
 * screen would be.
 */
function Ground({ children, padded = true }: { children: React.ReactNode; padded?: boolean }) {
  return (
    <div
      style={{
        background: 'var(--color-bg)',
        color: 'var(--color-text)',
        minHeight: 'calc(100vh - 48px)',
        padding: padded ? 16 : 0,
      }}
    >
      {children}
    </div>
  )
}

const button =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/** A destructive confirmation. Cancel is autofocused — the safe answer, per the component's own rule. */
export function Confirmation() {
  return (
    <Ground>
    <Dialog title="Delete Flanges DN40?" onClose={() => {}}>
      <p className="mt-2 text-sm text-[var(--color-muted)]">
        18 parts and 2 subcategories move to Removed. Nothing is erased — purge is a separate,
        explicit action.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        {/*
          Cancel is autofocused, as DeleteDialog does it: focus the safe answer. It is also
          what keeps the ring off the panel — `Dialog` focuses its own box only when nothing
          inside has taken focus, so a preview without this shows a state no caller produces.
        */}
        <button type="button" autoFocus className={button}>
          Cancel
        </button>
        <button type="button" className={button}>
          Delete
        </button>
      </div>
    </Dialog>
    </Ground>
  )
}

/** Create and rename: one field, a hint, a confirm disabled until the name is non-empty. */
export function Rename() {
  return (
    <Ground>
    <Dialog title="Rename flange-dn40-lp-3310-02" onClose={() => {}}>
      <form>
        <input
          type="text"
          defaultValue="flange-dn40-lp-3310-02"
          aria-label="Category name"
          autoFocus
          className="mt-3 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
        />
        <p className="mt-2 text-xs text-[var(--color-muted)]">
          Used in the folder path. Letters, digits and dashes.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className={button}>
            Cancel
          </button>
          <button type="submit" className={button}>
            Rename
          </button>
        </div>
      </form>
    </Dialog>
    </Ground>
  )
}

/**
 * A refused write. The dialog stays open and says why — the failure has to be readable
 * here, because the person is looking at a confirmation that appears to have done nothing.
 */
export function Refused() {
  return (
    <Ground>
    <Dialog title="New category in Flanges DN40" onClose={() => {}}>
      <form>
        <input
          type="text"
          defaultValue="Weld neck"
          aria-label="Category name"
          autoFocus
          className="mt-3 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
        />
        <p role="alert" className="mt-1 text-sm text-[var(--color-muted)]">
          A category called Weld neck is already here.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className={button}>
            Cancel
          </button>
          <button type="submit" disabled className={button}>
            Create
          </button>
        </div>
      </form>
    </Dialog>
    </Ground>
  )
}
