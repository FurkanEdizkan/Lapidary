import { useState } from 'react'
import { strings } from '../lib/strings'
import { Dialog } from './Dialog'

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
        <p className="mt-3 max-w-prose text-sm text-[var(--color-muted)]">{strings.importLink.lead}</p>
        <input
          type="url"
          inputMode="url"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          aria-label={strings.importLink.label}
          autoFocus
          className="mt-3 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
        />
        {note === null ? null : (
          <p role="alert" className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
            {note}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.cancel}
          </button>
          <button
            type="submit"
            disabled={trimmed === '' || busy}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.importLink.confirm}
          </button>
        </div>
      </form>
    </Dialog>
  )
}
