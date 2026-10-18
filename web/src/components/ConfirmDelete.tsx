import { DIALOG_FOOT, DIALOG_QUIET, Dialog } from './Dialog'
import { LEAD, STANDING } from './Page'
import { strings } from '../lib/strings'

/**
 * The one confirmation in front of deleting parts, from the part page and from the grid's
 * selection alike. Delete is final (owner, 2026-10-17), so the body says what goes and what
 * stays: the part's rows go, the model file on disk does not. Cancel is autofocused because it
 * is the safe answer, as `FolderTree`'s delete dialog does. Delete is the dialog's verb, so it stands; Cancel is
 * quiet, and focus starting on it is what keeps a stray Enter safe.
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
      <p className={LEAD}>{body}</p>
      {failure === null ? null : (
        <p role="alert" className="mt-3 max-w-[70ch] text-sm text-[var(--color-text)]">
          {failure}
        </p>
      )}
      <div className={DIALOG_FOOT}>
        <button type="button" autoFocus onClick={onCancel} className={DIALOG_QUIET}>
          {strings.folders.cancel}
        </button>
        <button type="button" disabled={busy} onClick={onConfirm} className={busy ? DIALOG_QUIET : STANDING}>
          {busy ? strings.removal.removing : strings.removal.confirm}
        </button>
      </div>
    </Dialog>
  )
}
