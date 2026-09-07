import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'
import {
  DEFAULT_LIBRARY_ID,
  batchEventsUrl,
  downloadUrl,
  fetchBatchStatus,
  fetchHealth,
  createLibrary,
  fetchInstanceStorage,
  fetchLibraries,
  fetchLibrarySettings,
  fetchLibraryStorage,
  fetchPartDetail,
  fetchParts,
  renderLibraryThumbnails,
  renderPartThumbnail,
  setAutoThumbnail,
  startScan,
} from '../lib/api'
import { Dialog } from '../components/Dialog'
import { Detail } from '../components/PartDetail'
import {
  DENSITIES,
  PAGE_SIZES,
  densityFor,
  pageSizeFor,
  setDensity,
  setPageSize,
  type Density,
  type PageSize,
} from '../lib/preferences'
import { strings } from '../lib/strings'
import { filesFromDrop, filesFromInput, uploadFiles } from '../lib/upload'
import type { PickedFile, UploadProgress } from '../lib/upload'
import {
  FolderTree,
  MovePartDialog,
  PART_DRAG_TYPE,
  partDragPayload,
  useFolders,
} from '../components/FolderTree'
import type {
  BatchId,
  BatchStatus,
  FolderId,
  InstanceStorageView,
  LibraryId,
  LibraryStorage,
  NewLibrary,
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
  validateSearch: (
    search: Record<string, unknown>,
  ): { batch?: string; folderId?: string; q?: string; library?: string } => {
    const batch = search.batch
    const folderId = search.folderId
    const q = search.q
    const library = search.library
    return {
      ...(typeof batch === 'string' && batch.length > 0 ? { batch } : {}),
      // Absent, never empty. No category selected is the whole library, which the parts
      // route spells as no parameter at all — so a selection that is cleared has to leave
      // nothing behind rather than leave `?folderId=` behind.
      ...(typeof folderId === 'string' && folderId.length > 0 ? { folderId } : {}),
      // **A number is a query too, and this is not hypothetical.** The router parses search
      // params as JSON, so it writes `?q="3310"` and reads that back as the string `3310` —
      // which works. But somebody sharing a link, or typing one, writes `?q=3310` without
      // the quotes, and *that* parses as the number 3310. A `typeof q === 'string'` check
      // alone drops it, and the page silently shows the whole library for a URL that plainly
      // asks for a search.
      //
      // `folderId` above never meets this because a UUID is not valid JSON, so it always
      // arrives as a string. Digits are.
      ...(typeof q === 'string' && q.length > 0
        ? { q }
        : typeof q === 'number'
          ? { q: String(q) }
          : {}),
      // A UUID, so never the number case `q` has to handle. Absent means the default
      // library, which is what every screen meant before there could be a second one — so
      // an old bookmark keeps working and a new one is shareable.
      ...(typeof library === 'string' && library.length > 0 ? { library } : {}),
    }
  },
})

/**
 * Reads the search params and hands them to `Index` as props. `Index` takes the batch and
 * the selected category rather than calling `useSearch` itself so it stays renderable
 * without a router — which is how `index.test.tsx` renders it.
 *
 * The selection lives in the URL for the reason every other filter does: it survives a
 * reload and it is a link a person can send someone.
 */
function RouteComponent() {
  const { batch, folderId, q, library } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Index
      batch={batch}
      folderId={folderId}
      q={q}
      // The seeded library when nobody has chosen: `DEFAULT_LIBRARY_ID` stops being the
      // answer and becomes the fallback, which is the whole of what "more than one library"
      // changes about every screen.
      library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID}
      onSelectLibrary={(next) =>
        void navigate({
          // Everything below a library belongs to it: a category id and a search from the
          // old one mean nothing in the new one, and carrying them over would filter the
          // new library by a folder it does not have.
          search: { library: next === DEFAULT_LIBRARY_ID ? undefined : next },
        })
      }
      onSelectFolder={(folder) =>
        void navigate({
          search: (previous) => ({ ...previous, folderId: folder ?? undefined }),
        })
      }
      onSearch={(query) =>
        void navigate({
          search: (previous) => ({ ...previous, q: query === '' ? undefined : query }),
          // The back button walks a person through the pages they went to, not through
          // every keystroke on the way to one.
          replace: true,
        })
      }
    />
  )
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
 * to, now that this page can start a scan, a render or a migration. Failing that, a
 * batch is read by what it CONTAINS: any `migrate_storage` row at all means a migration,
 * else any settled `derive` job means a render, else it is read as a scan. This is what
 * a batch this page never clicked into falls back to: a sweep or a migration started
 * with `curl`, or a migration the worker queued on its own at startup and nothing on
 * this page ever asked for.
 *
 * Migration is checked on `migrating` (rows of that kind), not `migrated` (settled
 * outcomes), and checked first. `migrated` stays 0 until a `migrate_storage` job
 * actually finishes, but the worker's startup enqueue is the ONLY way a migration batch
 * can exist, so every migration a browser can watch starts at `migrating: 1, migrated:
 * 0` — reading `migrated` here would misreport every migration as a scan for the whole
 * duration of its first run, which is worst on exactly the large corpus where that run
 * is slowest. `render` has no rows-based equivalent yet and keeps reading `rendered`
 * (settled outcomes), so it keeps the same one-job blind window `migrate` used to have —
 * a pre-existing gap this task does not extend, not one it closes.
 *
 * What this reads at all is a batch mixing more than one kind: nothing enqueues one —
 * `enqueue` is called once per payload kind, a scan's own children are all
 * `ingest_file`, and a migration's are all `migrate_storage`.
 */
type BatchKind = 'scan' | 'render' | 'migrate'

function progressText(status: BatchStatus, kind: BatchKind): string {
  if (status.finishedAt === null) {
    const settled = jobsSettled(status)
    if (kind === 'render') {
      return strings.render.running(settled, status.total)
    }
    if (kind === 'migrate') {
      return strings.migrate.running
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
  if (kind === 'render') {
    return strings.render.finished(status.rendered)
  }
  if (kind === 'migrate') {
    // The failure count, so the sentence can stop claiming every file arrived when the
    // line beside it says some did not. `scan.finished` and `render.finished` both report
    // what happened and let the failure line qualify them; this used to assert a total.
    return strings.migrate.finished(status.failedTotal)
  }
  return strings.scan.finished(status.ingested, status.skipped)
}

export function Index({
  batch,
  folderId,
  q,
  library,
  onSelectFolder,
  onSearch,
  onSelectLibrary,
}: {
  batch?: string
  folderId?: string
  /** The query in the URL. Absent, never empty — see `validateSearch`. */
  q?: string
  /** Which library this screen is of. Every query below is keyed by it. */
  library: LibraryId
  onSelectFolder?: (folder: FolderId | null) => void
  /** Writes the query to the URL. Given `''` it removes it. */
  onSearch?: (query: string) => void
  onSelectLibrary?: (library: LibraryId) => void
}) {
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
   *
   * The selected category is part of the key, not a filter applied after the fact: two
   * categories are two different sequences of pages, and a cursor from one of them means
   * nothing in the other. The invalidation the scan effect fires still reaches both —
   * `['parts', library]` is a prefix of every one of them.
   */
  // The same query key the sidebar's own `useFolders` uses, so this is a read of the cache
  // entry that component already fills and not a second request for the same tree.
  const folders = useFolders(library)
  const selectedFolderName =
    folderId === undefined
      ? null
      : (folders.data?.find((folder) => folder.id === folderId)?.name ?? null)

  // Where the store is on the host, and what the whole of it holds. **One query for the
  // page**, read by two places: every card needs the host root to show a path, and the
  // panel at the foot needs the totals. Two `useQuery` calls on one route — even under
  // different keys — would be two requests for one fact, and the second would answer a
  // question nobody asked twice.
  //
  // `measure` is part of the key rather than a refetch, so asking for the disk walk is a
  // different query with its own cached answer: pressing the button once and scrolling away
  // does not re-walk the store on the way back.
  const [measure, setMeasure] = useState(false)
  /**
   * Read once, from this browser's storage, keyed by library. Lazy initialisers because
   * `localStorage` is a synchronous read and there is no reason to do it on every render —
   * and because the accessor itself throws where site data is blocked, which the helpers
   * catch.
   */
  const [pageSize, setPageSizeState] = useState<PageSize>(() => pageSizeFor(library))
  const [density, setDensityState] = useState<Density>(() => densityFor(library))
  const instance = useQuery({
    queryKey: ['instance-storage', measure],
    queryFn: () => fetchInstanceStorage(measure),
  })

  const parts = useInfiniteQuery({
    // `q` is part of the key, so a result set is cached per query rather than one cache
    // entry being overwritten by whatever was typed last. The scan-completion invalidation
    // is `['parts', library]`, still a prefix of every one of these, so nothing about it
    // changes.
    // `pageSize` is in the key: changing it changes what a page *is*, so the pages already
    // held describe a different question and re-using them would show 50-card pages under a
    // grid that says 250.
    queryKey: ['parts', library, folderId ?? null, q ?? null, pageSize],
    queryFn: ({ pageParam }) =>
      fetchParts(library, pageParam, undefined, folderId, q, pageSize),
    initialPageParam: undefined as PartId | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
  // Flattened once per render rather than at each use: three things read it (the grid,
  // the extent line and the empty state) and they must agree about how many parts there
  // are.
  const loaded = parts.data?.pages.flatMap((page) => page.parts) ?? []
  const scan = useQuery({
    queryKey: ['batch', library, activeBatch],
    queryFn: () => fetchBatchStatus(library, activeBatch as string),
    enabled: activeBatch !== undefined,
    // The poll stops itself. A batch that finishes while the tab is backgrounded must not
    // leave a closed laptop asking about a completed scan forever — spec §11's last risk,
    // which is easy to forget and so has its own test.
    //
    // It is also the *fallback* now rather than the only path: the stream below writes
    // into this same cache entry, and a browser polls nothing on a hidden document. The
    // poll is kept because `EventSource` fails in ways a page cannot see — a proxy that
    // buffers `text/event-stream` breaks it silently — and because every progress test in
    // this suite is written against it.
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })

  /**
   * The same status, streamed, so the progress line keeps moving on a hidden tab.
   *
   * `2026-09-05-phase-1-slice-5-HANDOFF.md` recorded the freeze this closes: react-query
   * does not poll a hidden document, so a user who dropped a thousand files and switched
   * tabs came back to a line stopped where they left it. A browser keeps an `EventSource`
   * open on a hidden tab.
   *
   * It writes into the poll's cache entry rather than into state of its own, so there is
   * one status on the page and not two that can disagree. Whichever arrives last wins,
   * which is correct: both read the same row.
   */
  useEffect(() => {
    if (activeBatch === undefined) {
      return
    }
    const source = new EventSource(batchEventsUrl(library, activeBatch))
    source.onmessage = (event) => {
      const status = JSON.parse(event.data) as BatchStatus
      queryClient.setQueryData(['batch', library, activeBatch], status)
      // The server closes after the last event, and `EventSource` answers a closed stream
      // by reconnecting — forever, on a batch that will never change again. Closing from
      // this side is what stops that, and it is the same hazard `refetchInterval`
      // returning `false` closes for the poll.
      if (status.finishedAt != null) {
        source.close()
      }
    }
    // An error is not reported to the page beyond this: `EventSource` cannot read a status
    // code or a body. Closing hands the batch back to the poll above, which can.
    source.onerror = () => source.close()
    return () => source.close()
  }, [activeBatch, queryClient])

  const kind: BatchKind =
    started?.kind ??
    ((scan.data?.migrating ?? 0) > 0
      ? 'migrate'
      : (scan.data?.rendered ?? 0) > 0
        ? 'render'
        : 'scan')

  /**
   * What this library is actually set to. Its own query rather than a field on the grid's
   * page, because the two answer different questions and a settings read must not be
   * invalidated every time the worker commits a part.
   */
  const librarySettings = useQuery({
    queryKey: ['library', library],
    queryFn: () => fetchLibrarySettings(library),
  })
  /**
   * What this library occupies. Its own query for the reason the settings read is one:
   * it answers a different question from the grid's page, and it answers it about the
   * whole library rather than about the 50 parts a page holds.
   */
  const storage = useQuery({
    queryKey: ['storage', library],
    queryFn: () => fetchLibraryStorage(library),
  })
  const settings = useMutation({
    mutationFn: (on: boolean) => setAutoThumbnail(library, on),
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
    mutationFn: () => startScan(library),
    onSuccess: (accepted) => watch(accepted, 'scan'),
  })
  const sweep = useMutation({
    mutationFn: () => renderLibraryThumbnails(library),
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
      uploadFiles(library, picked, setUploading),
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
      void queryClient.invalidateQueries({ queryKey: ['parts', library] })
    }
    // The totals move with the grid, and nothing else would tell them so. A scan that
    // ingests 151 parts under a line still reporting the pre-scan figure is a
    // measurement contradicted by the cards directly above it. One row either way, so it
    // is not worth gating.
    void queryClient.invalidateQueries({ queryKey: ['storage', library] })
  }, [settled, pagesLoaded, batchFinished, queryClient])

  const note = scanNow.isError
    ? strings.scan.startFailed
    : sweep.isError || renderPart.isError
      ? strings.render.queueFailed
      : sweep.data?.queued === 0
        ? strings.render.nothingMissing
        : null

  return (
    <section className="flex items-start gap-6">
      {/*
        The tree and the grid are siblings, and the drag between them needs nothing
        shared: a card writes its identity into the drag payload and a category row
        reads it back on drop, so neither holds state for the other.
      */}
      <FolderTree
        library={library}
        selected={folderId ?? null}
        onSelect={(folder) => onSelectFolder?.(folder)}
      />
      <div className="min-w-0 flex-1">
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
        {/*
          Between the action bar and the drop target, and not inside the action bar. That row
          is a row of *actions* — a checkbox and two buttons — and putting a persistent text
          filter among them makes it read as "type here, then press Scan".
        */}
        <LibrarySwitcher library={library} onSelect={onSelectLibrary} />
        <GridSettings
          pageSize={pageSize}
          density={density}
          onPageSize={(size) => {
            setPageSizeState(size)
            setPageSize(library, size)
          }}
          onDensity={(next) => {
            setDensityState(next)
            setDensity(library, next)
          }}
        />
        <SearchBox
          q={q ?? ''}
          categoryName={selectedFolderName}
          filtered={folderId !== undefined}
          onSearch={onSearch}
          onWiden={() => onSelectFolder?.(null)}
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
          // that came back empty gets the empty state — and which empty state depends on
          // whether a category is filtering it, because "this library is empty" is false
          // and alarming when the library is full and the category is not.
          <EmptyLibrary
            filtered={folderId !== undefined}
            categoryName={selectedFolderName}
            query={q ?? null}
            onWiden={() => onSelectFolder?.(null)}
          />
        ) : (
          <>
            <Grid
              parts={loaded}
              onRender={(part) => renderPart.mutate(part)}
              busyPart={renderPart.isPending ? renderPart.variables : undefined}
              hostRoot={instance.data?.hostStorageRoot ?? null}
              density={density}
            />
            <MorePages
              count={loaded.length}
              hasMore={parts.hasNextPage}
              fetching={parts.isFetchingNextPage}
              onMore={() => void parts.fetchNextPage()}
            />
            <StorageTotals storage={storage.data} isError={storage.isError} />
            <InstanceStorage
              instance={instance.data}
              isError={instance.isError}
              measuring={measure && instance.isFetching}
              measured={measure}
              onMeasure={() => setMeasure(true)}
            />
          </>
        )}
        <p className="mt-6 text-sm text-[var(--color-muted)]">
          {health.isPending
            ? strings.health.checking
            : health.isError
              ? strings.health.failed
              : strings.health.ok(health.data.database.major)}
        </p>
      </div>
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
      {/*
        A link and not a button: the removed list is a place, not an action, and it is the
        only route back to a part somebody removed. Its absence would make removing a
        one-way door — every other read path filters `deleted_at`, so nothing else in this
        app can name a removed part again.
      */}
      <Link
        to="/removed"
        className="ease-mechanical text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
      >
        {strings.removal.removedTitle}
      </Link>
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
  //
  // All three kinds, and `migrate` is not a fall-through to `scan`'s copy. A failed
  // migration used to render "3 files could not be read. They will not appear in the grid"
  // about models that already exist and are already in the grid, and a status poll that
  // failed rendered "No scan with that id has run in this library" to an operator who
  // never started a scan. A non-destructive failure worded as data loss is the one class of
  // mistake this product treats as a correctness bug.
  const copy =
    kind === 'render' ? strings.render : kind === 'migrate' ? strings.migrate : strings.scan
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

/**
 * Which library this screen is of, and the control that makes another one.
 *
 * Hidden entirely while there is one library — which is every deployment until somebody
 * makes a second. A switcher offering one choice is a control that explains nothing and
 * takes a row of the screen to do it; the "New library" button stays, because that is how
 * the second one gets made.
 */
function LibrarySwitcher({
  library,
  onSelect,
}: {
  library: LibraryId
  onSelect?: (library: LibraryId) => void
}) {
  const [creating, setCreating] = useState(false)
  const queryClient = useQueryClient()
  const libraries = useQuery({ queryKey: ['libraries'], queryFn: fetchLibraries })
  const [refusal, setRefusal] = useState<string | null>(null)

  const add = useMutation({
    mutationFn: (body: NewLibrary) => createLibrary(body),
    onMutate: () => setRefusal(null),
    onSuccess: (result) => {
      if (result.kind === 'refused') {
        setRefusal(result.message)
        return
      }
      setCreating(false)
      void queryClient.invalidateQueries({ queryKey: ['libraries'] })
      // Straight into it: somebody who just made a library meant to use it, and leaving
      // them on the old one is a second step for no reason.
      onSelect?.(result.library.id)
    },
  })

  const all = libraries.data ?? []
  return (
    <div className="mb-3 flex flex-wrap items-center gap-3 text-xs text-[var(--color-muted)]">
      {all.length < 2 ? null : (
        <label className="flex items-center gap-2">
          {strings.libraries.label}
          <select
            value={library}
            onChange={(event) => onSelect?.(event.target.value as LibraryId)}
            className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1"
          >
            {all.map((one) => (
              <option key={one.id} value={one.id}>
                {strings.libraries.option(one.name, one.partCount)}
              </option>
            ))}
          </select>
        </label>
      )}
      <button
        type="button"
        onClick={() => setCreating(true)}
        className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.libraries.create}
      </button>
      {libraries.isError ? <span role="alert">{strings.libraries.failed}</span> : null}
      {!creating ? null : (
        <NewLibraryDialog
          busy={add.isPending}
          note={refusal ?? (add.isError ? strings.libraries.createFailed : null)}
          onConfirm={(body) => add.mutate(body)}
          onCancel={() => {
            setCreating(false)
            setRefusal(null)
          }}
        />
      )}
    </div>
  )
}

/**
 * A name and a governance mode, chosen once.
 *
 * The mode is at creation because later means asking about a library somebody has already
 * filled — and the copy says nothing reads it yet, rather than implying a switch that does
 * something today. `CLAUDE.md`: governance is opt-in per library, and flipping a library to
 * `controlled` is what turns that machinery on when Phase 8 builds it.
 */
function NewLibraryDialog({
  busy,
  note,
  onConfirm,
  onCancel,
}: {
  busy: boolean
  note: string | null
  onConfirm: (body: NewLibrary) => void
  onCancel: () => void
}) {
  const [name, setName] = useState('')
  const [mode, setMode] = useState<NewLibrary['mode']>('hobby')
  const trimmed = name.trim()
  return (
    <Dialog title={strings.libraries.createTitle} onClose={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (trimmed !== '' && !busy) onConfirm({ name: trimmed, mode })
        }}
      >
        <input
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-label={strings.libraries.nameLabel}
          autoFocus
          className="mt-3 w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-sm"
        />
        <label className="mt-3 flex flex-col gap-1 text-xs text-[var(--color-muted)]">
          {strings.libraries.modeLabel}
          <select
            value={mode}
            onChange={(event) => setMode(event.target.value as NewLibrary['mode'])}
            className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-sm"
          >
            {LIBRARY_MODES.map((option) => (
              <option key={option} value={option}>
                {MODE_LABEL[option]}
              </option>
            ))}
          </select>
        </label>
        {note === null ? null : (
          <p role="alert" className="mt-2 text-sm text-[var(--color-muted)]">
            {note}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="ease-mechanical rounded border border-[var(--color-border)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.cancel}
          </button>
          <button
            type="submit"
            disabled={busy || trimmed === ''}
            className="ease-mechanical rounded border border-[var(--color-border)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.libraries.createConfirm}
          </button>
        </div>
      </form>
    </Dialog>
  )
}

/** The two modes, and their labels — a lookup rather than a ternary in JSX, for the reason
 * `DENSITY_LABEL` above gives. */
const LIBRARY_MODES = ['hobby', 'controlled'] as const
const MODE_LABEL: Record<(typeof LIBRARY_MODES)[number], string> = {
  hobby: strings.libraries.hobby,
  controlled: strings.libraries.controlled,
}

/**
 * How many cards a page holds, and how tightly they pack.
 *
 * Two `<select>`s and no custom widget: a native select is keyboard-operable, screen-reader
 * announced and correct on a touch screen for free, and this is a preference rather than a
 * place to spend design on.
 *
 * Both are remembered per library in this browser — not on the server. There is no user
 * table and no auth in Phase 1, so a column would make one operator's choice everybody's;
 * `FEATURES.md` says "per viewer, per library" now, because that is what this is.
 */
/**
 * The label for each density. A lookup and not a ternary in the JSX: a `option === 'compact'`
 * inside a child expression puts the literal `'compact'` where `no-bare-strings.test.ts`
 * reads it — correctly — as a label reaching the screen. Same trap `ShowInFolder` and
 * `InstanceStorage` both carry a comment about.
 */
const DENSITY_LABEL: Record<Density, string> = {
  comfortable: strings.grid.comfortable,
  compact: strings.grid.compact,
}

function GridSettings({
  pageSize,
  density,
  onPageSize,
  onDensity,
}: {
  pageSize: PageSize
  density: Density
  onPageSize: (size: PageSize) => void
  onDensity: (density: Density) => void
}) {
  return (
    <div className="mb-3 flex flex-wrap items-center gap-4 text-xs text-[var(--color-muted)]">
      <label className="flex items-center gap-2">
        {strings.grid.pageSize}
        <select
          value={pageSize}
          onChange={(event) => onPageSize(Number(event.target.value) as PageSize)}
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1"
        >
          {PAGE_SIZES.map((size) => (
            <option key={size} value={size}>
              {strings.grid.pageSizeOption(size)}
            </option>
          ))}
        </select>
      </label>
      <label className="flex items-center gap-2">
        {strings.grid.density}
        <select
          value={density}
          onChange={(event) => onDensity(event.target.value as Density)}
          className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1"
        >
          {DENSITIES.map((option) => (
            <option key={option} value={option}>
              {DENSITY_LABEL[option]}
            </option>
          ))}
        </select>
      </label>
    </div>
  )
}

/**
 * The search box, and the chip that says what it is searching.
 *
 * A `<input type="search">`, so the clear affordance, Escape-to-clear and the right mobile
 * keyboard come from the browser rather than from code here.
 *
 * **Local state, debounced navigation.** The field responds to every keystroke and the URL
 * does not: `navigate` on each one would render the route per character and — without
 * `replace` — put every character in the back button's history. 250 ms is the pause after
 * typing, not a delay before feedback.
 *
 * **Two characters minimum, and it is not arbitrary.** A trigram is three characters, so
 * under that `gin_trgm_ops` cannot be used at all and the query is a sequential scan by
 * construction. The box accepts the keystroke and says it is waiting.
 *
 * ponytail: two characters because of the index, not because of the product. If a
 * one-character search is ever wanted, the fix is a prefix index, not removing this.
 */
function SearchBox({
  q,
  categoryName,
  filtered,
  onSearch,
  onWiden,
}: {
  q: string
  categoryName: string | null
  filtered: boolean
  onSearch?: (query: string) => void
  onWiden: () => void
}) {
  const [typed, setTyped] = useState(q)
  // The URL is the source of truth: a back navigation or a shared link has to move the box,
  // and without this the field would keep whatever was last typed into it.
  const [lastFromUrl, setLastFromUrl] = useState(q)
  if (q !== lastFromUrl) {
    setLastFromUrl(q)
    setTyped(q)
  }

  useEffect(() => {
    const trimmed = typed.trim()
    // Below the minimum the query is not run — but an empty box *is* a change, because it
    // means "show me the library again".
    if (trimmed.length === 1) return
    if (trimmed === q) return
    const timer = setTimeout(() => onSearch?.(trimmed), 250)
    return () => clearTimeout(timer)
  }, [typed, q, onSearch])

  const waiting = typed.trim().length === 1
  return (
    <div className="mb-4 flex flex-wrap items-center gap-2">
      <input
        type="search"
        value={typed}
        onChange={(event) => setTyped(event.target.value)}
        aria-label={strings.search.label}
        placeholder={strings.search.placeholder}
        className="min-w-64 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-sm"
      />
      {/*
        The disclosure, not a control that narrows. The sidebar has already narrowed the
        grid; a search that quietly kept that narrowing without saying so is how somebody
        concludes a part is missing from the library. Dismissing it widens and keeps the
        query.
      */}
      {!filtered || q === '' ? null : (
        <button
          type="button"
          onClick={onWiden}
          title={strings.search.widen}
          className="ease-mechanical rounded-full border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {categoryName === null
            ? strings.search.inThisCategory
            : strings.search.inCategory(categoryName)}{' '}
          ×
        </button>
      )}
      {!waiting ? null : (
        <p className="text-xs text-[var(--color-muted)]">{strings.search.keepTyping}</p>
      )}
    </div>
  )
}

/**
 * Nothing to show, and which "nothing" it is.
 *
 * `filtered` and not "is `categoryName` null": the two answer different questions, and only
 * the first is safe to render from. The category's name comes from the tree, which is a
 * different query from the grid's — so a page can know the category holds nothing before it
 * knows what the category is called, and a component that inferred "no category" from a
 * missing name would tell that user their library is empty.
 */
function EmptyLibrary({
  filtered,
  categoryName,
  query,
  onWiden,
}: {
  filtered: boolean
  categoryName: string | null
  /** The query that found nothing, or `null` when nobody searched. */
  query: string | null
  onWiden: () => void
}) {
  // A search that found nothing is not an empty library, and saying so is worse than
  // useless: the user did not empty anything, they typed something, and "drop a folder of
  // models above to add them" is an instruction for a problem they do not have.
  if (query !== null) {
    return (
      <div className="max-w-prose">
        <h2 className="text-lg">{strings.emptyLibrary.categoryTitle}</h2>
        <p className="mt-2 text-[var(--color-muted)]">
          {filtered
            ? strings.search.noMatchesInCategory(
                query,
                categoryName ?? strings.search.inThisCategory,
              )
            : strings.search.noMatches(query)}
        </p>
        {/*
          The narrowed case is the one that matters. Somebody searching inside a category
          and finding nothing has to be able to widen without first working out that the
          sidebar was the reason.
        */}
        {!filtered ? null : (
          <button
            type="button"
            onClick={onWiden}
            className="ease-mechanical mt-3 rounded border border-[var(--color-border)] px-2 py-1 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.search.widen}
          </button>
        )}
      </div>
    )
  }
  return (
    <div className="max-w-prose">
      <h2 className="text-lg">
        {filtered ? strings.emptyLibrary.categoryTitle : strings.emptyLibrary.title}
      </h2>
      <p className="mt-2 text-[var(--color-muted)]">
        {filtered ? strings.emptyLibrary.categoryBody(categoryName) : strings.emptyLibrary.body}
      </p>
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
 * nothing the empty state has not already said better. Both totals are bytes on disk —
 * source files counted one per part, derivatives deduplicated, and `PgParts::storage_totals`
 * is where that accounting is written down — and the ratio arrives computed rather than
 * divided here, so a second reader cannot report the same library the other way up.
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
      {/*
        Only when there is something to say. A permanent "0 B removed" would be noise on
        every library that has never removed anything, which is most of them — but the
        moment one exists, the totals above stop describing the whole volume and this is
        what says so.
      */}
      {storage.removedBytes > 0 ? strings.storage.removed(storage.removedBytes) : null}
    </p>
  )
}

/**
 * What the whole store holds, under the one library's line.
 *
 * Separate from `StorageTotals` and not folded into it, because it answers a different
 * question and the two disagree on purpose: a derivative two libraries share is charged to
 * both of them above and counted once here, and the quarantined figure belongs to no
 * library at all. Adding the panels up is exactly the thing this is here to stop somebody
 * doing.
 *
 * The disk measurement is a button rather than part of the load. It costs the server a
 * `stat` per file — instant on a small library, seconds on a corpus — and the four figures
 * beside it are free, so making everyone pay for it on every page load to answer a question
 * most visits do not ask would be the wrong default.
 */
function InstanceStorage({
  instance,
  isError,
  measuring,
  measured,
  onMeasure,
}: {
  instance?: InstanceStorageView
  isError: boolean
  /** The walk is in flight. */
  measuring: boolean
  /** The walk has been asked for, whether or not it came back. */
  measured: boolean
  onMeasure: () => void
}) {
  // Same rule as the library totals: nothing at all while the first read is in flight,
  // because a total is a claim and there is no honest placeholder for one.
  if (isError) {
    return <p className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">{strings.storage.failed}</p>
  }
  if (instance === undefined) {
    return null
  }

  const { sourceBytes, derivativeBytes, inlinePreviewBytes, removedBytes, quarantinedBytes, onDiskBytes } =
    instance
  // Deliberately **without** `inlinePreviewBytes`: those are in Postgres, and this figure
  // is compared against a walk of the storage folder. Including them put the tracked total
  // above the disk by exactly their size on a real library, which reads as loss.
  const tracked = sourceBytes + derivativeBytes + removedBytes + quarantinedBytes
  // Narrowed here and not in the JSX, for the reason `ShowInFolder` narrows where it does:
  // a `typeof x === 'number'` inside a child expression puts the literal `'number'` in a
  // position `no-bare-strings.test.ts` reads — correctly — as a label reaching the screen.
  // `typeof` and not truthiness, because a genuinely empty store measures 0, which is an
  // answer rather than a missing one.
  const onDisk = typeof onDiskBytes === 'number' ? onDiskBytes : null
  return (
    <div className="mt-1 max-w-prose text-xs text-[var(--color-muted)]">
      <p>
        {strings.storage.everything(
          sourceBytes,
          derivativeBytes,
          inlinePreviewBytes,
          removedBytes,
          quarantinedBytes,
        )}
      </p>
      {onDisk !== null ? (
        <p className="mt-1">{strings.storage.onDisk(onDisk, tracked)}</p>
      ) : measuring ? (
        <p className="mt-1">{strings.storage.measuring}</p>
      ) : measured ? (
        // Asked for and not answered: the walk failed, and the four figures above are still
        // true because they never came from the disk.
        <p className="mt-1">{strings.storage.onDiskFailed}</p>
      ) : (
        <button
          type="button"
          onClick={onMeasure}
          className="ease-mechanical mt-1 rounded border border-[var(--color-border)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.storage.measureOnDisk}
        </button>
      )}
    </div>
  )
}

function Grid({
  parts,
  onRender,
  busyPart,
  hostRoot,
  density,
}: {
  parts: readonly PartCard[]
  onRender: (part: PartId) => void
  busyPart?: PartId
  /** Passed down rather than fetched per card: it is one fact about the deployment. */
  hostRoot: string | null
  density: Density
}) {
  // Two numbers move together and have to: the column width sets how tall a card ends up,
  // and `contain-intrinsic-size` is the placeholder height for one that has not rendered.
  // Give the compact grid the comfortable card's height and the scrollbar jumps as cards
  // enter and leave — which is what makes `content-visibility` look broken.
  //
  // Whole class strings rather than interpolation: Tailwind scans source for literals, and
  // a class built at runtime is a class that was never generated.
  const columns =
    density === 'compact'
      ? 'grid-cols-[repeat(auto-fill,minmax(8rem,1fr))] gap-3'
      : 'grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4'
  //
  // **A compact card is TALLER, not shorter**, and guessing the other way was the first
  // thing this got wrong. A narrower column wraps more of the name and more of the "9.7 kB
  // on disk, stored uncompressed" line, so 8rem-wide cards run past 11rem-wide ones.
  // Measured in Chrome over 24 cards of the real 156-part library: comfortable 442–461px
  // (median 27.6rem), compact 478–516px (median 31.1rem). The same mistake the 26rem figure
  // below already records making once — a guess, in the wrong direction, about a height
  // that has to be measured.
  const intrinsic =
    density === 'compact'
      ? '[contain-intrinsic-size:auto_31rem]'
      : '[contain-intrinsic-size:auto_26rem]'
  return (
    <ul className={`grid list-none ${columns}`}>
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
        <li key={part.id} className={`[content-visibility:auto] ${intrinsic}`}>
          <Card part={part} onRender={onRender} busy={part.id === busyPart} hostRoot={hostRoot} />
        </li>
      ))}
    </ul>
  )
}

function Card({
  part,
  onRender,
  busy,
  hostRoot,
}: {
  part: PartCard
  onRender: (part: PartId) => void
  busy: boolean
  hostRoot: string | null
}) {
  const nameId = `part-name-${part.id}`
  const [moving, setMoving] = useState(false)
  const [looking, setLooking] = useState(false)
  const directory = part.directory
  // A model still in the shared store has no directory to rename, and the move route
  // refuses it. The card withholds the move rather than letting the user discover that
  // from a `409` — the same status the route uses for a name collision, which the UI
  // would otherwise present as one.
  const movable = directory !== null
  return (
    <article
      aria-labelledby={nameId}
      /*
        The whole card opens the quick look, and it is a handler rather than an anchor for
        the reason the name's own comment gives below: this card holds a render button, a
        move button, a download link and a path disclosure, and nesting those inside an
        `<a>` is invalid HTML that browsers resolve by guessing.

        So the click is filtered instead of the markup being reshaped. Anything that
        originated inside a control belongs to that control — including a click on a label
        inside a button, which is why this asks `closest` rather than comparing the target.
        The name stays a real `Link`: it is the keyboard path, the middle-click path, and
        what a screen reader announces for the card.
      */
      onClick={(event) => {
        if (!(event.target instanceof Element)) return
        if (event.target.closest('a, button, input')) return
        setLooking(true)
      }}
      draggable={movable}
      onDragStart={(event) =>
        event.dataTransfer.setData(
          PART_DRAG_TYPE,
          partDragPayload({ id: part.id, name: part.name }),
        )
      }
      // Right-click opens the same chooser the button does, so the pointer gesture people
      // expect from a file manager is there without being the only way in.
      onContextMenu={(event) => {
        if (movable) {
          event.preventDefault()
          setMoving(true)
        }
      }}
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
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => onRender(part.id)}
            disabled={busy}
            aria-label={strings.render.partFor(part.name)}
            className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.render.part}
          </button>
          {/*
            The keyboard path to a move, and not a fallback: dragging a card into a
            scrolled tree is a poor trackpad target and impossible without a pointer. A
            model with no directory of its own gets the reason instead of a control that
            would fail at the server.
          */}
          {movable ? (
            <button
              type="button"
              onClick={() => setMoving(true)}
              aria-label={strings.folders.moveToFor(part.name)}
              className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
            >
              {strings.folders.moveTo}
            </button>
          ) : (
            <span className="text-xs text-[var(--color-muted)]">{strings.folders.notMigrated}</span>
          )}
        </div>
        <ShowInFolder part={part} hostRoot={hostRoot} />
        {/*
          Written here and rendered at `<body>`: `Dialog` portals itself, and it has to.
          This card is `overflow-hidden hover:-translate-y-0.5`, Tailwind emits that lift as
          the `translate` property, and an element with a `translate` other than `none` is a
          containing block for fixed-position descendants — so a dialog rendered in the
          card's own subtree resolved its `fixed inset-0` against the card and was clipped
          to it for as long as the pointer stayed over the card. Nothing here may hoist that
          markup back out of the portal.
        */}
        {looking ? <QuickLook part={part} onClose={() => setLooking(false)} /> : null}
        {moving ? (
          <MovePartDialog
            part={{ id: part.id, name: part.name }}
            library={part.library}
            onClose={() => setMoving(false)}
          />
        ) : null}
      </div>
    </article>
  )
}

/**
 * The part, in a panel, without leaving the grid.
 *
 * Scanning a library means looking at one part and then the next, and a round trip through
 * a full page and the back button for each of them is what makes that tiring. So this is a
 * look; the page is where the controls that change something live, and where a URL someone
 * can share lives.
 *
 * **It renders `Detail`, the detail page's own article, rather than a version of it.** Two
 * renderings of one measurement that can disagree is a defect here, and measurements are
 * the case that matters: every figure goes through `Figure`, which cannot render a value
 * without its `approximate` flag, because a mesh-derived number must be labelled wherever
 * it appears.
 *
 * **And it fetches under the detail route's own query key**, so opening the panel and then
 * the page costs one request rather than two — the look warms the cache for the page it
 * links to.
 */
function QuickLook({ part, onClose }: { part: PartCard; onClose: () => void }) {
  const detail = useQuery({
    queryKey: ['part', part.id],
    queryFn: () => fetchPartDetail(part.id),
  })
  return (
    <Dialog title={part.name} onClose={onClose}>
      {detail.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.quickLook.loading}</p>
      ) : detail.isError ? (
        <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.quickLook.failed}
        </p>
      ) : (
        <Detail part={detail.data} />
      )}
      <div className="mt-4 flex justify-end gap-2">
        <Link
          to="/parts/$partId"
          params={{ partId: part.id }}
          className="ease-mechanical rounded border border-[var(--color-border)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.quickLook.fullPage}
        </Link>
      </div>
    </Dialog>
  )
}

/**
 * Where this model is on disk — shown, not opened.
 *
 * No browser opens a host file manager: `file://` navigation from a page is blocked
 * everywhere, and a button that claimed otherwise would be a control that cannot work. So
 * this reveals the path as selectable, copyable text and says why it is a path. A native
 * reveal belongs to the Tauri shell, which has a host to ask.
 *
 * The path is never assembled here from category names. The server disambiguates
 * colliding directory names — the second `cliff` becomes `cliff_a1b2c3` — and the client
 * cannot know when it did, so a path joined from slugs would be confidently wrong exactly
 * where it matters.
 */
function ShowInFolder({
  part,
  hostRoot,
}: {
  part: PartCard
  /**
   * Where the store is on the host, or `null` when the deployment has not said.
   *
   * Never derived here and never guessed. The api sees the store at a container path that
   * exists on nobody's machine, so if this is `null` the honest answer is the path within
   * the store — which is what the copy then says, along with how to fix it.
   */
  hostRoot: string | null
}) {
  const [open, setOpen] = useState(false)
  // The file, not its directory: "where is this model" is answered by the path to the
  // model, and the directory is one `rsplit` away for anyone who wants it. Narrowed with
  // `typeof` rather than in the JSX because a `!== 'string'` inside a child expression puts
  // the literal `'string'` where `no-bare-strings.test.ts` reads it — correctly — as a
  // label reaching the screen.
  const relative = typeof part.storagePath === 'string' ? part.storagePath : null
  // Joined with a single slash and no path library: `hostRoot` is absolute or absent (the
  // server drops a relative one), and the store-relative path never starts with one, so the
  // only case to handle is a trailing slash on the root.
  const path =
    relative === null ? null : hostRoot === null ? relative : `${hostRoot.replace(/\/$/, '')}/${relative}`
  return (
    <div className="mt-2 text-xs text-[var(--color-muted)]">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-label={strings.folders.showInFolderFor(part.name)}
        className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {strings.folders.showInFolder}
      </button>
      {!open ? null : path === null ? (
        <p className="mt-2">{strings.folders.directoryPending}</p>
      ) : (
        <div className="mt-2 space-y-2">
          {/* `select-all` so one click takes the whole path, which is what a person does
              with it — and `break-all` because a nested category path is longer than a
              card is wide. */}
          <code className="block font-mono break-all select-all text-[var(--color-text)]">
            {path}
          </code>
          <button
            type="button"
            onClick={() => {
              // Absent in an insecure context, and a rejected permission is not worth an
              // error state: the path is on screen and selectable either way.
              void navigator.clipboard?.writeText(path).catch(() => undefined)
            }}
            className="ease-mechanical rounded border border-[var(--color-border)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.copyPath}
          </button>
          <p>{hostRoot === null ? strings.folders.directoryHint : strings.folders.directoryHintAbsolute}</p>
          {hostRoot === null ? <p>{strings.folders.directoryPartial}</p> : null}
        </div>
      )}
    </div>
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
