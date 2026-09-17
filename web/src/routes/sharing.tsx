import { Link, createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  addPeer,
  decideGrant,
  fetchPeerShares,
  fetchPeers,
  fetchPulls,
  fetchShareRequests,
  fetchShares,
  fetchSharingIdentity,
  removePeer,
  setSharingName,
  stopSharing,
} from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD, SECTION, SECTION_TITLE } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import type { Peer, Pull, ShareRequest, ShareSummary } from '../lib/types'

const CONTROL =
  'mt-0.5 block w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 text-sm'
const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * How often the page asks again, so somebody's status changes while it is open. The peer role says
 * hello every fifteen seconds; a third of that keeps the page no more than one round behind.
 */
const REFRESH_MS = 5000

/**
 * Shared libraries: this installation's device id and name, and the people it is paired with.
 *
 * Pairing is two people each pasting the other's id and address, so the page's first job is to show
 * this installation's id plainly enough to read off to somebody, and its second is to say, for each
 * person, whether their machine is answering — and when it is not, the peer role's own sentence for why,
 * because "offline" alone cannot tell a machine that is switched off from one that has not added this one.
 */
export const Route = createFileRoute('/sharing')({ component: SharingPage })

export function SharingPage() {
  return (
    <AppFrame current="sharing">
      <section className="max-w-3xl">
        <title>{strings.titles.sharing}</title>
        <h2 className={HEADLINE}>{strings.sharing.title}</h2>
        <p className={LEAD}>{strings.sharing.lead}</p>
        <ThisInstallation />
        <OwnShares />
        <Requests />
        <Pulls />
        <People />
      </section>
    </AppFrame>
  )
}

function ThisInstallation() {
  const identity = useQuery({
    queryKey: ['sharing', 'identity'],
    queryFn: fetchSharingIdentity,
    // The id appears the moment the peer role first starts, which may be while this page is open.
    refetchInterval: REFRESH_MS,
  })
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.thisInstallation}</h3>
      {identity.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
      ) : identity.isError ? (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {strings.sharing.loadFailed}
        </p>
      ) : identity.data.deviceId === null ? (
        <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">{strings.sharing.off}</p>
      ) : (
        <>
          <DeviceId id={identity.data.deviceId} />
          {/* Keyed by the id, so a name loaded after the form first rendered still fills it. */}
          <NameForm key={identity.data.deviceId} name={identity.data.name} />
        </>
      )}
    </div>
  )
}

function DeviceId({ id }: { id: string }) {
  const [copied, setCopied] = useState(false)
  // Absent over plain HTTP on a LAN, which is how most people will reach this page. The id selects in one
  // click either way, so the button is an extra, never the only way to take the id.
  const clipboard = typeof navigator !== 'undefined' ? navigator.clipboard : undefined
  return (
    <div className="mt-2">
      <p className="text-xs text-[var(--color-muted)]">{strings.sharing.deviceId}</p>
      <div className="mt-1 flex flex-wrap items-center gap-2">
        <code className="rounded-[var(--radius-ctl)] bg-[var(--color-raised)] px-2 py-1 font-mono text-sm break-all select-all">
          {id}
        </code>
        {clipboard === undefined ? null : (
          <button
            type="button"
            onClick={() => {
              void clipboard.writeText(id).then(() => setCopied(true))
            }}
            className={BUTTON}
          >
            {copied ? strings.sharing.copied : strings.sharing.copy}
          </button>
        )}
      </div>
    </div>
  )
}

function NameForm({ name }: { name: string | null }) {
  const queryClient = useQueryClient()
  const [typed, setTyped] = useState(name ?? '')
  const [note, setNote] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: () => setSharingName(typed.trim() === '' ? null : typed),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      setNote(null)
      void queryClient.invalidateQueries({ queryKey: ['sharing', 'identity'] })
    },
    onError: () => setNote(strings.sharing.refusedWithoutReason),
  })
  return (
    <form
      className="mt-3 flex flex-wrap items-end gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (!save.isPending) save.mutate()
      }}
    >
      <label className="min-w-56 flex-1 text-xs text-[var(--color-muted)]">
        {strings.sharing.nameField}
        <input
          value={typed}
          placeholder={strings.sharing.namePlaceholder}
          onChange={(event) => setTyped(event.target.value)}
          className={CONTROL}
        />
      </label>
      <button type="submit" disabled={save.isPending} className={BUTTON}>
        {save.isPending ? strings.sharing.savingName : strings.sharing.saveName}
      </button>
      {note === null ? null : (
        <p role="alert" className="w-full text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </form>
  )
}

function People() {
  const queryClient = useQueryClient()
  const peers = useQuery({ queryKey: ['sharing', 'peers'], queryFn: fetchPeers, refetchInterval: REFRESH_MS })
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['sharing', 'peers'] })
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.people}</h3>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.removeNote}</p>
      <PairForm onPaired={refresh} />
      {peers.isPending ? (
        <p className="mt-4 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
      ) : peers.isError ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-muted)]">
          {strings.sharing.loadFailed}
        </p>
      ) : peers.data.length === 0 ? (
        <p className="mt-4 text-sm text-[var(--color-muted)]">{strings.sharing.none}</p>
      ) : (
        <ul role="list" className="mt-4 flex flex-col gap-2">
          {peers.data.map((peer) => (
            <PeerRow key={peer.deviceId} peer={peer} onRemoved={refresh} />
          ))}
        </ul>
      )}
    </div>
  )
}

function PairForm({ onPaired }: { onPaired: () => Promise<void> }) {
  const [deviceId, setDeviceId] = useState('')
  const [address, setAddress] = useState('')
  const [note, setNote] = useState<string | null>(null)
  const pair = useMutation({
    mutationFn: () => addPeer(deviceId, address),
    onSuccess: (result) => {
      // A refusal keeps what was typed: the fix is usually one character, not a fresh paste.
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      setNote(null)
      setDeviceId('')
      setAddress('')
      void onPaired()
    },
    onError: () => setNote(strings.sharing.refusedWithoutReason),
  })
  return (
    <form
      className="mt-3 flex flex-wrap items-end gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        if (!pair.isPending) pair.mutate()
      }}
    >
      <label className="min-w-72 flex-[2] text-xs text-[var(--color-muted)]">
        {strings.sharing.pairDeviceId}
        <input
          value={deviceId}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setDeviceId(event.target.value)}
          className={`${CONTROL} font-mono`}
        />
      </label>
      <label className="min-w-44 flex-1 text-xs text-[var(--color-muted)]">
        {strings.sharing.pairAddress}
        <input
          value={address}
          autoComplete="off"
          spellCheck={false}
          placeholder={strings.sharing.addressPlaceholder}
          onChange={(event) => setAddress(event.target.value)}
          className={CONTROL}
        />
      </label>
      <button type="submit" disabled={pair.isPending} className={BUTTON}>
        {pair.isPending ? strings.sharing.pairing : strings.sharing.pair}
      </button>
      {note === null ? null : (
        <p role="alert" className="w-full text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </form>
  )
}

function PeerRow({ peer, onRemoved }: { peer: Peer; onRemoved: () => Promise<void> }) {
  const [note, setNote] = useState<string | null>(null)
  const remove = useMutation({
    mutationFn: () => removePeer(peer.deviceId),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      void onRemoved()
    },
    onError: () => setNote(strings.sharing.refusedWithoutReason),
  })
  const status = peer.online
    ? strings.sharing.online
    : peer.lastSeenAt === null
      ? strings.sharing.notReached
      : strings.sharing.lastSeen(peer.lastSeenAt)
  return (
    <li className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <span className="grow">
          <span className="text-sm">{peer.name ?? strings.sharing.unnamed}</span>
          <span
            className={`ml-2 text-xs ${peer.online ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted)]'}`}
          >
            {status}
          </span>
        </span>
        <button
          type="button"
          onClick={() => remove.mutate()}
          disabled={remove.isPending}
          aria-label={strings.sharing.removeLabel(peer.name ?? peer.deviceId)}
          className={`${BUTTON} text-[var(--color-muted)]`}
        >
          {remove.isPending ? strings.sharing.removing : strings.sharing.removeButton}
        </button>
      </div>
      <p className="mt-1 font-mono text-xs break-all text-[var(--color-muted)]">
        {peer.deviceId} · {peer.address}
      </p>
      <TheirShares deviceId={peer.deviceId} />
      {peer.online || peer.lastError === null ? null : (
        <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{peer.lastError}</p>
      )}
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </li>
  )
}

/** What this installation offers the people it is paired with, and the way to stop offering each one. */
function OwnShares() {
  const queryClient = useQueryClient()
  const shares = useQuery({ queryKey: ['shares', 'own'], queryFn: fetchShares, refetchInterval: REFRESH_MS })
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['shares'] })
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.ownShares}</h3>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.stopNote}</p>
      {shares.isPending ? (
        <p className="mt-3 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
      ) : shares.isError ? (
        <p role="alert" className="mt-3 text-sm text-[var(--color-muted)]">
          {strings.sharing.loadFailed}
        </p>
      ) : shares.data.length === 0 ? (
        <p className="mt-3 text-sm text-[var(--color-muted)]">{strings.sharing.ownSharesNone}</p>
      ) : (
        <ul role="list" className="mt-3 flex flex-col gap-2">
          {shares.data.map((share) => (
            <OwnShareRow key={share.id} share={share} onStopped={refresh} />
          ))}
        </ul>
      )}
    </div>
  )
}

function OwnShareRow({ share, onStopped }: { share: ShareSummary; onStopped: () => Promise<void> }) {
  const [note, setNote] = useState<string | null>(null)
  const stop = useMutation({
    mutationFn: () => stopSharing(share.id),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      void onStopped()
    },
    onError: () => setNote(strings.sharing.shareFailed),
  })
  return (
    <li className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <span className="grow">
          <span className="text-sm">{share.name}</span>
          <span className="ml-2 text-xs text-[var(--color-muted)]">
            {strings.sharing.ownShareParts(share.partCount)}
          </span>
          {share.asksFirst ? (
            <span className="ml-2 text-xs text-[var(--color-muted)]">{strings.sharing.asksFirst}</span>
          ) : null}
        </span>
        <button
          type="button"
          onClick={() => stop.mutate()}
          disabled={stop.isPending}
          aria-label={strings.sharing.stopSharingLabel(share.name)}
          className={`${BUTTON} text-[var(--color-muted)]`}
        >
          {stop.isPending ? strings.sharing.stopping : strings.sharing.stopSharing}
        </button>
      </div>
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </li>
  )
}

/** Who asked to pull a share that asks first, with this installation's answer and the way to give or change it. */
function Requests() {
  const queryClient = useQueryClient()
  const requests = useQuery({
    queryKey: ['shares', 'requests'],
    queryFn: fetchShareRequests,
    refetchInterval: REFRESH_MS,
  })
  if (!requests.isSuccess) return null
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.requests}</h3>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.requestsNote}</p>
      {requests.data.length === 0 ? (
        <p className="mt-3 text-sm text-[var(--color-muted)]">{strings.sharing.requestsNone}</p>
      ) : (
        <ul role="list" className="mt-3 flex flex-col gap-2">
          {requests.data.map((request) => (
            <RequestRow
              key={`${request.shareId} ${request.deviceId}`}
              request={request}
              onDecided={() => queryClient.invalidateQueries({ queryKey: ['shares', 'requests'] })}
            />
          ))}
        </ul>
      )}
    </div>
  )
}

function RequestRow({ request, onDecided }: { request: ShareRequest; onDecided: () => Promise<void> }) {
  const [note, setNote] = useState<string | null>(null)
  const decide = useMutation({
    mutationFn: (granted: boolean) => decideGrant(request.shareId, request.deviceId, granted),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      void onDecided()
    },
    onError: () => setNote(strings.sharing.requestFailed),
  })
  const who = request.name ?? request.deviceId
  const standing =
    request.state === 'granted'
      ? strings.sharing.requestGranted
      : request.state === 'denied'
        ? strings.sharing.requestDenied
        : strings.sharing.requestAsked
  return (
    <li className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <span className="grow">
          <span className="text-sm">{strings.sharing.requestLine(who, request.shareName)}</span>
          <span className="ml-2 text-xs text-[var(--color-muted)]">{standing}</span>
        </span>
        <button
          type="button"
          onClick={() => decide.mutate(true)}
          disabled={decide.isPending || request.state === 'granted'}
          aria-label={strings.sharing.grantLabel(who, request.shareName)}
          className={BUTTON}
        >
          {strings.sharing.grant}
        </button>
        <button
          type="button"
          onClick={() => decide.mutate(false)}
          disabled={decide.isPending || request.state === 'denied'}
          aria-label={strings.sharing.denyLabel(who, request.shareName)}
          className={`${BUTTON} text-[var(--color-muted)]`}
        >
          {strings.sharing.deny}
        </button>
      </div>
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </li>
  )
}

/**
 * The newest pulls, whichever share they were of. A share's own page follows its pull; this is where a pull whose share
 * was withdrawn is still found, saying so, after the mirror has let the share go.
 */
function Pulls() {
  const pulls = useQuery({ queryKey: ['sharing', 'pulls'], queryFn: fetchPulls, refetchInterval: REFRESH_MS })
  if (!pulls.isSuccess || pulls.data.length === 0) return null
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.pulls}</h3>
      <ul role="list" className="mt-3 flex flex-col gap-2">
        {pulls.data.map((pull) => (
          <li
            key={pull.id}
            className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
          >
            <p>
              {pull.sharer === null
                ? strings.sharing.pullFrom(pull.shareName)
                : strings.sharing.pullLine(pull.shareName, pull.sharer)}
            </p>
            <p className="text-xs text-[var(--color-muted)]">{pullStanding(pull)}</p>
          </li>
        ))}
      </ul>
    </div>
  )
}

function pullStanding(pull: Pull): string {
  switch (pull.state) {
    case 'done':
      return strings.sharing.pullDone(pull.filesTotal)
    case 'failed':
      return strings.sharing.pullStopped(pull.error ?? '')
    case 'paused':
      return strings.sharing.pullPaused
    case 'waiting':
      return pull.error ?? strings.sharing.pullWaiting
    case 'fetching':
      return strings.sharing.pullFetching(pull.filesDone, pull.filesTotal, pull.bytesDone, pull.bytesTotal)
    case 'importing':
      return strings.sharing.pullImportingPlain
    default:
      return strings.sharing.pullQueued
  }
}

/** What one person shares, each a link to the shared library — read from the mirror, so it lists while they are away. */
function TheirShares({ deviceId }: { deviceId: string }) {
  const shares = useQuery({
    queryKey: ['sharing', 'peers', deviceId, 'shares'],
    queryFn: () => fetchPeerShares(deviceId),
    refetchInterval: REFRESH_MS,
  })
  if (!shares.isSuccess) return null
  if (shares.data.length === 0) {
    return <p className="mt-1 text-xs text-[var(--color-muted)]">{strings.sharing.theirSharesNone}</p>
  }
  return (
    <div className="mt-2">
      <p className="text-xs text-[var(--color-muted)]">{strings.sharing.theirShares}</p>
      <ul role="list" className="mt-1 flex flex-wrap gap-2">
        {shares.data.map((share) => (
          <li key={share.id}>
            <Link
              to="/sharing/shares/$shareId"
              params={{ shareId: share.id }}
              className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:-translate-y-px"
            >
              {strings.sharing.theirShareParts(share.name, share.partCount)}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  )
}
