import { useState } from 'react'
import { strings } from '../lib/strings'
import { DIALOG_FOOT, DIALOG_QUIET, Dialog } from './Dialog'
import { LEAD, STANDING } from './Page'

/**
 * The address of a model's page, for an import (F3). The server's refusal — an unsupported site, a
 * missing key, importing switched off — is shown in the dialog that caused it, in the server's words.
 */
export function ImportLinkDialog({
  busy,
  note,
  onImport,
  onCancel,
}: {
  busy: boolean
  note: string | null
  onImport: (url: string) => void
  onCancel: () => void
}) {
  const [url, setUrl] = useState('')
  const trimmed = url.trim()
  return (
    <Dialog title={strings.importLink.title} onClose={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (trimmed !== '' && !busy) onImport(trimmed)
        }}
      >
        <p className={LEAD}>{strings.importLink.lead}</p>
        <input
          type="url"
          inputMode="url"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          aria-label={strings.importLink.label}
          autoFocus
          autoComplete="url"
          spellCheck={false}
          className="mt-4 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)]"
        />
        {note === null ? null : (
          <p role="alert" className="mt-2 max-w-[70ch] text-sm text-[var(--color-text)]">
            {note}
          </p>
        )}
        <div className={DIALOG_FOOT}>
          <button type="button" onClick={onCancel} className={DIALOG_QUIET}>
            {strings.folders.cancel}
          </button>
          <button
            type="submit"
            disabled={trimmed === '' || busy}
            className={trimmed === '' || busy ? DIALOG_QUIET : STANDING}
          >
            {strings.importLink.confirm}
          </button>
        </div>
      </form>
    </Dialog>
  )
}
