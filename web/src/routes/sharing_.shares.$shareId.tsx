import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  SharedLibraryGone,
  fetchBatchStatus,
  fetchLatestPull,
  fetchLibraries,
  fetchMirroredParts,
  fetchMirroredShare,
  mirroredThumbnailUrl,
  startPull,
} from '../lib/api'
import { strings } from '../lib/strings'
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
          <h2 className="mt-4 text-xl font-medium">{library.data.name}</h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            {strings.sharing.librarySharedBy(
              library.data.sharer ?? strings.sharing.unnamedSharer,
              library.data.partCount,
            )}
          </p>
          <p className="mt-1 text-xs text-[var(--color-muted)]">
            {library.data.syncedAt === null
              ? strings.sharing.libraryNotReadYet
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
  )
}

function unfinished(pull: Pull | null | undefined): boolean {
  return pull !== null && pull !== undefined && ['queued', 'fetching', 'importing'].includes(pull.state)
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
  const library = chosen ?? libraries.data?.[0]?.id ?? null
  const pull = latest.data ?? null
  const refusal = start.data?.kind === 'refused' ? start.data.message : null
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
          disabled={library === null || start.isPending || unfinished(pull)}
          className={BUTTON}
        >
          {start.isPending ? strings.sharing.pullStarting : strings.sharing.pullAll}
        </button>
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
  return (
    <p role="status" className="text-sm">
      {text}
    </p>
  )
}

function SharedParts({ share, parts, read }: { share: PeerShareId; parts: MirroredPart[]; read: boolean }) {
  if (parts.length === 0) {
    // Not read yet is already said above; only a catalogue that has been read and holds nothing is empty.
    return read ? <p className="mt-6 text-sm text-[var(--color-muted)]">{strings.sharing.libraryEmpty}</p> : null
  }
  return (
    <ul role="list" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-3">
      {parts.map((part) => (
        <li
          key={part.sourcePath}
          className="flex flex-col rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2"
        >
          <div className="aspect-square overflow-hidden rounded bg-[var(--color-raised)]">
            {part.thumbnail ? (
              <img
                src={mirroredThumbnailUrl(share, part.sourcePath)}
                alt={strings.parts.thumbnailAlt(part.name)}
                loading="lazy"
                className="size-full object-contain"
              />
            ) : null}
          </div>
          <p className="mt-2 text-sm">{part.name}</p>
          <p className="text-xs break-all text-[var(--color-muted)]">{part.sourcePath}</p>
          <p className="mt-1 text-xs">
            {part.licences.length === 0
              ? strings.sharing.noLicence
              : strings.sharing.licences(part.licences.join(', '))}
          </p>
          <p className="text-xs text-[var(--color-muted)]">{strings.sharing.partKind(part.format, part.sizeBytes)}</p>
        </li>
      ))}
    </ul>
  )
}
