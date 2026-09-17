import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { fetchPeers, previewShare, shareCategory } from '../lib/api'
import { strings } from '../lib/strings'
import type { FolderNode, LibraryId } from '../lib/types'
import { Dialog } from './Dialog'

const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * Sharing a category: what it would offer, said before anything is sent.
 *
 * A share is the category and everything under it, parts filed there later included, offered to the people
 * ticked here — so the dialog counts the parts, and how many carry no licence or a non-commercial one. The
 * owner decided the warning is shown and never blocks, so Share is not disabled by it, only by the count
 * not having arrived: nobody confirms a warning they have not been shown.
 *
 * It opens on Cancel, as the delete confirmation does, because this sends a category to other people.
 */
export function ShareDialog({
  library,
  folder,
  alreadyShared,
  onClose,
}: {
  library: LibraryId
  folder: FolderNode
  /** Sharing it again — to ask first, say — which must not quietly widen who it already goes to. */
  alreadyShared: boolean
  onClose: () => void
}) {
  const queryClient = useQueryClient()
  const warning = useQuery({
    queryKey: ['shares', 'preview', library, folder.id],
    queryFn: () => previewShare(library, folder.id),
  })
  const peers = useQuery({
    queryKey: ['sharing', 'peers'],
    queryFn: fetchPeers,
    enabled: !alreadyShared,
  })
  const [note, setNote] = useState<string | null>(null)
  const [asksFirst, setAsksFirst] = useState(false)
  // Everybody, until somebody is unticked: sharing with the people you know is what this did before member
  // lists, and the dialog should not make a person choose to keep that. The list is sent only once the people
  // are known — naming nobody would share a folder with no one, so a dialog confirmed before they arrived, or
  // with nobody paired yet, sends no list and the share reaches whoever is paired, as it always did. A folder
  // already shared sends none either: this dialog does not know who it goes to, and everyone ticked here would
  // widen a list somebody picked.
  const [dropped, setDropped] = useState<ReadonlySet<string>>(new Set())
  const chosen =
    !alreadyShared && peers.data?.length
      ? peers.data.filter((peer) => !dropped.has(peer.deviceId)).map((peer) => peer.deviceId)
      : undefined
  const share = useMutation({
    mutationFn: () => shareCategory(library, folder.id, asksFirst, chosen),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      // Every list of shares: this library's, which marks the tree, and the sharing page's.
      void queryClient.invalidateQueries({ queryKey: ['shares'] })
      onClose()
    },
    onError: () => setNote(strings.sharing.shareFailed),
  })
  return (
    <Dialog title={strings.sharing.shareTitle(folder.name)} onClose={onClose}>
      {warning.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.sharing.shareCounting}</p>
      ) : warning.isError ? (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {strings.sharing.shareCountFailed}
        </p>
      ) : (
        <>
          <p className="mt-2 max-w-prose text-sm">{strings.sharing.shareBody(warning.data.parts)}</p>
          {warning.data.unrecorded === 0 && warning.data.nonCommercial === 0 ? (
            <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
              {strings.sharing.shareLicencesClear}
            </p>
          ) : (
            <ul role="list" className="mt-2 flex max-w-prose list-none flex-col gap-1 text-sm">
              {warning.data.unrecorded > 0 ? (
                <li>{strings.sharing.shareUnrecorded(warning.data.unrecorded)}</li>
              ) : null}
              {warning.data.nonCommercial > 0 ? (
                <li>{strings.sharing.shareNonCommercial(warning.data.nonCommercial)}</li>
              ) : null}
            </ul>
          )}
        </>
      )}
      <fieldset className="mt-4">
        <legend className="text-xs tracking-wider text-[var(--color-muted)] uppercase">
          {strings.sharing.membersLabel}
        </legend>
        {alreadyShared ? (
          <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">{strings.sharing.membersKept}</p>
        ) : peers.data === undefined ? null : peers.data.length === 0 ? (
          <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
            {strings.sharing.membersNobodyPaired}
          </p>
        ) : (
          <ul role="list" className="mt-2 flex list-none flex-col gap-1">
            {peers.data.map((peer) => (
              <li key={peer.deviceId}>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={!dropped.has(peer.deviceId)}
                    onChange={(event) =>
                      setDropped((current) => {
                        const next = new Set(current)
                        if (event.target.checked) next.delete(peer.deviceId)
                        else next.add(peer.deviceId)
                        return next
                      })
                    }
                  />
                  {peer.name ?? strings.sharing.unnamed}
                  <span className="tabular text-xs text-[var(--color-muted)]">{peer.address}</span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </fieldset>
      <label className="mt-4 flex items-center gap-2 text-sm">
        <input type="checkbox" checked={asksFirst} onChange={(event) => setAsksFirst(event.target.checked)} />
        {strings.sharing.askFirstLabel}
      </label>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.askFirstNote}</p>
      {note === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={onClose} autoFocus className={BUTTON}>
          {strings.sharing.shareCancel}
        </button>
        <button
          type="button"
          onClick={() => share.mutate()}
          disabled={share.isPending || !warning.isSuccess}
          className={BUTTON}
        >
          {share.isPending ? strings.sharing.sharingNow : strings.sharing.shareConfirm}
        </button>
      </div>
    </Dialog>
  )
}
