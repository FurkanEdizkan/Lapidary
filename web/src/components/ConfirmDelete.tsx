import { Dialog } from './Dialog'
import { strings } from '../lib/strings'

const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * The one confirmation in front of deleting parts, from the part page and from the grid's
 * selection alike. Delete is final (owner, 2026-10-17), so the body says what goes and what
 * stays: the part's rows go, the model file on disk does not. Cancel is autofocused because it
 * is the safe answer, as `FolderTree`'s delete dialog does.
 */
export function ConfirmDelete({
  title,
  body,
  busy = false,
  failure = null,
  onConfirm,
  onCancel,
}: {
  title: string
  body: string
  busy?: boolean
  /** Said inside the dialog, which stays open when a delete fails. */
  failure?: string | null
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <Dialog title={title} onClose={onCancel}>
      <p className="mt-2 text-sm text-[var(--color-muted)]">{body}</p>
      {failure === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {failure}
        </p>
      )}
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" autoFocus onClick={onCancel} className={BUTTON}>
          {strings.folders.cancel}
        </button>
        <button type="button" disabled={busy} onClick={onConfirm} className={BUTTON}>
          {busy ? strings.removal.removing : strings.removal.confirm}
        </button>
      </div>
    </Dialog>
  )
}
