import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import {
  DEFAULT_LIBRARY_ID,
  downloadUrl,
  fetchBatchStatus,
  fetchHealth,
  fetchLibrarySettings,
  fetchLibraryStorage,
  fetchParts,
  renderLibraryThumbnails,
  renderPartThumbnail,
  setAutoThumbnail,
  startScan,
} from '../lib/api'
import { strings } from '../lib/strings'
import { filesFromDrop, filesFromInput, uploadFiles } from '../lib/upload'
import type { PickedFile, UploadProgress } from '../lib/upload'
import type {
  BatchId,
  BatchStatus,
  LibraryStorage,
  PartCard,
  PartId,
  PartsPage,
  ScanAccepted,
} from '../lib/types'

export const Route = createFileRoute('/')({
  component: RouteComponent,
  /**
   * `?batch=<id>` — the batch a scan returned, so this page can watch it drain.
   *
   * The page can start a scan of its own now (`POST /api/libraries/{id}/scan` on
   * `Role::Api`, which enqueues the walk rather than performing it), so this parameter is
   * no longer the only way in. It stays for the scan someone starts against the worker's
   * own `:8081` with `curl`, and for a link to a scan already running — a batch id is a
   * URL a person can send someone.
   *
   * Without `validateSearch` the search params are not typed at all and `useSearch()`
   * hands back nothing, so the poll below would simply never enable — silently, and
   * identically to there being no scan.
   */
  validateSearch: (search: Record<string, unknown>): { batch?: string } => {
    const batch = search.batch
    return typeof batch === 'string' && batch.length > 0 ? { batch } : {}
  },
})

/**
 * Reads the search param and hands it to `Index` as a prop. `Index` takes the batch
 * rather than calling `useSearch` itself so it stays renderable without a router — which
 * is how `index.test.tsx` renders it.
 */
function RouteComponent() {
  const { batch } = Route.useSearch()
  return <Index batch={batch} />
}

/**
 * Jobs the worker is finished with, however it finished with them.
 *
 * Subtraction, not a sum of the outcome counters. Summing them was a standing bug the
 * moment a new outcome appeared: `rendered` was once missing from the sum, so a thumbnail
 * sweep — every job of which settles as `rendered` — returned 0 for the whole batch, the
 * invalidation below never fired, and the grid stayed blank while every job succeeded.
 * `scanned` would be the same bug a second time. What is actually being asked is "how
 * much is no longer in flight", and `total - pending - running` answers exactly that for
 * every outcome there will ever be, including the ones `BatchStatus` does not break out.
 *
 * Named for jobs rather than files since a `derive` job is a revision and a
 * `scan_directory` job is a directory.
 */
function jobsSettled(status: BatchStatus): number {
  return status.total - status.pending - status.running
}

/**
 * Which copy the progress line uses. `BatchStatus` carries counters, not job kinds, so
 * this is read from two things instead: a batch this page started is whichever kind the
 * click that started it was — the state below carries that alongside the id, and it has
 * to, now that this page can start both a scan and a render. Failing that, a batch that
 * has rendered something is a render, which covers a sweep started with `curl` and opened
 * as `/?batch=<id>`: it has no trigger to be read from and would otherwise report "Scan
 * complete — 0 added." over a batch of successful renders.
 *
 * What neither reads is a batch mixing both kinds. Nothing enqueues one — `enqueue` is
 * called once per payload kind, and a scan's own children are all `ingest_file` — and
 * telling them apart properly means putting the job kind on `BatchStatus`, which is a
 * backend change.
 */
type BatchKind = 'scan' | 'render'

function progressText(status: BatchStatus, kind: BatchKind): string {
  if (status.finishedAt === null) {
    const settled = jobsSettled(status)
    if (kind === 'render') {
      return strings.render.running(settled, status.total)
    }
    // `total` counts jobs and the walk is one of them, so both halves are shifted by the
    // number of walks that have finished. While that is still 0 the batch holds nothing
    // but the walk, and there is no file count to report yet — the worker is still
    // reading the directory.
    if (status.scanned === 0) {
      return strings.scan.walking
    }
    return strings.scan.running(settled - status.scanned, status.total - status.scanned)
  }
  return kind === 'render'
    ? strings.render.finished(status.rendered)
    : strings.scan.finished(status.ingested, status.skipped)
}

export function Index({ batch }: { batch?: string }) {
  const queryClient = useQueryClient()

  /**
   * The batch this page started, if it started one, and which button started it. A
   * trigger route answers `202` with a `batchId`, and watching it is the same poll
   * whatever was triggered — the whole reason every trigger route returns `ScanAccepted`
   * rather than a shape of its own. The kind rides along because the counters cannot
   * carry it: a scan of 150 new files and a preview sweep are told apart by what was
   * clicked, not by anything in `BatchStatus`.
   */
  const [started, setStarted] = useState<{ id: BatchId; kind: BatchKind } | undefined>(undefined)
  const activeBatch = started?.id ?? batch

  const health = useQuery({ queryKey: ['health'], queryFn: fetchHealth })
  /**
   * The grid, page by page. `useInfiniteQuery` over the cursor the server already
   * returns: `PartsPage.next` is a `PartId` or `null`, which is exactly
   * `getNextPageParam`'s contract, so nothing on the server changed to make this work.
   *
   * This is the half of the roadmap's exit criterion that was failing. `fetchParts` asked
   * for one page and never asked for another, so a library of 1,000 parts showed 50 and
   * the other 950 were unreachable from the UI — "every part appears" failed for a reason
   * no amount of virtualization addresses.
   */
  const parts = useInfiniteQuery({
    queryKey: ['parts', DEFAULT_LIBRARY_ID],
    queryFn: ({ pageParam }) => fetchParts(DEFAULT_LIBRARY_ID, pageParam),
    initialPageParam: undefined as PartId | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
  // Flattened once per render rather than at each use: three things read it (the grid,
  // the extent line and the empty state) and they must agree about how many parts there
  // are.
  const loaded = parts.data?.pages.flatMap((page) => page.parts) ?? []
  const scan = useQuery({
    queryKey: ['batch', DEFAULT_LIBRARY_ID, activeBatch],
    queryFn: () => fetchBatchStatus(DEFAULT_LIBRARY_ID, activeBatch as string),
    enabled: activeBatch !== undefined,
    // The poll stops itself. A batch that finishes while the tab is backgrounded must not
    // leave a closed laptop asking about a completed scan forever — spec §11's last risk,
    // which is easy to forget and so has its own test.
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })

  const kind: BatchKind = started?.kind ?? ((scan.data?.rendered ?? 0) > 0 ? 'render' : 'scan')

  /**
   * What this library is actually set to. Its own query rather than a field on the grid's
   * page, because the two answer different questions and a settings read must not be
   * invalidated every time the worker commits a part.
   */
  const librarySettings = useQuery({
    queryKey: ['library', DEFAULT_LIBRARY_ID],
    queryFn: () => fetchLibrarySettings(DEFAULT_LIBRARY_ID),
  })
  /**
   * What this library occupies. Its own query for the reason the settings read is one:
   * it answers a different question from the grid's page, and it answers it about the
   * whole library rather than about the 50 parts a page holds.
   */
  const storage = useQuery({
    queryKey: ['storage', DEFAULT_LIBRARY_ID],
    queryFn: () => fetchLibraryStorage(DEFAULT_LIBRARY_ID),
  })
  const settings = useMutation({
    mutationFn: (on: boolean) => setAutoThumbnail(DEFAULT_LIBRARY_ID, on),
  })
  /**
   * `queued: 0` is a success with nothing to watch — such a batch has no status resource
   * — so the poll is armed only when there is work. Neither mutation invalidates
   * `['parts']`: the batch drains through the poll above and the effect below is the one
   * path to a refetch. A second path here would make that effect untestable, since the
   * grid would still refill with `rendered` missing from `jobsSettled`.
   */
  const watch = (accepted: ScanAccepted, kind: BatchKind) => {
    if (accepted.queued > 0) {
      setStarted({ id: accepted.batchId, kind })
    }
  }
  // Always `queued: 1` — the directory walk — so this always arms the poll. The file
  // count arrives as `total` grows, which is why nothing here waits for it.
  const scanNow = useMutation({
    mutationFn: () => startScan(DEFAULT_LIBRARY_ID),
    onSuccess: (accepted) => watch(accepted, 'scan'),
  })
  const sweep = useMutation({
    mutationFn: () => renderLibraryThumbnails(DEFAULT_LIBRARY_ID),
    onSuccess: (accepted) => watch(accepted, 'render'),
  })
  const renderPart = useMutation({
    mutationFn: (part: PartId) => renderPartThumbnail(part),
    onSuccess: (accepted) => watch(accepted, 'render'),
  })
  /**
   * The upload's own progress, which the batch poll cannot carry: the batch does not
   * exist until the transfer is finished and committed, so everything before that — the
   * hashing, the probe, the bytes on the wire — has no server-side resource to read. Once
   * the commit lands, `watch` hands it to the same poll a scan uses and this goes quiet.
   */
  const [uploading, setUploading] = useState<UploadProgress | undefined>(undefined)
  const [uploadNote, setUploadNote] = useState<string | null>(null)
  const upload = useMutation({
    mutationFn: (picked: PickedFile[]) =>
      uploadFiles(DEFAULT_LIBRARY_ID, picked, setUploading),
    onSuccess: ({ accepted, alreadyHere, bytesSkipped }) => {
      setUploading(undefined)
      // `queued: 0` means the probe found every file already indexed here, which is a
      // success with no batch to watch — and reads as one rather than as silence.
      const saved = strings.upload.saved(alreadyHere, bytesSkipped)
      setUploadNote(
        accepted.queued === 0 ? strings.upload.nothingToDo : saved === '' ? null : saved,
      )
      watch(accepted, 'scan')
    },
    onError: () => {
      setUploading(undefined)
      setUploadNote(strings.upload.failed)
    },
  })
  const startUpload = (picked: PickedFile[]) => {
    setUploadNote(picked.length === 0 ? strings.upload.empty : null)
    if (picked.length > 0) {
      upload.mutate(picked)
    }
  }

  // The grid is a separate query with its own cache, and nothing else would tell it the
  // library changed underneath it while the worker commits parts. Keyed on jobs settled
  // rather than on the poll tick, so a second in which nothing finished costs no refetch.
  const settled = scan.data === undefined ? 0 : jobsSettled(scan.data)
  const pagesLoaded = parts.data?.pages.length ?? 0
  const batchFinished = scan.data?.finishedAt != null
  useEffect(() => {
    if (settled === 0) {
      return
    }
    // Live-fill costs one request while the grid is on its first page, and one request
    // *per loaded page* after that: `invalidateQueries` on an infinite query refetches
    // every page it holds. Measured in Chrome against the 1,000-part bench — a grid
    // eleven pages deep pulled eleven pages on every settle tick, which during a real
    // scan is roughly 4.7 MB a second.
    //
    // And it bought nothing. Each page keeps its own cursor, so pages two and beyond
    // re-fetch rows that cannot have changed: parts arrive newest-first, at the top,
    // ahead of every cursor already held. Only the first page can gain anything.
    //
    // So a deep-scrolled grid waits for the batch instead. Trimming to the first page
    // would be the other way to make it cheap and it snaps a reading user back to the
    // top of a library they were scrolled into, which is worse than a grid that fills a
    // few seconds later.
    if (pagesLoaded <= 1 || batchFinished) {
      void queryClient.invalidateQueries({ queryKey: ['parts', DEFAULT_LIBRARY_ID] })
    }
    // The totals move with the grid, and nothing else would tell them so. A scan that
    // ingests 151 parts under a line still reporting the pre-scan figure is a
    // measurement contradicted by the cards directly above it. One row either way, so it
    // is not worth gating.
    void queryClient.invalidateQueries({ queryKey: ['storage', DEFAULT_LIBRARY_ID] })
  }, [settled, pagesLoaded, batchFinished, queryClient])

  const note = scanNow.isError
    ? strings.scan.startFailed
    : sweep.isError || renderPart.isError
      ? strings.render.queueFailed
      : sweep.data?.queued === 0
        ? strings.render.nothingMissing
        : null

  return (
    <section>
      <ActionBar
        // Three sources, most authoritative first, and `undefined` when none of them has
        // an answer. The server's echo is the truth once it lands; `variables` is what this
        // click asked for and covers the round trip, since react-query clears `data` the
        // moment a mutation goes pending — without it the box springs back to its old
        // position and sits there, disabled, for as long as the request takes, which reads
        // as the click having been ignored. A click the server refused is dropped, because
        // a value it rejected is not a position this library is in and there is now
        // something true to fall back to: the `GET`, which is the starting position and
        // the reason design §3.2's default is not. The default is what a library is set to
        // until someone changes it, not what this one is set to.
        autoThumbnail={
          settings.data?.autoThumbnail ??
          (settings.isError ? undefined : settings.variables) ??
          librarySettings.data?.autoThumbnail
        }
        onAutoThumbnail={(on) => settings.mutate(on)}
        settingsBusy={settings.isPending}
        settingsNote={
          settings.isError
            ? strings.library.autoThumbnailFailed
            : librarySettings.isError
              ? strings.library.autoThumbnailUnknown
              : null
        }
        onScan={() => scanNow.mutate()}
        scanBusy={scanNow.isPending}
        onSweep={() => sweep.mutate()}
        sweepBusy={sweep.isPending}
        note={note}
      />
      <DropTarget onFiles={startUpload} busy={upload.isPending} progress={uploading} />
      {uploadNote === null ? null : (
        <p className="mb-4 text-sm text-[var(--color-muted)]">{uploadNote}</p>
      )}
      {activeBatch === undefined ? null : (
        <ScanProgress status={scan.data} isError={scan.isError} kind={kind} />
      )}
      {parts.isPending ? (
        <p className="text-[var(--color-muted)]">{strings.parts.loading}</p>
      ) : parts.isError ? (
        <p className="max-w-prose text-[var(--color-muted)]">{strings.parts.failed}</p>
      ) : loaded.length === 0 ? (
        // An empty page and a page still in flight are different facts, so only a page
        // that came back empty gets the empty state.
        <EmptyLibrary />
      ) : (
        <>
          <Grid
            parts={loaded}
            onRender={(part) => renderPart.mutate(part)}
            busyPart={renderPart.isPending ? renderPart.variables : undefined}
          />
          <MorePages
            count={loaded.length}
            hasMore={parts.hasNextPage}
            fetching={parts.isFetchingNextPage}
            onMore={() => void parts.fetchNextPage()}
          />
          <StorageTotals storage={storage.data} isError={storage.isError} />
        </>
      )}
      <p className="mt-6 text-sm text-[var(--color-muted)]">
        {health.isPending
          ? strings.health.checking
          : health.isError
            ? strings.health.failed
            : strings.health.ok(health.data.database.major)}
      </p>
    </section>
  )
}

/**
 * Where a folder goes in.
 *
 * Both gestures, because they are not interchangeable. `<input webkitdirectory>` opens
 * the picker and reports `webkitRelativePath` for free; a *drag* of a folder gives
 * neither — `DataTransfer.files` holds the folder as a zero-byte entry with none of its
 * contents — so the drop path goes through `webkitGetAsEntry` and a recursive walk. See
 * `filesFromDrop`, which is also where the `readEntries` 100-entry limit is handled.
 *
 * `dragover` must call `preventDefault` on every event or the browser navigates to the
 * dropped file instead of handing it here, which is the default and looks exactly like a
 * broken page. The counter, rather than a boolean: `dragenter`/`dragleave` fire for every
 * child element the pointer crosses, so a boolean flickers off the moment the drag passes
 * over the label inside the target.
 */
function DropTarget({
  onFiles,
  busy,
  progress,
}: {
  onFiles: (picked: PickedFile[]) => void
  busy: boolean
  progress: UploadProgress | undefined
}) {
  const [depth, setDepth] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const over = depth > 0

  return (
    <div
      onDragEnter={(event) => {
        event.preventDefault()
        setDepth((d) => d + 1)
      }}
      onDragOver={(event) => event.preventDefault()}
      onDragLeave={() => setDepth((d) => Math.max(0, d - 1))}
      onDrop={(event) => {
        event.preventDefault()
        setDepth(0)
        void filesFromDrop(event.dataTransfer.items).then(onFiles)
      }}
      className={`ease-mechanical mb-6 rounded border border-dashed p-6 text-center text-sm duration-[var(--duration-fast)] ${
        over
          ? 'border-[var(--color-accent)] bg-[var(--color-surface)]'
          : 'border-[var(--color-border)]'
      }`}
    >
      {busy && progress !== undefined ? (
        <span className="text-[var(--color-muted)]">{progressLine(progress)}</span>
      ) : (
        <span className="text-[var(--color-muted)]">
          {over ? (
            strings.upload.dropNow
          ) : (
            <>
              {strings.upload.dropHere}{' '}
              <button
                type="button"
                onClick={() => input.current?.click()}
                className="underline underline-offset-2 hover:text-[var(--color-fg)]"
              >
                {strings.upload.choose}
              </button>
            </>
          )}
        </span>
      )}
      {/*
        `webkitdirectory` is not in React's typed attribute set — it is a non-standard
        attribute every browser that matters implements — so it is spelled lowercase as a
        DOM attribute rather than camelCased. Hidden rather than styled: the button above
        is the control, and a bare file input cannot be made to look like anything.
      */}
      <input
        ref={input}
        type="file"
        multiple
        {...{ webkitdirectory: '' }}
        hidden
        onChange={(event) => {
          if (event.target.files !== null) {
            onFiles(filesFromInput(event.target.files))
          }
          // So dropping the same folder twice in a row fires a second change event.
          event.target.value = ''
        }}
      />
    </div>
  )
}

/** One line for whichever phase the upload is in. Each phase has a different number
 *  worth showing, which is why this is a switch and not one string with holes in it. */
function progressLine(progress: UploadProgress): string {
  switch (progress.phase) {
    case 'hashing':
      return strings.upload.hashing(progress.filesDone, progress.filesTotal)
    case 'probing':
      return strings.upload.probing
    case 'transferring':
      return strings.upload.transferring(
        progress.bytesSent,
        progress.bytesToSend,
        progress.filesDone,
        progress.filesTotal,
      )
    case 'committing':
      return strings.upload.committing
  }
}

/**
 * What this page can do to a library: scan the server's ingest folder into it, change
 * whether ingest renders previews, and render the previews it does not have.
 *
 * Every action enqueues rather than does — the walk and the rendering both happen in the
 * worker — so no button here waits on a filesystem or on geometry. The toggle is the one
 * control that has a state of its own to be wrong about, which is why it takes
 * `boolean | undefined` and not a default. `note` carries whatever the last action has to
 * say: a sweep that found nothing missing is a success and says so, which is the one
 * place this reading is easy to get backwards.
 */
function ActionBar({
  autoThumbnail,
  onAutoThumbnail,
  settingsBusy,
  settingsNote,
  onScan,
  scanBusy,
  onSweep,
  sweepBusy,
  note,
}: {
  autoThumbnail: boolean | undefined
  onAutoThumbnail: (on: boolean) => void
  settingsBusy: boolean
  settingsNote: string | null
  onScan: () => void
  scanBusy: boolean
  onSweep: () => void
  sweepBusy: boolean
  note: string | null
}) {
  return (
    <div className="mb-6 flex flex-wrap items-center gap-x-6 gap-y-2 border-b border-[var(--color-border)] pb-4">
      <label className="flex items-center gap-2 text-sm" title={strings.library.autoThumbnailDetail}>
        {/*
          `undefined` is "not known yet", and the checkbox says so in the way a checkbox
          says it: mixed, and not clickable until there is a state to click away from.
          Painting a confident "on" for the tick before the read lands is the same lie in a
          shorter window, and a box that flips under the cursor is worse than one that
          waits. It stays mixed if the read fails outright — `settingsNote` says why and
          says to reload — because there is nothing honest to put there.

          `indeterminate` is a DOM property with no attribute, so it is set through the ref
          rather than rendered. Block body: a React 19 ref callback that returns a value is
          read as a cleanup function.
        */}
        <input
          type="checkbox"
          checked={autoThumbnail ?? false}
          ref={(el) => {
            if (el !== null) {
              el.indeterminate = autoThumbnail === undefined
            }
          }}
          disabled={settingsBusy || autoThumbnail === undefined}
          onChange={(event) => onAutoThumbnail(event.target.checked)}
          className="accent-[var(--color-accent)]"
        />
        {strings.library.autoThumbnail}
      </label>
      <button
        type="button"
        onClick={onScan}
        disabled={scanBusy}
        className="ease-mechanical rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.scan.start}
      </button>
      <button
        type="button"
        onClick={onSweep}
        disabled={sweepBusy}
        className="ease-mechanical rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.render.sweep}
      </button>
      {settingsNote === null ? null : (
        <span className="text-sm text-[var(--color-muted)]">{settingsNote}</span>
      )}
      {note === null ? null : <span className="text-sm text-[var(--color-muted)]">{note}</span>}
    </div>
  )
}

/**
 * The batch line: how far a batch has got, how it ended, and what went wrong.
 *
 * The reasons, not only the count. `batch_status` has returned a `failures` list carrying
 * each job's `last_error` since slice 2 and nothing displayed it, which mattered more
 * once the directory walk moved into a job: an unreadable `/ingest` mount used to fail
 * the request an operator was watching, and now fails a job in this batch. If the reason
 * does not reach the screen, that move only relocated a terminal round-trip somewhere
 * less obvious. See `crates/lapidary-ingest/src/scan.rs`'s module doc.
 *
 * Nothing renders while the first poll is in flight. A batch whose status has not
 * arrived yet is not a fact about the library, and the grid below is the page — a
 * placeholder here would push it down for one tick and then move it back.
 */
function ScanProgress({
  status,
  isError,
  kind,
}: {
  status?: BatchStatus
  isError: boolean
  kind: BatchKind
}) {
  // Picked once, out here: a `kind === 'render' ? … : …` inside JSX puts the discriminator
  // itself in a child expression, where `no-bare-strings.test.ts` reads it — correctly —
  // as a bare literal reaching the screen.
  const copy = kind === 'render' ? strings.render : strings.scan
  if (isError) {
    return <p className="mb-4 max-w-prose text-[var(--color-muted)]">{copy.unknown}</p>
  }
  if (status === undefined) {
    return null
  }
  // The server caps the list at 100 while `failedTotal` is the real number, so a batch
  // with more failures than that says so rather than trailing off at the hundredth.
  const hidden = status.failedTotal - status.failed.length
  return (
    <div className="mb-4 max-w-prose text-[var(--color-muted)]">
      <p className="flex flex-wrap gap-2">
        <span>{progressText(status, kind)}</span>
        {status.failedTotal === 0 ? null : <span>{copy.failed(status.failedTotal)}</span>}
      </p>
      {status.failed.length === 0 ? null : (
        <ul className="mt-2 space-y-1 text-sm">
          {status.failed.map((failure) => (
            // The path is not unique — two jobs can name the same file across retries,
            // and a `scan_directory` failure has no path at all — so the key is the pair
            // that identifies the row on screen.
            <li key={`${failure.path}\u0000${failure.reason}`}>
              {strings.failure.line(failure.path, failure.reason)}
            </li>
          ))}
          {hidden <= 0 ? null : <li>{strings.failure.more(hidden)}</li>}
        </ul>
      )}
    </div>
  )
}

function EmptyLibrary() {
  return (
    <div className="max-w-prose">
      <h2 className="text-lg">{strings.emptyLibrary.title}</h2>
      <p className="mt-2 text-[var(--color-muted)]">{strings.emptyLibrary.body}</p>
    </div>
  )
}

/**
 * How much of the library is on screen, and the sentinel that fetches the rest.
 *
 * A grid is a scrolling surface, so the gesture that means "show me more" is scrolling to
 * the end of it. An `IntersectionObserver` on a sentinel after the last card is fifteen
 * lines and no dependency; a "load more" button would make a user click ten times to see
 * a library they can already scroll through.
 *
 * The button is still rendered, and not as a fallback nobody sees. It is what a keyboard
 * user reaches, and what works when `IntersectionObserver` never fires because the grid is
 * short enough that the sentinel is already on screen and never crosses the boundary
 * again. Both paths call the same thing.
 *
 * `hasMore` is the server's own answer, not a length comparison against a limit this
 * component would have to know: a full page hands back a cursor and a short one hands back
 * null.
 */
function MorePages({
  count,
  hasMore,
  fetching,
  onMore,
}: {
  count: number
  hasMore: boolean
  fetching: boolean
  onMore: () => void
}) {
  const sentinel = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const node = sentinel.current
    if (node === null || !hasMore) {
      return
    }
    // `fetching` is deliberately not in the dependency list. Re-creating the observer on
    // every fetch would disconnect and reconnect it mid-scroll, and an observer that
    // reconnects while its target is already visible fires immediately — which is a
    // second request for the page still in flight. The guard is inside the callback
    // instead, where it reads the current value.
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        onMore()
      }
    })
    observer.observe(node)
    return () => observer.disconnect()
  }, [hasMore, onMore])

  return (
    <>
      <div ref={sentinel} aria-hidden className="h-px" />
      <p className="mt-4 max-w-prose text-xs text-[var(--color-muted)]">
        {hasMore ? (
          <>
            {strings.parts.showingSoFar(count)}{' '}
            <button
              type="button"
              onClick={onMore}
              disabled={fetching}
              className="underline underline-offset-2 disabled:opacity-50"
            >
              {fetching ? strings.parts.loadingMore : strings.parts.loadMore}
            </button>
          </>
        ) : (
          strings.parts.showingAll(count)
        )}
      </p>
    </>
  )
}

/**
 * What the whole library occupies, under the page that shows part of it.
 *
 * Rendered only where the grid has cards: a line of zeroes over an empty library says
 * nothing the empty state has not already said better. Both totals are bytes on disk
 * after compression and deduplicated — `PgParts::storage_totals` is where that
 * accounting is written down — and the ratio arrives computed rather than divided here,
 * so a second reader cannot report the same library the other way up.
 */
function StorageTotals({ storage, isError }: { storage?: LibraryStorage; isError: boolean }) {
  if (isError) {
    return <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">{strings.storage.failed}</p>
  }
  // Nothing at all while the first read is in flight: a total is a claim about the
  // library, and there is no honest placeholder for a claim.
  if (storage === undefined) {
    return null
  }
  return (
    <p className="mt-2 max-w-prose text-xs text-[var(--color-muted)]">
      {strings.storage.totals(storage.sourceBytes, storage.derivativeBytes, storage.derivativeRatio)}
    </p>
  )
}

function Grid({
  parts,
  onRender,
  busyPart,
}: {
  parts: readonly PartCard[]
  onRender: (part: PartId) => void
  busyPart?: PartId
}) {
  return (
    <ul className="grid list-none grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4">
      {parts.map((part) => (
        // `content-visibility: auto` is the virtualization, and it is one CSS property
        // rather than a dependency. It tells the browser to skip layout, paint and image
        // decode for a card that is off screen, which is what a virtualizer buys — while
        // this grid stays a plain `repeat(auto-fill, …)` CSS grid, whose column count
        // changes with the viewport and which a virtualizer would therefore have to
        // measure and re-measure to know a row height it currently never needs.
        //
        // `contain-intrinsic-size` is not optional beside it. Without a placeholder size
        // a skipped card measures zero, so the page height collapses and the scrollbar
        // jumps as cards enter and leave — which is what makes `content-visibility` look
        // broken.
        //
        // 26rem is 416px, which is a rendered card measured in Chrome (415px, uniform
        // across 1,000 of them) and not arithmetic — the first guess was 20rem from a
        // card measured before its thumbnail had loaded, and it under-reported the page
        // height by 23%. The leading `auto` means the browser substitutes each card's
        // real size once it has rendered one, so this figure only has to be close for the
        // first paint rather than exact forever.
        <li key={part.id} className="[content-visibility:auto] [contain-intrinsic-size:auto_26rem]">
          <Card part={part} onRender={onRender} busy={part.id === busyPart} />
        </li>
      ))}
    </ul>
  )
}

function Card({
  part,
  onRender,
  busy,
}: {
  part: PartCard
  onRender: (part: PartId) => void
  busy: boolean
}) {
  const nameId = `part-name-${part.id}`
  return (
    <article
      aria-labelledby={nameId}
      className="ease-mechanical flex h-full flex-col overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-0.5"
    >
      <div className="flex aspect-square items-center justify-center bg-[var(--color-bg)]">
        {part.thumbnail === null ? (
          // Never an <img> with an empty src: a broken-image glyph reads as a failure,
          // and "the worker has not rasterized this yet" is not one.
          <span className="text-xs text-[var(--color-muted)]">{strings.parts.noThumbnail}</span>
        ) : (
          <img
            src={part.thumbnail}
            alt={strings.parts.thumbnailAlt(part.name)}
            className="h-full w-full object-contain"
          />
        )}
      </div>
      <div className="flex flex-1 flex-col gap-1 p-3">
        {/*
          The name is the link, not the whole card. A card holds a render button and a
          download link already, and nesting those inside an anchor is invalid HTML that
          browsers resolve by guessing. The name is also what a keyboard user tabs to and
          what a screen reader announces for the card, so it is the right target.
        */}
        <h2 id={nameId} className="text-sm leading-snug">
          <Link
            to="/parts/$partId"
            params={{ partId: part.id }}
            className="ease-mechanical duration-[var(--duration-fast)] hover:underline hover:underline-offset-2"
          >
            {part.name}
          </Link>
        </h2>
        {part.partNumber === null ? null : (
          <p className="font-mono text-xs text-[var(--color-muted)]">{part.partNumber}</p>
        )}
        <Measurements part={part} />
        <SourceFile part={part} />
        {/*
          Every card carries it, not only the ones showing "No preview yet": re-rendering
          a stale preview is the same request, and a control that appears and disappears
          as the sweep lands is harder to hit than one that stays put. The accessible name
          says which part, since the visible label is identical on every card.
        */}
        <button
          type="button"
          onClick={() => onRender(part.id)}
          disabled={busy}
          aria-label={strings.render.partFor(part.name)}
          className="ease-mechanical mt-2 self-start rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {strings.render.part}
        </button>
      </div>
    </article>
  )
}

/**
 * The download control, the hash to check what arrives against, and what the file costs
 * on disk.
 *
 * A plain `<a href download>`, never a fetch. The browser is what reads
 * `Content-Disposition`, and the route works to get the RFC 5987 `filename*` right so
 * that a Turkish part name survives the save dialog; pulling the bytes through `fetch`
 * into a blob URL would discard that header and name every download after the revision
 * id. It also costs no JavaScript, no request until it is clicked, and nothing at all
 * when it is middle-clicked into a background tab.
 *
 * The four source fields are absent together (`PartCard.sourceHash`), so a revision with
 * no source row renders a sentence in place of the whole line rather than a link that
 * would 404. Deliberately not a disabled-looking link either: clicking it again would
 * not help, and a control that cannot work must not look like one that can.
 */
function SourceFile({ part }: { part: PartCard }) {
  // Narrowed with typeof for the reason `Measurements` narrows: the response is cast
  // rather than validated, so a field the server stops sending arrives here as undefined
  // and would reach a formatter as one.
  const hash = typeof part.sourceHash === 'string' ? part.sourceHash : null
  const stored = typeof part.storedBytes === 'number' ? part.storedBytes : null
  const ingested = typeof part.sourceBytes === 'number' ? part.sourceBytes : null
  if (hash === null || stored === null) {
    return <p className="mt-2 text-xs text-[var(--color-muted)]">{strings.download.noSource}</p>
  }
  return (
    <p className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--color-muted)]">
      <a
        href={downloadUrl(part.revision)}
        download
        aria-label={strings.download.originalFor(part.name)}
        className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.download.original}
      </a>
      {/* The head of the digest on screen, the whole of it on the title — DATA.md §5.1. */}
      <span className="font-mono" title={hash}>
        {strings.parts.shortHash(hash)}
      </span>
      {/*
        `compressed` is `boolean | null`, and null means "no source file", never "unknown
        compression" — a card that got this far has a source row and knows which of the
        two it is. Three branches rather than two, because the third combination has no
        honest sentence: a compressed part whose ingested size did not arrive cannot be
        called uncompressed, which is what a fallback to `storedRaw` would say. That is
        not the claim that says less, it is the claim that is wrong.
      */}
      <span>
        {part.compressed !== true
          ? strings.parts.storedRaw(stored)
          : ingested !== null
            ? strings.parts.storedCompressed(stored, ingested)
            : strings.parts.storedSize(stored)}
      </span>
    </p>
  )
}

/**
 * The card's measurement line, rendered as one indivisible unit.
 *
 * A triangle count is tessellation-derived by construction, so a card showing one is
 * showing a mesh-derived figure whatever the wire's `approximate` says. CLAUDE.md
 * forbids such a figure appearing unlabelled, so the label is not a sibling conditional
 * that the count can drift away from: either the whole line renders or none of it does,
 * and within it the badge is unconditional. No branch here can emit a count without a
 * label, which is the difference between the rule holding and the rule happening to
 * hold because the ingest path currently sets the flag to a constant.
 *
 * The line still renders for a part with no count but the flag set, because the flag
 * means *any* figure on this part is mesh-derived — not that this count is.
 */
function Measurements({ part }: { part: PartCard }) {
  // Narrowed with typeof rather than compared to null: the binding says `number | null`,
  // but the response is cast rather than validated, so a field that disappears upstream
  // arrives here as undefined and would reach .toLocaleString() as one.
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  if (!part.approximate && count === null) {
    return null
  }
  return (
    <p className="mt-auto flex flex-wrap items-center gap-2 pt-2 text-xs text-[var(--color-muted)]">
      {count === null ? null : <span>{strings.parts.triangles(count)}</span>}
      <span
        title={strings.parts.approximateDetail}
        className="rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[0.65rem] tracking-wider uppercase"
      >
        {strings.parts.approximate}
      </span>
    </p>
  )
}
