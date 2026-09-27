import { Link, createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  addPeer,
  answerIntroduction,
  decideGrant,
  fetchIntroductions,
  fetchLibraries,
  fetchLibraryShares,
  fetchPeerShares,
  fetchPeers,
  fetchPulls,
  fetchShareMembers,
  fetchShareRequests,
  fetchShares,
  fetchSharingIdentity,
  removePeer,
  shareCategory,
  setShareMembers,
  setSharingName,
  stopSharing,
} from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD, SECTION, SECTION_TITLE } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import { Dialog } from '../components/Dialog'
import { breakable } from '../components/Card'
import type {
  FolderId,
  Introduction,
  LibraryId,
  Peer,
  Pull,
  ShareRequest,
  ShareSummary,
} from '../lib/types'

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
        <Introductions />
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
        {/* `<wbr>` after each hyphen, so a narrow screen breaks the id between its groups, never inside one. */}
        <code className="rounded-[var(--radius-ctl)] bg-[var(--color-raised)] px-2 py-1 font-mono text-sm break-words select-all">
          {breakable(id)}
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
  const named = new Map(
    (peers.data ?? []).flatMap((peer) => (peer.name === null ? [] : [[peer.deviceId, peer.name] as const])),
  )
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
            <PeerRow key={peer.deviceId} peer={peer} peers={named} onRemoved={refresh} />
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

function PeerRow({
  peer,
  peers,
  onRemoved,
}: {
  peer: Peer
  /** Everybody paired, by device id, so an introducer is named rather than given as an id. */
  peers: Map<string, string>
  onRemoved: () => Promise<void>
}) {
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
  // Where they came from, for somebody a folder's owner introduced rather than somebody whose id was pasted.
  const introducer =
    peer.introducedBy === null
      ? null
      : strings.sharing.introducedBySomebody(peers.get(peer.introducedBy) ?? peer.introducedBy)
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
      {introducer === null ? null : (
        <p className="mt-1 text-xs text-[var(--color-muted)]">{introducer}</p>
      )}
      {/* Each on its own line: `break-all` on one string containing both cuts a port number in half, and a
          port is the thing a person retypes. */}
      <p className="mt-1 font-mono text-xs break-all text-[var(--color-muted)]">{peer.deviceId}</p>
      <p className="font-mono text-xs break-all text-[var(--color-muted)]">{peer.address}</p>
      <TheirShares deviceId={peer.deviceId} foldersInCommon={peer.foldersInCommon} />
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

/**
 * Which library's category each share is, by share id.
 *
 * `GET /api/shares` is the owner's list of what is shared and says nothing about where each one lives, while
 * changing how a folder is shared is `POST /api/libraries/{id}/shares` — so the switch below needs the pair.
 * One query holding the whole map rather than one per row, and a fresh read rather than whatever a library's
 * tree left in the cache: a folder shared since that page was last open would be missing from a cached list,
 * and its switch would sit there disabled with nothing to say why.
 */
function useWhereSharesLive() {
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  return useQuery({
    queryKey: ['shares', 'where', (libraries.data ?? []).map((library) => library.id)],
    enabled: libraries.isSuccess,
    queryFn: async () => {
      const lists = await Promise.all(
        (libraries.data ?? []).map(
          async (library) => [library.id, await fetchLibraryShares(library.id)] as const,
        ),
      )
      return new Map(
        lists.flatMap(([library, shares]) =>
          shares.map((share) => [share.id, { library, folder: share.folderId }] as const),
        ),
      )
    },
  })
}

/** What this installation offers the people it is paired with, and the way to stop offering each one. */
function OwnShares() {
  const queryClient = useQueryClient()
  const shares = useQuery({ queryKey: ['shares', 'own'], queryFn: fetchShares, refetchInterval: REFRESH_MS })
  const where = useWhereSharesLive()
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
            <OwnShareRow
              key={share.id}
              share={share}
              category={where.data?.get(share.id)}
              onStopped={refresh}
            />
          ))}
        </ul>
      )}
    </div>
  )
}

function OwnShareRow({
  share,
  category,
  onStopped,
}: {
  share: ShareSummary
  /** Where it lives, once that has been read: without it the folder cannot be shared again to change this. */
  category: { library: LibraryId; folder: FolderId } | undefined
  onStopped: () => Promise<void>
}) {
  const [note, setNote] = useState<string | null>(null)
  const [choosing, setChoosing] = useState(false)
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
        </span>
        {/* One group, so the two actions wrap together onto their own full-width line at phone width. Left
            to wrap one button at a time, two rows of the same list took two different shapes. */}
        <span className="flex gap-2 max-xs:w-full">
          <button type="button" onClick={() => setChoosing(true)} className={`${BUTTON} text-[var(--color-muted)]`}>
            {strings.sharing.membersChange}
          </button>
          <button
            type="button"
            onClick={() => stop.mutate()}
            disabled={stop.isPending}
            aria-label={strings.sharing.stopSharingLabel(share.name)}
            className={`${BUTTON} text-[var(--color-muted)]`}
          >
            {stop.isPending ? strings.sharing.stopping : strings.sharing.stopSharing}
          </button>
        </span>
      </div>
      <Members share={share} />
      <AskFirst share={share} category={category} />
      {choosing ? <ChooseMembers share={share} onClose={() => setChoosing(false)} /> : null}
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </li>
  )
}

/**
 * Whether a folder already shared asks before anybody pulls its files, and the way to change that either way.
 *
 * Changing it is sharing the folder again with a different standing — `POST /api/libraries/{id}/shares`, the
 * only call there is — so the switch waits for the row to know which category that is, and shows where the
 * folder stands meanwhile rather than hiding. Switching it off says so once, because the asks about that
 * folder leave the list above: everybody it reaches may pull it, so there is nothing left to decide, and
 * switching it on again brings them back with what was answered.
 */
function AskFirst({
  share,
  category,
}: {
  share: ShareSummary
  category: { library: LibraryId; folder: FolderId } | undefined
}) {
  const queryClient = useQueryClient()
  const [note, setNote] = useState<string | null>(null)
  const set = useMutation({
    mutationFn: ({
      library,
      folder,
      asksFirst,
    }: {
      library: LibraryId
      folder: FolderId
      asksFirst: boolean
    }) => shareCategory(library, folder, asksFirst),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      setNote(null)
      void queryClient.invalidateQueries({ queryKey: ['shares'] })
    },
    onError: () => setNote(strings.sharing.askFirstFailed),
  })
  // Said at the moment it is switched off, where it answers "what happened to the people who asked?", and not
  // on every folder that has never asked first.
  const justOpened = set.isSuccess && set.variables.asksFirst === false
  return (
    <div className="mt-2">
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={share.asksFirst}
          disabled={category === undefined || set.isPending}
          aria-label={strings.sharing.askFirstRowLabel(share.name)}
          onChange={(event) => {
            if (category !== undefined) set.mutate({ ...category, asksFirst: event.target.checked })
          }}
        />
        {strings.sharing.askFirstLabel}
      </label>
      {share.asksFirst ? (
        <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.askFirstNote}</p>
      ) : justOpened ? (
        <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.askFirstOffNote}</p>
      ) : null}
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </div>
  )
}

/**
 * Who a folder goes to, under its row.
 *
 * A folder nobody has picked people for reaches everyone paired and says so; a folder whose owner picked has
 * the same empty list when they picked nobody, and that one reaches nobody. Only the folder's own answer tells
 * them apart, so the row reads it rather than the length of the list.
 */
function Members({ share }: { share: ShareSummary }) {
  const members = useQuery({
    queryKey: ['shares', 'members', share.id],
    queryFn: () => fetchShareMembers(share.id),
  })
  if (members.data === undefined) return null
  const who = share.reachesEveryone
    ? strings.sharing.membersEveryone
    : members.data.length === 0
      ? strings.sharing.membersNone
      : members.data.map((member) => member.name ?? strings.sharing.unnamed).join(', ')
  return <p className="mt-1 text-xs text-[var(--color-muted)]">{who}</p>
}

/** Picking who a folder goes to. The list replaces whatever was there; nobody ticked reaches nobody. */
function ChooseMembers({ share, onClose }: { share: ShareSummary; onClose: () => void }) {
  const queryClient = useQueryClient()
  const peers = useQuery({ queryKey: ['sharing', 'peers'], queryFn: fetchPeers })
  const members = useQuery({
    queryKey: ['shares', 'members', share.id],
    queryFn: () => fetchShareMembers(share.id),
  })
  const [picked, setPicked] = useState<ReadonlySet<string> | null>(null)
  const [note, setNote] = useState<string | null>(null)
  // What is on the folder now, until somebody ticks something, so saving without a change keeps what it had:
  // everyone ticked for a folder nobody has picked people for, and what was picked for one that has — nobody
  // included.
  const current =
    picked ??
    new Set(
      members.data === undefined
        ? []
        : share.reachesEveryone
          ? (peers.data ?? []).map((peer) => peer.deviceId)
          : members.data.map((member) => member.deviceId),
    )
  const save = useMutation({
    mutationFn: () => setShareMembers(share.id, [...current]),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      void queryClient.invalidateQueries({ queryKey: ['shares'] })
      onClose()
    },
    onError: () => setNote(strings.sharing.membersFailed),
  })
  return (
    <Dialog title={strings.sharing.membersTitle(share.name)} onClose={onClose}>
      {peers.data === undefined ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
      ) : peers.data.length === 0 ? (
        <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">{strings.sharing.membersNobodyPaired}</p>
      ) : (
        <ul role="list" className="mt-3 flex list-none flex-col gap-1">
          {peers.data.map((peer) => (
            <li key={peer.deviceId}>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={current.has(peer.deviceId)}
                  onChange={(event) => {
                    const next = new Set(current)
                    if (event.target.checked) next.add(peer.deviceId)
                    else next.delete(peer.deviceId)
                    setPicked(next)
                  }}
                />
                <span className="min-w-0 shrink-0">{peer.name ?? strings.sharing.unnamed}</span>
                <span className="tabular truncate text-xs text-[var(--color-muted)]">{peer.address}</span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {current.size > 0 ? null : (
        <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.membersNone}</p>
      )}
      {note === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
          {note}
        </p>
      )}
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={onClose} autoFocus className={BUTTON}>
          {strings.sharing.shareCancel}
        </button>
        <button type="button" onClick={() => save.mutate()} disabled={save.isPending} className={BUTTON}>
          {save.isPending ? strings.sharing.membersSaving : strings.sharing.membersSave}
        </button>
      </div>
    </Dialog>
  )
}

/**
 * Who the owners of the folders here have introduced (S6).
 *
 * Shown only when there is somebody to answer about: a section saying nobody has introduced anybody is a
 * section about nothing. Each person is one answer, and the card says what accepting lets them reach, because
 * accepting pairs with a machine whose owner this person may never have met.
 */
function Introductions() {
  const queryClient = useQueryClient()
  const introductions = useQuery({
    queryKey: ['sharing', 'introductions'],
    queryFn: fetchIntroductions,
    refetchInterval: REFRESH_MS,
  })
  if (!introductions.isSuccess || introductions.data.length === 0) return null
  return (
    <div className={SECTION}>
      <h3 className={SECTION_TITLE}>{strings.sharing.introductionsTitle}</h3>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">
        {strings.sharing.introductionsNote}
      </p>
      <ul role="list" className="mt-3 flex flex-col gap-2">
        {introductions.data.map((introduction) => (
          <IntroductionRow
            key={`${introduction.shareId} ${introduction.deviceId}`}
            introduction={introduction}
            onAnswered={() =>
              queryClient.invalidateQueries({ queryKey: ['sharing'] }).then(() => undefined)
            }
          />
        ))}
      </ul>
    </div>
  )
}

function IntroductionRow({
  introduction,
  onAnswered,
}: {
  introduction: Introduction
  onAnswered: () => Promise<void>
}) {
  const [note, setNote] = useState<string | null>(null)
  const answer = useMutation({
    mutationFn: (accept: boolean) =>
      answerIntroduction(introduction.shareId, introduction.deviceId, accept),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setNote(result.message)
        return
      }
      void onAnswered()
    },
    onError: () => setNote(strings.sharing.introductionFailed),
  })
  const who = introduction.name ?? introduction.deviceId
  const introducer = introduction.introducerName ?? introduction.introducedBy
  return (
    <li className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <span className="grow">
          <span className="text-sm">
            {strings.sharing.introducedBy(who, introduction.shareName, introducer)}
          </span>
          <span className="ml-2 text-xs break-all text-[var(--color-muted)] max-xs:mt-0.5 max-xs:ml-0 max-xs:block">
            {introduction.address}
          </span>
        </span>
        <span className="flex gap-2 max-xs:w-full">
          <button
            type="button"
            onClick={() => answer.mutate(true)}
            disabled={answer.isPending}
            className={BUTTON}
          >
            {answer.isPending ? strings.sharing.introductionAnswering : strings.sharing.introductionAccept}
          </button>
          <button
            type="button"
            onClick={() => answer.mutate(false)}
            disabled={answer.isPending}
            className={`${BUTTON} text-[var(--color-muted)]`}
          >
            {strings.sharing.introductionDecline}
          </button>
        </span>
      </div>
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">
        {strings.sharing.introductionReaches(introduction.shareName)}
      </p>
      {note === null ? null : (
        <p role="alert" className="mt-1 text-xs text-[var(--color-muted)]">
          {note}
        </p>
      )}
    </li>
  )
}

/**
 * Who asked to pull a share that asks first, with this installation's answer and the way to give or change it.
 *
 * Only for an installation that has a folder that asks first. Nothing here can ever happen without one, so on
 * every other installation — which is most of them, since a folder is open unless somebody says otherwise —
 * this was a heading, a note and an empty state about something that cannot occur.
 */
function Requests() {
  const queryClient = useQueryClient()
  const requests = useQuery({
    queryKey: ['shares', 'requests'],
    queryFn: fetchShareRequests,
    refetchInterval: REFRESH_MS,
  })
  // The same query the list of own shares above already made, so this costs no request.
  const shares = useQuery({ queryKey: ['shares', 'own'], queryFn: fetchShares })
  const asksFirst = (shares.data ?? []).some((share) => share.asksFirst)
  if (!requests.isSuccess) return null
  if (!asksFirst && requests.data.length === 0) return null
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
          {/* Its own line at phone width, where it otherwise wrapped mid-sentence and orphaned a word. */}
          <span className="ml-2 text-xs text-[var(--color-muted)] max-xs:mt-0.5 max-xs:ml-0 max-xs:block">
            {standing}
          </span>
        </span>
        <span className="flex gap-2 max-xs:w-full">
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
        </span>
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

/**
 * What one person shares, each a link to the shared library — read from the mirror, so it lists while they
 * are away — or, when they share nothing, why there is nothing.
 *
 * One slot with one answer, because the two used to be written by different components and could contradict
 * each other: the taken-back line is decided by `foldersInCommon`, which does not count the owner of a folder
 * this installation holds, so the person whose folder is listed immediately below was told there was no
 * folder in common any more. What they share is read straight from the mirror here, and it wins.
 */
function TheirShares({ deviceId, foldersInCommon }: { deviceId: string; foldersInCommon: number }) {
  const shares = useQuery({
    queryKey: ['sharing', 'peers', deviceId, 'shares'],
    queryFn: () => fetchPeerShares(deviceId),
    refetchInterval: REFRESH_MS,
  })
  if (!shares.isSuccess) return null
  if (shares.data.length === 0) {
    // Nothing from them and nothing of ours reaching them: paired, and reaching each other not at all (S10).
    return foldersInCommon > 0 ? (
      <p className="mt-1 text-xs text-[var(--color-muted)]">{strings.sharing.theirSharesNone}</p>
    ) : (
      <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">
        {strings.sharing.noFoldersInCommon}
      </p>
    )
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
