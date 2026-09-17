import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  SharedLibraryGone,
  controlPull,
  fetchBatchStatus,
  fetchLatestPull,
  fetchLibraries,
  fetchMirroredParts,
  fetchMirroredShare,
  mirroredThumbnailUrl,
  startPull,
} from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import { breakable } from '../components/Card'
import type { BatchId, LibraryId, MirroredPart, PeerShareId, Pull } from '../lib/types'

const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * How often the page asks again. The hello round re-reads a catalogue only when its digest moves, so this costs a
 * database read and never a request to the other machine.
 */
const REFRESH_MS = 15_000

/** How often an unfinished pull is asked about. A database read; the peer role does the fetching. */
const PULL_POLL_MS = 2_000

/**
 * A shared library: a category somebody paired with this installation offers, read from the mirror the peer role
 * keeps, so it browses while their machine is asleep.
 *
 * A person decides by two things here — the licence and the look — so every card carries both, and a part with no
 * licence recorded says so rather than showing nothing. The mirror is a copy of their list, not this installation's
 * data: a part they stop offering leaves this page, and the page says that is what happened.
 */
export const Route = createFileRoute('/sharing_/shares/$shareId')({ component: RouteComponent })

function RouteComponent() {
  const { shareId } = Route.useParams()
  return <SharedLibraryPage share={shareId as PeerShareId} />
}

export function SharedLibraryPage({ share }: { share: PeerShareId }) {
  const library = useQuery({
    queryKey: ['sharing', 'shares', share],
    queryFn: () => fetchMirroredShare(share),
    refetchInterval: REFRESH_MS,
  })
  const parts = useInfiniteQuery({
    queryKey: ['sharing', 'shares', share, 'parts'],
    queryFn: ({ pageParam }) => fetchMirroredParts(share, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next ?? undefined,
    enabled: library.isSuccess,
  })
  const gone = library.error instanceof SharedLibraryGone || parts.error instanceof SharedLibraryGone
  return (
    <AppFrame>
      <section>
        {library.isSuccess ? <title>{strings.sharing.libraryTitle(library.data.name)}</title> : null}
        <Link
          to="/sharing"
          className="ease-mechanical text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          {strings.sharing.backToSharing}
        </Link>
        {library.isPending ? (
          <p className="mt-6 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
        ) : gone ? (
          <p className="mt-6 max-w-prose text-sm text-[var(--color-muted)]">{strings.sharing.libraryGone}</p>
        ) : library.isError ? (
          <p role="alert" className="mt-6 text-sm text-[var(--color-muted)]">
            {strings.sharing.libraryLoadFailed}
          </p>
        ) : (
          <>
            <h2 className={`mt-4 ${HEADLINE}`}>{library.data.name}</h2>
            <p className="mt-1 text-sm text-[var(--color-muted)]">
              {strings.sharing.librarySharedBy(
                library.data.sharer ?? strings.sharing.unnamedSharer,
                library.data.partCount,
              )}
            </p>
            <p className="mt-1 text-xs text-[var(--color-muted)]">
              {library.data.syncedAt === null
                ? strings.sharing.libraryNotReadYet
                : library.data.readFrom !== null && library.data.asOf !== null
                  ? strings.sharing.libraryRelayed(
                      library.data.readFromName ?? strings.sharing.unnamedSharer,
                      library.data.asOf,
                    )
                  : strings.sharing.librarySynced(library.data.syncedAt)}
            </p>
            <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.libraryLead}</p>
            <PullPanel share={share} />
            {parts.isPending ? (
              <p className="mt-6 text-sm text-[var(--color-muted)]">{strings.sharing.loading}</p>
            ) : parts.isError && !gone ? (
              <p role="alert" className="mt-6 text-sm text-[var(--color-muted)]">
                {strings.sharing.libraryLoadFailed}
              </p>
            ) : (
              <SharedParts
                share={share}
                parts={parts.data?.pages.flatMap((page) => page.parts) ?? []}
                read={library.data.syncedAt !== null}
              />
            )}
            {parts.hasNextPage ? (
              <button
                type="button"
                onClick={() => void parts.fetchNextPage()}
                disabled={parts.isFetchingNextPage}
                className={`${BUTTON} mt-4`}
              >
                {parts.isFetchingNextPage ? strings.sharing.showingMore : strings.sharing.showMore}
              </button>
            ) : null}
          </>
        )}
      </section>
    </AppFrame>
  )
}

/** Still moving: worth asking about again. A paused pull is not, until somebody resumes it. */
function unfinished(pull: Pull | null | undefined): boolean {
  return pull !== null && pull !== undefined && ['queued', 'fetching', 'waiting', 'importing'].includes(pull.state)
}

/** Before its import, a pull can be paused; paused, it can be resumed. */
function pausable(pull: Pull | null): boolean {
  return pull !== null && ['queued', 'fetching', 'waiting'].includes(pull.state)
}

/** Pull every part into a library of this installation's, and follow the pull while the peer role works it. */
function PullPanel({ share }: { share: PeerShareId }) {
  const client = useQueryClient()
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const latest = useQuery({
    queryKey: ['sharing', 'shares', share, 'pull'],
    queryFn: () => fetchLatestPull(share),
    refetchInterval: (query) => (unfinished(query.state.data) ? PULL_POLL_MS : false),
  })
  const [chosen, setChosen] = useState<LibraryId | null>(null)
  const start = useMutation({
    mutationFn: (library: LibraryId) => startPull(share, library),
    onSuccess: () => client.invalidateQueries({ queryKey: ['sharing', 'shares', share, 'pull'] }),
  })
  const control = useMutation({
    mutationFn: ({ pull, action }: { pull: Pull; action: 'pause' | 'resume' }) => controlPull(pull.id, action),
    onSuccess: () => client.invalidateQueries({ queryKey: ['sharing', 'shares', share, 'pull'] }),
  })
  const pull = latest.data ?? null
  // The library this share was last pulled into, until somebody chooses another.
  const library = chosen ?? pull?.libraryId ?? libraries.data?.[0]?.id ?? null
  const refusal =
    start.data?.kind === 'refused'
      ? start.data.message
      : control.data?.kind === 'refused'
        ? control.data.message
        : null
  const paused = pull?.state === 'paused'
  return (
    <div className="mt-4 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-sm">
          {strings.sharing.pullInto}
          <select
            value={library ?? ''}
            onChange={(event) => setChosen(event.target.value as LibraryId)}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2 py-1 text-sm"
          >
            {(libraries.data ?? []).map((option) => (
              <option key={option.id} value={option.id}>
                {option.name}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          onClick={() => library !== null && start.mutate(library)}
          disabled={library === null || start.isPending || unfinished(pull) || paused}
          className={BUTTON}
        >
          {start.isPending ? strings.sharing.pullStarting : strings.sharing.pullAll}
        </button>
        {pull !== null && (pausable(pull) || paused) ? (
          <button
            type="button"
            onClick={() => control.mutate({ pull, action: paused ? 'resume' : 'pause' })}
            disabled={control.isPending}
            className={BUTTON}
          >
            {paused ? strings.sharing.resume : strings.sharing.pause}
          </button>
        ) : null}
      </div>
      {refusal === null ? null : (
        <p role="alert" className="text-sm">
          {refusal}
        </p>
      )}
      {pull === null ? null : <PullProgress pull={pull} />}
      <p className="max-w-prose text-xs text-[var(--color-muted)]">{strings.sharing.pullNote}</p>
    </div>
  )
}

function PullProgress({ pull }: { pull: Pull }) {
  const importing = pull.state === 'importing' && pull.batchId !== null
  const batch = useQuery({
    queryKey: ['batch', pull.batchId],
    queryFn: () => fetchBatchStatus(pull.libraryId, pull.batchId as BatchId),
    enabled: importing,
    refetchInterval: PULL_POLL_MS,
  })
  const text =
    pull.state === 'done'
      ? strings.sharing.pullDone(pull.filesTotal)
      : pull.state === 'failed'
        ? strings.sharing.pullStopped(pull.error ?? '')
        : pull.state === 'paused'
          ? strings.sharing.pullPaused
          : pull.state === 'waiting'
            ? (pull.error ?? strings.sharing.pullWaiting)
        : pull.error !== null
          ? strings.sharing.pullRetrying(pull.error)
          : pull.state === 'queued'
            ? strings.sharing.pullQueued
            : pull.state === 'fetching'
              ? strings.sharing.pullFetching(pull.filesDone, pull.filesTotal, pull.bytesDone, pull.bytesTotal)
              : strings.sharing.pullImporting(
                  batch.data === undefined ? 0 : batch.data.total - batch.data.pending - batch.data.running,
                  batch.data?.total ?? 0,
                )
  // How far along, while there is a number to say it with: bytes while fetching, jobs while importing.
  const fraction =
    pull.state === 'fetching' && pull.bytesTotal > 0
      ? pull.bytesDone / pull.bytesTotal
      : importing && batch.data !== undefined && batch.data.total > 0
        ? (batch.data.total - batch.data.pending - batch.data.running) / batch.data.total
        : null
  return (
    <div>
      <p role="status" className="tabular text-sm">
        {text}
      </p>
      {fraction === null ? null : (
        // Layout Blue, because this is live. A transform, so it moves on the stylesheet's own transition.
        <div aria-hidden="true" className="mt-2 h-1 max-w-md overflow-hidden rounded-full bg-[var(--color-border)]">
          <div
            style={{ transform: `scaleX(${Math.min(1, Math.max(0, fraction))})` }}
            className="h-full origin-left bg-[var(--color-accent)]"
          />
        </div>
      )}
    </div>
  )
}

function SharedParts({ share, parts, read }: { share: PeerShareId; parts: MirroredPart[]; read: boolean }) {
  if (parts.length === 0) {
    // Not read yet is already said above; only a catalogue that has been read and holds nothing is empty.
    return read ? <p className="mt-6 text-sm text-[var(--color-muted)]">{strings.sharing.libraryEmpty}</p> : null
  }
  return (
    <ul role="list" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 max-xs:grid-cols-2 max-xs:gap-2">
      {parts.map((part) => (
        // The grid's card, so a part looks the same whether it is yours or somebody else's.
        <li
          key={part.sourcePath}
          className="flex flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]"
        >
          <div className="flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)] p-[7%]">
            {part.thumbnail ? (
              <img
                src={mirroredThumbnailUrl(share, part.sourcePath)}
                alt={strings.parts.thumbnailAlt(part.name)}
                loading="lazy"
                className="size-full object-contain"
              />
            ) : null}
          </div>
          <div className="flex flex-1 flex-col gap-1 p-3">
            <p className="line-clamp-2 text-[13px] leading-snug font-semibold text-[var(--color-bright)]" title={part.name}>
              {breakable(part.name)}
            </p>
            <p className="tabular text-[11px] break-all text-[var(--color-muted)]">{part.sourcePath}</p>
            <p className="mt-auto pt-1 text-xs text-[var(--color-dim)]">
              {part.licences.length === 0
                ? strings.sharing.noLicence
                : strings.sharing.licences(part.licences.join(', '))}
            </p>
            <p className="tabular text-[11px] text-[var(--color-muted)]">{strings.sharing.partKind(part.format, part.sizeBytes)}</p>
          </div>
        </li>
      ))}
    </ul>
  )
}
