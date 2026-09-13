import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react'
import {
  DEFAULT_LIBRARY_ID,
  batchEventsUrl,
  downloadUrl,
  fetchBatchStatus,
  fetchFailures,
  movePart,
  removePart,
  retryFailed,
  fetchHealth,
  createLibrary,
  fetchInstanceStorage,
  fetchLibraries,
  fetchLibrarySettings,
  fetchLibraryStorage,
  fetchPartDetail,
  fetchFacets,
  fetchParts,
  renderLibraryThumbnails,
  renderPartThumbnail,
  setAutoThumbnail,
  startScan,
} from '../lib/api'
import { flipFrom } from '../lib/flip'
import { Dialog } from '../components/Dialog'
import { ShowInFolder } from '../components/ShowInFolder'
import { Detail } from '../components/PartDetail'
import {
  DENSITIES,
  LAYOUTS,
  PAGE_SIZES,
  SORTS,
  densityFor,
  layoutFor,
  pageSizeFor,
  setDensity,
  setLayout,
  setPageSize,
  setSort,
  sortFor,
  type Density,
  type Layout,
  type PageSize,
  type Sort,
} from '../lib/preferences'
import { strings } from '../lib/strings'
import { eachAtMost } from '../lib/bulk'
import { filesFromDrop, filesFromInput, uploadFiles } from '../lib/upload'
import type { PickedFile, UploadProgress } from '../lib/upload'
import {
  FolderTree,
  MovePartDialog,
  PickCategoryDialog,
  refusalMessage,
  PART_DRAG_TYPE,
  partDragPayload,
  useFolders,
} from '../components/FolderTree'
import type {
  BatchId,
  BatchStatus,
  JobFailure,
  JobId,
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
  ): { batch?: string; folderId?: string; q?: string; library?: string; format?: string; part?: string } => {
    const batch = search.batch
    const folderId = search.folderId
    const q = search.q
    const library = search.library
    const format = search.format
    const part = search.part
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
      // A format as ingest records it: the extension, lowercased. Absent is every format.
      ...(typeof format === 'string' && format.length > 0 ? { format: format.toLowerCase() } : {}),
      // The part the quick look is open on. A UUID, so a string whenever it is anything.
      ...(typeof part === 'string' && part.length > 0 ? { part } : {}),
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
  const { batch, folderId, q, library, format, part } = Route.useSearch()
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
          search: (previous) => ({ ...previous, folderId: folder ?? undefined, part: undefined }),
        })
      }
      format={format}
      onSelectFormat={(value) =>
        void navigate({
          search: (previous) => ({ ...previous, format: value ?? undefined, part: undefined }),
        })
      }
      part={part}
      onOpenPart={(id) =>
        void navigate({
          search: (previous) => ({ ...previous, part: id ?? undefined }),
          // One history entry however many parts are looked at, so Back leaves the grid rather
          // than stepping through every card clicked on the way — and Back from the full page
          // still lands on the pane it was opened from.
          replace: true,
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
type BatchKind = 'scan' | 'render' | 'migrate' | 'upload'

function progressText(status: BatchStatus, kind: BatchKind): string {
  if (status.finishedAt === null) {
    const settled = jobsSettled(status)
    if (kind === 'render') {
      return strings.render.running(settled, status.total)
    }
    if (kind === 'migrate') {
      return strings.migrate.running
    }
    // No walk job in an upload's batch, so nothing to subtract and no walk to wait for.
    if (kind === 'upload') {
      return strings.upload.batchRunning(settled, status.total)
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
  if (kind === 'upload') {
    return strings.upload.batchFinished(status.ingested, status.skipped)
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
  format,
  onSelectFormat,
  part,
  onOpenPart,
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
  /** The format the grid is narrowed to, as the URL carries it. Absent is every format. */
  format?: string
  onSelectFormat?: (format: string | null) => void
  /** The part the quick look is open on, as the URL carries it. */
  part?: string
  /** Writes the open part to the URL; `null` closes it. */
  onOpenPart?: (part: PartId | null) => void
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
  const [started, setStarted] = useState<
    { id: BatchId; kind: BatchKind; library: typeof library } | undefined
  >(undefined)
  // Only while the page is still on the library that started it. Switching libraries does
  // not remount this component, so the state outlives the library it belongs to — and a
  // live stack went on asking the new library about the old one's scan, and got 404 twice.
  const mine = started?.library === library ? started : undefined
  const activeBatch = mine?.id ?? batch

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
  const [layout, setLayoutState] = useState<Layout>(() => layoutFor(library))
  const [sort, setSortState] = useState<Sort>(() => sortFor(library))
  // A search is in relevance order, so the choice sits out while one runs rather than splitting
  // the cache on a parameter the route ignores. It comes back when the search is cleared.
  const order: Sort = q === undefined ? sort : 'newest'
  // Owned here rather than inside `DropTarget`, because two controls open the same picker
  // now: the drop strip's link, and the toolbar's Upload button.
  const picker = useRef<HTMLInputElement>(null)
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
    queryKey: ['parts', library, folderId ?? null, q ?? null, pageSize, format ?? null, order],
    queryFn: ({ pageParam }) =>
      fetchParts(library, pageParam, undefined, folderId, q, pageSize, format, order),
    initialPageParam: undefined as PartId | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
  // Flattened once per render rather than at each use: three things read it (the grid,
  // the extent line and the empty state) and they must agree about how many parts there
  // are.
  const loaded = parts.data?.pages.flatMap((page) => page.parts) ?? []

  /**
   * The part being looked at. The URL holds it where there is a router, so a reload and Back
   * keep it; `index.test.tsx` renders `Index` bare, and there it is this component's own.
   *
   * Where the card sat when it was clicked never goes in the URL. A pane reopened by a reload
   * simply appears, and only a click flies — a rectangle from another page load would be a
   * flight from somewhere the card no longer is.
   */
  const [ownPart, setOwnPart] = useState<string | undefined>(undefined)
  const openPart = onOpenPart === undefined ? ownPart : part
  const [openFrom, setOpenFrom] = useState<DOMRect>(DEFAULT_ORIGIN)
  const [moving, setMoving] = useState<PartCard | null>(null)
  const wide = useWide()
  // Found among the pages already loaded rather than fetched on its own: a part further down
  // than the grid has reached opens once its page arrives.
  const looking = openPart === undefined ? undefined : loaded.find((card) => card.id === openPart)
  const setOpenPart = (id: PartId | null) => {
    if (onOpenPart === undefined) setOwnPart(id ?? undefined)
    else onOpenPart(id)
  }
  const closeLook = () => {
    const id = openPart
    setOpenPart(null)
    // The pane traps nothing, so nothing gives focus back for it: return it to the card's name,
    // the keyboard's way in. The dialog restores focus itself, and doing it here as well would
    // fight its own blur handler.
    if (wide && id !== undefined) {
      document.getElementById(`part-name-${id}`)?.querySelector('a')?.focus()
    }
  }

  /**
   * Bulk selection. Ids rather than cards, so a refetch that replaces the card objects keeps
   * what is picked; off by default, so a card keeps its one tab stop.
   */
  const [selecting, setSelecting] = useState(false)
  const [selected, setSelected] = useState<ReadonlySet<PartId>>(new Set())
  const [anchor, setAnchor] = useState<PartId | null>(null)
  const [bulk, setBulk] = useState<BulkProgress | null>(null)
  const [picking, setPicking] = useState(false)
  // Ids picked out of one grid mean nothing in another: a category, format or search that
  // changes which parts are on screen would otherwise leave hidden parts selected, and a
  // bulk action would reach parts nobody can see. Sort only reorders, so it keeps them.
  const scope = [library, folderId ?? '', format ?? '', q ?? ''].join('\u0000')
  const [selectionScope, setSelectionScope] = useState(scope)
  if (scope !== selectionScope) {
    setSelectionScope(scope)
    setSelected(new Set())
    setAnchor(null)
    setBulk(null)
  }
  const toggle = (id: PartId, range: boolean) => {
    setSelected((held) => {
      const next = new Set(held)
      const from = range && anchor !== null ? loaded.findIndex((part) => part.id === anchor) : -1
      const to = loaded.findIndex((part) => part.id === id)
      if (from !== -1 && to !== -1) {
        for (const part of loaded.slice(Math.min(from, to), Math.max(from, to) + 1)) {
          next.add(part.id)
        }
      } else if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
    setAnchor(id)
  }
  const runBulk = async (task: (part: PartCard) => Promise<string | null>) => {
    const chosen = loaded.filter((part) => selected.has(part.id))
    let done = 0
    setBulk({ done, total: chosen.length, failures: [] })
    const failures = await eachAtMost(chosen, BULK_CONCURRENCY, async (part) => {
      const reason = await task(part)
      done += 1
      setBulk((now) => (now === null ? now : { ...now, done }))
      return reason
    })
    setBulk({
      done: chosen.length,
      total: chosen.length,
      failures: failures.map(({ item, reason }) => ({ id: item.id, name: item.name, reason })),
    })
    // What failed stays selected, so trying again is one press.
    setSelected(new Set(failures.map(({ item }) => item.id)))
    void queryClient.invalidateQueries({ queryKey: ['parts', library] })
    void queryClient.invalidateQueries({ queryKey: ['folders', library] })
    void queryClient.invalidateQueries({ queryKey: ['facets', library] })
    void queryClient.invalidateQueries({ queryKey: ['storage', library] })
  }
  /**
   * Every part is sent with the duplicate name acknowledged, which the one-part dialog does not
   * do. Its warning exists so a person confirms once that two models may share a name; across
   * forty parts it would be forty dialogs, and the answer to each is already in the choice to
   * move them all. The refusals that are dead ends still come back, per part.
   */
  const moveSelected = (folder: FolderId | null) =>
    runBulk(async (part) => {
      try {
        const outcome = await movePart(part.id, folder, true)
        if (outcome.kind === 'moved') return null
        return outcome.reason === 'duplicateName'
          ? strings.folders.moveRefused
          : refusalMessage(outcome.reason)
      } catch {
        return strings.folders.moveFailed
      }
    })
  const removeSelected = () =>
    runBulk(async (part) => {
      try {
        await removePart(part.id)
        return null
      } catch {
        return strings.removal.removeFailed
      }
    })
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
  }, [activeBatch, library, queryClient])

  const kind: BatchKind =
    mine?.kind ??
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
      setStarted({ id: accepted.batchId, kind, library })
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
      watch(accepted, 'upload')
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
    // And the counts beside the grid, which a scan changes as surely as it changes the grid.
    void queryClient.invalidateQueries({ queryKey: ['facets', library] })
  }, [settled, pagesLoaded, batchFinished, queryClient])

  const note = scanNow.isError
    ? strings.scan.startFailed
    : sweep.isError || renderPart.isError
      ? strings.render.queueFailed
      : sweep.data?.queued === 0
        ? strings.render.nothingMissing
        : null

  const look =
    looking === undefined ? null : (
      <QuickLook
        part={looking}
        from={openFrom}
        hostRoot={instance.data?.hostStorageRoot ?? null}
        busy={renderPart.isPending && renderPart.variables === looking.id}
        onRender={(id) => renderPart.mutate(id)}
        onMove={looking.directory === null ? null : () => setMoving(looking)}
        onClose={closeLook}
        pane={wide}
      />
    )

  return (
    <section className="flex items-start gap-6">
      {/*
        Rendered, not assigned. React 19 hoists a `<title>` into the head from wherever it
        is written and removes it again on unmount, so the route that owns the page owns
        its title and `index.html`'s static one stays as the pre-hydration fallback.
        SC 2.4.2, Level A — one title for the whole application titles none of its pages.
      */}
      <title>{strings.titles.library}</title>
      {/*
        The first tab stop on the page, and off-screen until it is one.

        SC 2.4.1, Level A. The category tree below is dozens of tab stops that repeat on
        every visit and it comes first in the source order, so without this the keyboard
        route to the first part runs through every category in the library.

        Moved by `translate` rather than hidden: `display: none` and `visibility: hidden`
        both remove it from the tab order, which is the one thing it must stay in. The
        target takes `tabIndex={-1}` because a `<div>` is not focusable, and a fragment
        link that moves the viewport without moving focus leaves a keyboard user exactly
        where they were.
      */}
      <a
        href="#parts"
        className="ease-mechanical fixed top-4 left-4 z-30 -translate-y-20 rounded-sm border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-2 text-sm duration-[var(--duration-fast)] focus:translate-y-0"
      >
        {strings.skipToParts}
      </a>
      {/*
        The tree and the grid are siblings, and the drag between them needs nothing
        shared: a card writes its identity into the drag payload and a category row
        reads it back on drop, so neither holds state for the other.
      */}
      <div className="w-56 shrink-0">
        <FormatFacet
          library={library}
          folderId={folderId}
          q={q}
          selected={format}
          onSelect={(value) => onSelectFormat?.(value)}
        />
        <FolderTree
          library={library}
          selected={folderId ?? null}
          onSelect={(folder) => onSelectFolder?.(folder)}
        />
      </div>
      <div id="parts" tabIndex={-1} className="min-w-0 flex-1">
        <Toolbar
          library={library}
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
          onSelectLibrary={onSelectLibrary}
          pageSize={pageSize}
          onPageSize={(size) => {
            setPageSizeState(size)
            setPageSize(library, size)
          }}
          density={density}
          onDensity={(next) => {
            setDensityState(next)
            setDensity(library, next)
          }}
          layout={layout}
          onLayout={(next) => {
            setLayoutState(next)
            setLayout(library, next)
          }}
          sort={sort}
          onSort={(next) => {
            setSortState(next)
            setSort(library, next)
          }}
          searching={q !== undefined}
          selecting={selecting}
          onSelecting={(on) => {
            setSelecting(on)
            setSelected(new Set())
            setAnchor(null)
            setBulk(null)
          }}
          onUpload={() => picker.current?.click()}
          uploadBusy={upload.isPending}
          search={
            <SearchBox
              q={q ?? ''}
              categoryName={selectedFolderName}
              filtered={folderId !== undefined}
              onSearch={onSearch}
              onWiden={() => onSelectFolder?.(null)}
            />
          }
        />
        <DropTarget onFiles={startUpload} busy={upload.isPending} progress={uploading} picker={picker} />
        {uploadNote === null ? null : (
          <p className="mb-4 text-sm text-[var(--color-muted)]">{uploadNote}</p>
        )}
        {activeBatch === undefined ? null : (
          <ScanProgress
            status={scan.data}
            isError={scan.isError}
            kind={kind}
            library={library}
            batch={activeBatch}
          />
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
            onClearSearch={() => onSearch?.('')}
          />
        ) : (
          <>
            {/*
              The scope line, which `v2` puts above the grid and this page had only below
              it. Two facts a person needs before they start scanning rather than after
              they finish: what they are looking at, and how much of it there is.

              `h2` because it names the region the grid fills, and the grid is a list under
              it — the page's `h1` is the application's name in the bar. The count is
              `.tabular`, so it is set in the mono face like every other figure here, and a
              number that changes as pages load does not shift the words beside it.
            */}
            <div className="mb-3 flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <h2 className="text-[15px] leading-none font-semibold text-[var(--color-bright)]">
                {selectedFolderName ?? strings.folders.root}
              </h2>
              <p className="tabular text-[10.5px] text-[var(--color-muted)]">
                {parts.hasNextPage
                  ? strings.parts.showingSoFar(loaded.length)
                  : strings.parts.showingAll(loaded.length)}
              </p>
            </div>
            {wide ? null : look}
            {moving === null ? null : (
              <MovePartDialog
                part={{ id: moving.id, name: moving.name }}
                library={moving.library}
                onClose={() => setMoving(null)}
              />
            )}
            {selecting ? (
              <SelectionBar
                count={selected.size}
                bulk={bulk}
                onMove={() => setPicking(true)}
                onRemove={() => void removeSelected()}
                onClear={() => setSelected(new Set())}
              />
            ) : null}
            {picking ? (
              <PickCategoryDialog
                title={strings.selection.moveTitle(selected.size)}
                library={library}
                onPick={(folder) => {
                  setPicking(false)
                  void moveSelected(folder)
                }}
                onClose={() => setPicking(false)}
              />
            ) : null}
            <Grid
              parts={loaded}
              onRender={(part) => renderPart.mutate(part)}
              busyPart={renderPart.isPending ? renderPart.variables : undefined}
              hostRoot={instance.data?.hostStorageRoot ?? null}
              density={density}
              layout={layout}
              selecting={selecting}
              selected={selected}
              onToggle={toggle}
              onSelectAll={() => setSelected(new Set(loaded.map((part) => part.id)))}
              onOpen={(card, from) => {
                setOpenFrom(from)
                setOpenPart(card.id)
              }}
            />
            <MorePages
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
      {/*
        The pane is the page's third column on a wide screen: not modal, so the grid beside it
        stays clickable and the next card swaps what it shows.
      */}
      {wide ? look : null}
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
  picker,
}: {
  onFiles: (picked: PickedFile[]) => void
  busy: boolean
  progress: UploadProgress | undefined
  /** The hidden folder input, shared with the toolbar's Upload button. */
  picker: RefObject<HTMLInputElement | null>
}) {
  const [depth, setDepth] = useState(0)
  const input = picker
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
      className={`ease-mechanical mb-4 rounded-[var(--radius-ctl)] border border-dashed px-4 py-2.5 text-center text-xs duration-[var(--duration-fast)] ${
        over
          ? 'border-[var(--color-accent)] bg-[var(--color-surface)]'
          : // The dashed rectangle *is* the affordance — there is no label, no icon and no
            // fill saying "drop here", only this line. SC 1.4.11 wants 3:1 for exactly that.
            'border-[var(--color-edge)]'
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
                className="underline hover:text-[var(--color-text)]"
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
 * Everything above the grid, in one row.
 *
 * `v2` puts navigation, search, view options and upload in a single bar. This used to be four
 * stacked rows — the action bar, the library switcher, the grid settings and the search field
 * — which put 387px of chrome between the page's top and its first render. The controls are
 * the same controls; what changed is that the ones you set once and leave (card size, page
 * size, which library, whether previews render) went into two menus, and the ones you touch
 * every visit (search, upload, switching to the removed list) stayed on the bar.
 *
 * **Two bars, not one, and that is a deliberate difference from the design.** The brand sits
 * in the root route's header and this row sits under it. Hoisting search into the root would
 * move its debounce and its `q` navigation out of the route that owns that search param, and
 * `index.test.tsx` drives this route's own `SearchBox` through a synthetic root — so the
 * refactor would rewrite a large share of that file to save one 38px row.
 *
 * Menus are native popovers (`styles.css` has why). Every control inside one stays in the
 * document while the menu is closed, which is what keeps a test that finds the auto-thumbnail
 * checkbox by role meaningful — and also what would let such a test pass if the menu could
 * never be opened at all, so `index.test.tsx` asserts the trigger is wired to its menu, and
 * the keyboard pass in the browser is the check that it opens.
 */
function Toolbar({
  autoThumbnail,
  onAutoThumbnail,
  settingsBusy,
  settingsNote,
  onScan,
  scanBusy,
  onSweep,
  sweepBusy,
  note,
  library,
  onSelectLibrary,
  pageSize,
  onPageSize,
  density,
  onDensity,
  layout,
  onLayout,
  sort,
  onSort,
  searching,
  selecting,
  onSelecting,
  onUpload,
  uploadBusy,
  search,
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
  library: LibraryId
  onSelectLibrary?: (library: LibraryId) => void
  pageSize: PageSize
  onPageSize: (size: PageSize) => void
  density: Density
  onDensity: (density: Density) => void
  layout: Layout
  onLayout: (layout: Layout) => void
  sort: Sort
  onSort: (sort: Sort) => void
  /** A search is running, and a search is in relevance order whatever `sort` says. */
  searching: boolean
  selecting: boolean
  onSelecting: (on: boolean) => void
  onUpload: () => void
  uploadBusy: boolean
  /** The search field, built by the route that owns its query. */
  search: ReactNode
}) {
  return (
    <>
      <div className="mb-3 flex flex-wrap items-center gap-2">
        {/*
          Grid is where you are, so it is marked and not linked: a link to `/` from `/` would
          drop the category and the search you are in the middle of. Removed is a place, not
          an action, and the only route back to a part somebody removed — so it stays a link,
          carrying the library, as it was.
        */}
        <nav
          aria-label={strings.toolbar.views}
          className="flex flex-none items-center gap-0.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-raised)] p-[3px]"
        >
          <span
            aria-current="page"
            className="flex min-h-6 items-center rounded-[5px] bg-[var(--color-surface)] px-3 text-xs font-semibold text-[var(--color-bright)]"
          >
            {strings.toolbar.grid}
          </span>
          <Link
            to="/removed"
            search={library === DEFAULT_LIBRARY_ID ? undefined : { library }}
            className="ease-mechanical flex min-h-6 items-center rounded-[5px] px-3 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
          >
            {strings.removal.removedTitle}
          </Link>
        </nav>
        {search}
        <button
          type="button"
          aria-pressed={selecting}
          onClick={() => onSelecting(!selecting)}
          className="ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] aria-pressed:border-[var(--color-accent)] aria-pressed:text-[var(--color-bright)]"
        >
          {strings.selection.toggle}
        </button>
        <Menu id="view-menu" label={strings.toolbar.view}>
          <p className="text-xs font-medium text-[var(--color-muted)]">{strings.toolbar.layout}</p>
          <div role="group" aria-label={strings.toolbar.layout} className="flex gap-1.5">
            {LAYOUTS.map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={layout === option}
                onClick={() => onLayout(option)}
                className={
                  layout === option
                    ? 'min-h-6 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-accent)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-bright)]'
                    : 'ease-mechanical min-h-6 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'
                }
              >
                {LAYOUT_LABEL[option]}
              </button>
            ))}
          </div>
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.sort}
            <select
              value={sort}
              disabled={searching}
              aria-describedby={searching ? 'sort-while-searching' : undefined}
              onChange={(event) => onSort(event.target.value as Sort)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 disabled:opacity-60"
            >
              {SORTS.map((option) => (
                <option key={option} value={option}>
                  {strings.grid.sortOption[option]}
                </option>
              ))}
            </select>
          </label>
          {searching && (
            <p id="sort-while-searching" className="text-xs text-[var(--color-muted)]">
              {strings.grid.sortWhileSearching}
            </p>
          )}
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.density}
            <select
              value={density}
              onChange={(event) => onDensity(event.target.value as Density)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
            >
              {DENSITIES.map((option) => (
                <option key={option} value={option}>
                  {DENSITY_LABEL[option]}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center justify-between gap-3 text-xs text-[var(--color-muted)]">
            {strings.grid.pageSize}
            <select
              value={pageSize}
              onChange={(event) => onPageSize(Number(event.target.value) as PageSize)}
              className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
            >
              {PAGE_SIZES.map((size) => (
                <option key={size} value={size}>
                  {strings.grid.pageSizeOption(size)}
                </option>
              ))}
            </select>
          </label>
        </Menu>
        <Menu id="library-menu" label={strings.toolbar.library}>
          <LibrarySwitcher library={library} onSelect={onSelectLibrary} />
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
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.scan.start}
          </button>
          <button
            type="button"
            onClick={onSweep}
            disabled={sweepBusy}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {strings.render.sweep}
          </button>
        </Menu>
        <button
          type="button"
          onClick={onUpload}
          disabled={uploadBusy}
          className="ease-mechanical flex min-h-6 flex-none items-center rounded-[var(--radius-ctl)] border border-[var(--color-accent)] bg-[var(--color-surface)] px-3 py-1.5 text-xs font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
        >
          {strings.toolbar.upload}
        </button>
      </div>
      {/*
        Outside the menus, always. A note that says the scan failed or that previews could not
        be queued answers a control that may live in a closed menu, and a message inside a
        closed menu is a message nobody reads.
      */}
      {settingsNote === null && note === null ? null : (
        <p className="mb-3 flex flex-wrap gap-x-4 text-sm text-[var(--color-muted)]">
          {settingsNote === null ? null : <span>{settingsNote}</span>}
          {note === null ? null : <span>{note}</span>}
        </p>
      )}
    </>
  )
}

/** A button and the popover it opens. See `styles.css` for why it is native. */
function Menu({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  // Each menu names its own anchor, so two menus on one page never position against the
  // same button.
  const anchor = { '--anchor': `--${id}` } as CSSProperties
  return (
    <>
      <button
        type="button"
        popoverTarget={id}
        style={anchor}
        className="menu-anchor ease-mechanical flex min-h-6 flex-none items-center gap-1.5 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-xs duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {label}
        <span aria-hidden="true" className="text-[9px] opacity-75">
          ▾
        </span>
      </button>
      <div
        id={id}
        popover="auto"
        role="group"
        aria-label={label}
        style={anchor}
        /*
          `open:flex`, never `flex`. A `display` utility on the popover itself outranks the
          browser's own `[popover]:not(:popover-open) { display: none }` — author styles beat
          user-agent ones — so a plain `flex` here painted both menus permanently open over
          the grid while every test stayed green: jsdom applies none of these classes, so the
          suite could not see it. The first browser check did.
        */
        className="menu panel-in w-64 flex-col gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[var(--color-text)] shadow-[0_14px_34px_rgba(0,0,0,0.5)] open:flex"
      >
        {children}
      </div>
    </>
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
  library,
  batch,
}: {
  status?: BatchStatus
  isError: boolean
  kind: BatchKind
  library: LibraryId
  batch: BatchId
}) {
  const queryClient = useQueryClient()
  // Failures past the hundred the status carries, fetched only when somebody asks for them.
  const [more, setMore] = useState<JobFailure[]>([])
  const showMore = useMutation({
    mutationFn: (after: JobId) => fetchFailures(library, batch, after),
    onSuccess: (page) => setMore((held) => [...held, ...page.failed]),
  })
  // The server reopens the batch, so reading its status again restarts the poll, which stops
  // only on a finished batch. The pages fetched past the sample are dropped rather than
  // patched: the retried rows are no longer failures, and the status re-lists what still is.
  const retry = useMutation({
    mutationFn: (job: JobId | undefined) => retryFailed(library, batch, job),
    onSuccess: () => {
      setMore([])
      void queryClient.invalidateQueries({ queryKey: ['batch', library, batch] })
    },
  })
  // Picked once, out here: a `kind === 'render' ? … : …` inside JSX puts the discriminator
  // itself in a child expression, where `no-bare-strings.test.ts` reads it — correctly —
  // as a bare literal reaching the screen.
  //
  // Every kind, and `migrate` is not a fall-through to `scan`'s copy. A failed
  // migration used to render "3 files could not be read. They will not appear in the grid"
  // about models that already exist and are already in the grid, and a status poll that
  // failed rendered "No scan with that id has run in this library" to an operator who
  // never started a scan. A non-destructive failure worded as data loss is the one class of
  // mistake this product treats as a correctness bug.
  const copy =
    kind === 'render'
      ? strings.render
      : kind === 'migrate'
        ? strings.migrate
        : kind === 'upload'
          ? // Scan's per-file failure line, which never says "scan", and an unknown line of its
            // own: `scan.unknown` tells somebody who never started one about a scan.
            { failed: strings.scan.failed, unknown: strings.upload.batchUnknown }
          : strings.scan
  if (isError) {
    return <p className="mb-4 max-w-prose text-[var(--color-muted)]">{copy.unknown}</p>
  }
  if (status === undefined) {
    return null
  }
  // The server caps the list at 100 while `failedTotal` is the real number, so a batch
  // with more failures than that says so rather than trailing off at the hundredth — and the
  // count is a button, so the rest is a press away.
  const listed = [...status.failed, ...more]
  const hidden = status.failedTotal - listed.length
  const last = listed[listed.length - 1]
  // A migration retries itself (see `PgJobs::retry`), so its line offers nothing to press.
  const retryable = kind !== 'migrate'
  const button =
    'ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-60'
  return (
    <div className="mb-4 max-w-prose text-[var(--color-muted)]">
      <p className="flex flex-wrap items-center gap-2">
        <span>{progressText(status, kind)}</span>
        {status.failedTotal === 0 ? null : <span>{copy.failed(status.failedTotal)}</span>}
        {retryable && status.failedTotal > 1 ? (
          <button
            type="button"
            className={button}
            disabled={retry.isPending}
            onClick={() => retry.mutate(undefined)}
          >
            {strings.failure.retryAll(status.failedTotal)}
          </button>
        ) : null}
      </p>
      {retry.isError ? (
        <p role="alert" className="mt-2 text-sm">
          {strings.failure.retryFailed}
        </p>
      ) : null}
      {listed.length === 0 ? null : (
        <ul role="list" className="mt-2 space-y-1 text-sm">
          {listed.map((failure) => (
            // The job, which is the one thing unique to a row: two jobs can name the same
            // file, and a `scan_directory` failure has no path at all.
            <li key={failure.job} className="flex flex-wrap items-baseline gap-2">
              <span>{strings.failure.line(failure.path, failure.reason)}</span>
              {retryable ? (
                <button
                  type="button"
                  className={button}
                  aria-label={strings.failure.retryOne(failure.path)}
                  disabled={retry.isPending}
                  onClick={() => retry.mutate(failure.job)}
                >
                  {strings.failure.retry}
                </button>
              ) : null}
            </li>
          ))}
          {hidden <= 0 || last === undefined ? null : (
            <li>
              <button
                type="button"
                className={button}
                disabled={showMore.isPending}
                onClick={() => showMore.mutate(last.job)}
              >
                {strings.failure.more(hidden)}
              </button>
            </li>
          )}
        </ul>
      )}
      {showMore.isError ? (
        <p role="alert" className="mt-2 text-sm">
          {strings.failure.moreFailed}
        </p>
      ) : null}
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
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1"
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
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
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
          className="mt-3 w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
        />
        <label className="mt-3 flex flex-col gap-1 text-xs text-[var(--color-muted)]">
          {strings.libraries.modeLabel}
          <select
            value={mode}
            onChange={(event) => setMode(event.target.value as NewLibrary['mode'])}
            className="rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm"
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
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.folders.cancel}
          </button>
          <button
            type="submit"
            disabled={busy || trimmed === ''}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
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
const LAYOUT_LABEL: Record<Layout, string> = {
  detail: strings.layouts.detail,
  gallery: strings.layouts.gallery,
  list: strings.layouts.list,
}

const DENSITY_LABEL: Record<Density, string> = {
  comfortable: strings.grid.comfortable,
  compact: strings.grid.compact,
}

/**
 * The key that moves focus into search from anywhere on the grid, shown inside the field and
 * declared as its `aria-keyshortcuts`. `/` because the tools this audience already lives in
 * spend it on search, so it is a key people try before they read anything.
 *
 * A constant here and not a `strings.ts` entry: it is a key, not copy — nothing translates it,
 * and `KeyboardEvent.key` reports `/` on a Turkish layout too, where it sits on Shift+7.
 */
const SEARCH_SHORTCUT = '/'

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

  // `/` from anywhere on the grid. Not while a person is typing into a field — a slash is
  // half of every file path — and not under a modal dialog, whose own keys own the page.
  const field = useRef<HTMLInputElement>(null)
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== SEARCH_SHORTCUT || event.defaultPrevented) return
      if (event.ctrlKey || event.metaKey || event.altKey) return
      const target = event.target
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest('input, textarea, select') !== null)
      ) {
        return
      }
      if (document.querySelector('[aria-modal="true"]') !== null) return
      event.preventDefault()
      field.current?.focus()
      field.current?.select()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  const waiting = typed.trim().length === 1
  return (
    <div className="flex min-w-64 flex-1 flex-wrap items-center gap-2">
      {/*
        The field sits *below* the ground rather than on it — `--color-raised` against the
        page, which is how `v2` draws every input. A control you type into reads as a well;
        one you press reads as a surface.
      */}
      <div className="relative flex min-w-64 flex-1 items-center">
        <span
          aria-hidden="true"
          className="pointer-events-none absolute left-[11px] text-sm text-[var(--color-muted)]"
        >
          ⌕
        </span>
        <input
          ref={field}
          type="search"
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            // Back to the grid, keeping the query. Chrome's own Escape clears a search field,
            // which would throw away what was typed on the way out.
            event.preventDefault()
            document.getElementById('parts')?.focus()
          }}
          aria-label={strings.search.label}
          aria-keyshortcuts={SEARCH_SHORTCUT}
          placeholder={strings.search.placeholder}
          className="peer w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] py-2 pr-9 pl-[30px] text-[13px] focus:border-[var(--color-accent)]"
        />
        {/*
          The key, shown where it is used. Only while the field is empty and unfocused: once
          you are in it the hint has done its job, and over typed text it would be noise.
        */}
        {typed === '' ? (
          <kbd
            aria-hidden="true"
            className="ease-mechanical pointer-events-none absolute right-2.5 rounded border border-[var(--color-edge)] px-1.5 font-mono text-[11px] leading-4 text-[var(--color-muted)] duration-[var(--duration-fast)] peer-focus:opacity-0"
          >
            {SEARCH_SHORTCUT}
          </kbd>
        ) : null}
      </div>
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
          className="ease-mechanical rounded-full border border-[var(--color-accent)] bg-[var(--color-surface)] px-3 py-1 text-xs text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px"
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
  onClearSearch,
}: {
  filtered: boolean
  categoryName: string | null
  /** The query that found nothing, or `null` when nobody searched. */
  query: string | null
  onWiden: () => void
  /** Drops the query and keeps the category, for the case the sidebar was not the reason. */
  onClearSearch: () => void
}) {
  // A search that found nothing is not an empty library, and saying so is worse than
  // useless: the user did not empty anything, they typed something, and "drop a folder of
  // models above to add them" is an instruction for a problem they do not have.
  if (query !== null) {
    return (
      <div className="max-w-prose">
        <h2 className="text-lg">
          {filtered ? strings.emptyLibrary.categoryTitle : strings.search.noMatchesTitle}
        </h2>
        <p className="mt-2 text-[var(--color-muted)]">
          {filtered
            ? strings.search.noMatchesInCategory(
                query,
                categoryName ?? strings.search.inThisCategory,
              )
            : strings.search.noMatches(query)}
        </p>
        <p className="mt-2 text-xs text-[var(--color-muted)]">{strings.search.scope}</p>
        {/*
          Both cases get a way out. The narrowed one widens; the unnarrowed one clears —
          which used to be nothing at all, a dead end on the one screen this product's
          primary user reaches by doing exactly what the product is for.
        */}
        <button
          type="button"
          onClick={filtered ? onWiden : onClearSearch}
          className="ease-mechanical mt-3 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {filtered ? strings.search.widen : strings.search.clear}
        </button>
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
  hasMore,
  fetching,
  onMore,
}: {
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
      {/*
        The count moved to the scope line above the grid, where `v2` puts it and where it is
        legible before the scanning starts rather than after it. What is left here is the
        control, and only when there is another page — a sentence saying the library is all
        on screen is what the header now says by naming the whole count.
      */}
      {!hasMore ? null : (
        <p className="mt-4 max-w-prose text-xs text-[var(--color-muted)]">
          <button
            type="button"
            onClick={onMore}
            disabled={fetching}
            className="underline underline-offset-2 disabled:opacity-50"
          >
            {fetching ? strings.parts.loadingMore : strings.parts.loadMore}
          </button>
        </p>
      )}
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
          className="ease-mechanical mt-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px"
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
  layout,
  selecting,
  selected,
  onToggle,
  onSelectAll,
  onOpen,
}: {
  parts: readonly PartCard[]
  onRender: (part: PartId) => void
  busyPart?: PartId
  /** Passed down rather than fetched per card: it is one fact about the deployment. */
  hostRoot: string | null
  density: Density
  layout: Layout
  selecting: boolean
  selected: ReadonlySet<PartId>
  /** `range` is a shift-click: everything from the last part toggled to this one. */
  onToggle: (part: PartId, range: boolean) => void
  onSelectAll: () => void
  /** A card asks to be looked at, from where its render sits. */
  onOpen: (part: PartCard, from: DOMRect) => void
}) {
  // Two numbers move together and have to: the column width sets how tall a card ends up,
  // and `contain-intrinsic-size` is the placeholder height for one that has not rendered.
  // Give the compact grid the comfortable card's height and the scrollbar jumps as cards
  // enter and leave — which is what makes `content-visibility` look broken.
  //
  // Whole class strings rather than interpolation: Tailwind scans source for literals, and
  // a class built at runtime is a class that was never generated.
  // A list is one column whatever the density: density sizes a card, and a row has no card
  // to size.
  const columns =
    layout === 'list'
      ? 'grid-cols-1 gap-1.5'
      : density === 'compact'
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
  //
  // Gallery and list, measured in Chrome over the seeded library: a comfortable gallery card
  // rendered 12.89rem tall and a compact one 8.47rem, at a 1157px viewport — square, because a
  // gallery card is its well and nothing else, so these two track column width and are the
  // figures most likely to drift at other widths, which the leading `auto` absorbs after first
  // render. A list row is 4rem at either density, exactly: its height is the 46px well plus
  // padding and nothing about it scales. The first draft of these was guessed at 11rem and
  // 8rem, and was marked as a guess until this replaced it.
  const intrinsic =
    layout === 'list'
      ? '[contain-intrinsic-size:auto_4rem]'
      : layout === 'gallery'
        ? density === 'compact'
          ? '[contain-intrinsic-size:auto_8.5rem]'
          : '[contain-intrinsic-size:auto_13rem]'
        : density === 'compact'
          ? '[contain-intrinsic-size:auto_31rem]'
          : '[contain-intrinsic-size:auto_26rem]'
  return (
    <ul
      role="list"
      className={`grid list-none ${columns}`}
      // Ctrl/Cmd-A inside the grid selects every part loaded, while selecting. Anywhere else
      // it is still the browser's own select-all.
      onKeyDown={(event) => {
        if (selecting && selectsAll(event)) {
          event.preventDefault()
          onSelectAll()
        }
      }}
    >
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
          <Card
            part={part}
            onRender={onRender}
            busy={part.id === busyPart}
            hostRoot={hostRoot}
            layout={layout}
            selecting={selecting}
            selected={selected.has(part.id)}
            onToggle={onToggle}
            onOpen={onOpen}
          />
        </li>
      ))}
    </ul>
  )
}

/**
 * The origin for a tile with no render yet. `flipFrom` declines on a zero-area rectangle, so
 * a part still waiting on the worker opens its panel without a flight rather than bursting
 * out of a point.
 */
const DEFAULT_ORIGIN = new DOMRect(0, 0, 0, 0)

/** Where the quick look sits beside the grid rather than over it: 1280px and up. */
const PANE_QUERY = '(min-width: 80rem)'

/**
 * Whether the screen is wide enough for the pane. `false` where `matchMedia` does not exist —
 * jsdom, which is where every test that expects the dialog runs.
 */
function useWide(): boolean {
  const [wide, setWide] = useState(
    () => typeof window.matchMedia === 'function' && window.matchMedia(PANE_QUERY).matches,
  )
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const query = window.matchMedia(PANE_QUERY)
    const onChange = () => setWide(query.matches)
    query.addEventListener?.('change', onChange)
    return () => query.removeEventListener?.('change', onChange)
  }, [])
  return wide
}

/**
 * The quick look beside the grid.
 *
 * Not modal, so it traps nothing: the grid stays reachable and the next card swaps what this
 * shows. Opening moves focus to the heading, so a screen reader announces the part and Tab
 * carries on into its details. Escape closes it — but not out from under a dialog opened on
 * top of it, whose own Escape comes first.
 */
function QuickLookPane({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  const titleId = useId()
  const heading = useRef<HTMLHeadingElement>(null)
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })
  useEffect(() => {
    heading.current?.focus()
  }, [title])
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      if (document.querySelector('[aria-modal="true"]') !== null) return
      close.current()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])
  return (
    <aside
      aria-labelledby={titleId}
      className="panel-in sticky top-4 max-h-[calc(100vh-2rem)] w-[26rem] shrink-0 overflow-y-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
    >
      <div className="flex items-start justify-between gap-4">
        <h2 id={titleId} ref={heading} tabIndex={-1} className="text-sm font-medium outline-none">
          {title}
        </h2>
        <button
          type="button"
          onClick={onClose}
          aria-label={strings.dialog.close}
          className="ease-mechanical -m-1 flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="none"
               stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
      {children}
    </aside>
  )
}

/** How many parts a bulk action changes at once. See `eachAtMost`. */
const BULK_CONCURRENCY = 4

type BulkProgress = {
  done: number
  total: number
  failures: { id: PartId; name: string; reason: string }[]
}

function selectsAll(event: { key: string; ctrlKey: boolean; metaKey: boolean }): boolean {
  return (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a'
}

/**
 * The count, the two actions, and afterwards the parts an action did not change.
 *
 * Move and Remove only: purge stays on the Removed page, one part at a time, because it is the
 * one thing in the product that cannot be undone.
 */
function SelectionBar({
  count,
  bulk,
  onMove,
  onRemove,
  onClear,
}: {
  count: number
  bulk: BulkProgress | null
  onMove: () => void
  onRemove: () => void
  onClear: () => void
}) {
  const busy = bulk !== null && bulk.done < bulk.total
  const button =
    'ease-mechanical min-h-6 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-60'
  return (
    <section
      aria-label={strings.selection.bar}
      className="mb-3 flex flex-col gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span aria-live="polite" className="tabular mr-2">
          {bulk !== null && bulk.done < bulk.total
            ? strings.selection.working(bulk.done, bulk.total)
            : strings.selection.count(count)}
        </span>
        <button type="button" className={button} disabled={count === 0 || busy} onClick={onMove}>
          {strings.folders.moveTo}
        </button>
        <button
          type="button"
          className={button}
          disabled={count === 0 || busy}
          onClick={onRemove}
          title={strings.removal.removeHint}
        >
          {strings.removal.remove}
        </button>
        <button type="button" className={button} disabled={count === 0 || busy} onClick={onClear}>
          {strings.selection.clear}
        </button>
      </div>
      {bulk === null || busy || bulk.failures.length === 0 ? null : (
        <div role="alert">
          <p>{strings.selection.failedHeading(bulk.failures.length)}</p>
          <ul role="list" className="mt-1 space-y-1 text-[var(--color-muted)]">
            {bulk.failures.map((failure) => (
              <li key={failure.id}>{strings.selection.failure(failure.name, failure.reason)}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}

/**
 * A card's own box, its render's well, and its text, in each layout.
 *
 * Whole class strings, never assembled: Tailwind generates what it finds in source text, and
 * a class built at runtime is a class that was never generated.
 *
 * **Every layout keeps `Measurements` visible**, and that is the constraint the gallery is
 * built around rather than an afterthought. `v2`'s gallery overlay shows a name and three
 * dimensions and nothing else; this one carries the approximate label as well, because
 * `CLAUDE.md` makes that label unconditional and an overlay is exactly where it would drop out.
 */
const CARD_SHAPE: Record<Layout, string> = {
  detail:
    'ease-mechanical group relative flex h-full cursor-pointer flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-0.5 hover:border-[var(--color-edge)] hover:shadow-[0_12px_26px_rgba(0,0,0,0.45)]',
  gallery:
    'ease-mechanical group relative flex h-full cursor-pointer flex-col overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-0.5 hover:border-[var(--color-edge)] hover:shadow-[0_12px_26px_rgba(0,0,0,0.45)]',
  // No lift on a row. Forty rows each rising under a passing pointer is a list that shimmers;
  // the edge brightening is enough to say which one is addressed.
  list: 'ease-mechanical group relative flex cursor-pointer items-center gap-3 overflow-hidden rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-surface)] p-2 duration-[var(--duration-fast)] hover:border-[var(--color-edge)]',
}

const WELL: Record<Layout, string> = {
  detail:
    'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)] p-[7%]',
  gallery:
    'relative flex aspect-square items-center justify-center overflow-hidden bg-[var(--color-raised)] p-[7%]',
  list: 'relative flex size-[46px] flex-none items-center justify-center overflow-hidden rounded-[var(--radius-ctl)] bg-[var(--color-raised)] p-1',
}

/**
 * The name, per layout. The gallery clamps it to one line, and that is the fix for the first
 * gallery this shipped: a two-line name above the figures made the overlay two-thirds of a
 * square card, which put the name over the middle of the render — the critique's P0, "a tile
 * that hides the part", back again — and set white text over light grey facets. Every test
 * and a measured `checkVisibility` on the approximate label passed it, because none of them
 * asks whether the *part* can be seen. A screenshot at two widths did. The full name stays in
 * the link's text for every reader and in its `title` for a pointer.
 */
const NAME: Record<Layout, string> = {
  detail: 'text-sm leading-snug font-semibold',
  gallery: 'truncate text-[13px] leading-snug font-semibold text-[var(--color-bright)]',
  list: 'text-sm leading-snug font-semibold',
}

const FOOTER: Record<Layout, string> = {
  detail: 'flex flex-1 flex-col gap-1 p-3',
  // Clear at the top so the render reads through, and dark enough under the text for the
  // palette's contrast to hold over any render. `v2`'s own stops (0.88 at 48%) did not: over the
  // brightest face `raster.rs` can draw, the name's top row measured 4.1:1 and the triangle
  // count 3.9:1. The name needs alpha 0.53 there and muted text 0.91; these stops give 0.63 and
  // 0.93. The stop is in rem so it moves with `pt-6` rather than with the overlay's height.
  // `contrast.test.ts` cannot see text over a picture — re-measure from a screenshot if the
  // stops, the padding or the text sizes here change.
  gallery:
    'absolute inset-x-0 bottom-0 flex flex-col gap-0.5 bg-[linear-gradient(180deg,rgba(18,18,20,0)_0%,rgba(18,18,20,0.93)_2.5rem,rgba(18,18,20,0.97)_100%)] px-3 pt-6 pb-2.5',
  list: 'flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-4 gap-y-1',
}

function Card({
  part,
  onRender,
  busy,
  hostRoot,
  layout,
  selecting,
  selected,
  onToggle,
  onOpen,
}: {
  part: PartCard
  onRender: (part: PartId) => void
  busy: boolean
  hostRoot: string | null
  layout: Layout
  selecting: boolean
  selected: boolean
  onToggle: (part: PartId, range: boolean) => void
  onOpen: (part: PartCard, from: DOMRect) => void
}) {
  const nameId = `part-name-${part.id}`
  const directory = part.directory
  // A model still in the shared store has no directory to rename, and the move route
  // refuses it. The card withholds the move rather than letting the user discover that
  // from a `409` — the same status the route uses for a name collision, which the UI
  // would otherwise present as one.
  const movable = directory !== null
  // Only where the gallery clamps the name to one line; elsewhere the whole name is on the
  // card and a tooltip repeating it is noise. Decided here rather than in the attribute,
  // because a literal inside a user-visible attribute reads to the bare-strings gate as copy.
  const clampedName = layout === 'gallery' ? part.name : undefined
  return (
    <article
      aria-labelledby={nameId}
      /*
        The whole card opens the panel. It stays a handler rather than an anchor because the
        name inside it is itself a link, and an anchor inside an anchor is invalid HTML that
        browsers resolve by guessing — the click is filtered instead of the markup reshaped.

        The filter still asks `closest`, because the name is the one control left in here and
        a click on it belongs to it: the name is the keyboard path, the middle-click path,
        and what a screen reader announces for the card.
      */
      onClick={(event) => {
        if (!(event.target instanceof Element)) return
        if (event.target.closest('a, button, input')) return
        // While selecting, the card is the checkbox's larger target, and the panel waits.
        if (selecting) {
          onToggle(part.id, event.shiftKey)
          return
        }
        // Measured here rather than in the panel, because by the time the panel exists this
        // tile may have been scrolled, re-laid-out by a density change, or replaced by the
        // next page. Where the render *was* when it was clicked is the only honest origin.
        const render = event.currentTarget.querySelector('img')
        onOpen(part, render === null ? DEFAULT_ORIGIN : render.getBoundingClientRect())
      }}
      draggable={movable}
      onDragStart={(event) =>
        event.dataTransfer.setData(
          PART_DRAG_TYPE,
          partDragPayload({ id: part.id, name: part.name }),
        )
      }
      /*
        **A border, which reverses what stood here.** The old note argued that the render is
        its own edge and a box around a picture is a second frame competing with the first.
        That held while the render went edge to edge; `v2` insets it instead, so the card's
        own ground is visible all the way round and the tile has no edge of its own left.
        A hairline is what puts one back — and it is the thing that lifts on hover, which is
        how a pointer says which tile it is on without moving the picture.

        `--color-border` at rest and `--color-edge` under the pointer: the card is a control
        and 1.4.11 wants 3:1 on the boundary that identifies one, but only while it is the
        one being addressed. A wall of forty tiles all drawn at 3:1 is a grid of boxes
        rather than a page of parts.
      */
      className={selected ? `${CARD_SHAPE[layout]} outline-2 outline-[var(--color-accent)]` : CARD_SHAPE[layout]}
    >
      {/*
        The well the render sits in, one step *down* from the card and inset from it.

        `v2` paints the thumbnail `center/86%` on `#17171b` rather than filling the tile:
        the render floats with air around it, which is what makes a wall of parts read as
        objects on shelves instead of as a mosaic. `p-[7%]` is the same 86% from the other
        side, in the one unit that keeps it proportional as the density control changes the
        column width.
      */}
      {/*
        Rendered only while selecting, never rendered and hidden: with selection off the card's
        name is its one tab stop, and a hidden checkbox would still be a second.
      */}
      {selecting ? (
        <input
          type="checkbox"
          checked={selected}
          aria-label={strings.selection.selectPart(part.name)}
          onChange={() => undefined}
          onClick={(event) => onToggle(part.id, event.shiftKey)}
          className="absolute top-2 left-2 z-10 size-4 accent-[var(--color-accent)]"
        />
      ) : null}
      <div className={WELL[layout]}>
        {part.thumbnail === null ? (
          // Never an <img> with an empty src: a broken-image glyph reads as a failure,
          // and "the worker has not rasterized this yet" is not one.
          // A 46px list well cannot hold the sentence, and the row's name already says which
          // part this is — so there it is read, not drawn.
          <span className={layout === 'list' ? 'sr-only' : 'text-xs text-[var(--color-muted)]'}>
            {strings.parts.noThumbnail}
          </span>
        ) : (
          <img
            src={part.thumbnail}
            alt={strings.parts.thumbnailAlt(part.name)}
            className="h-full w-full object-contain"
          />
        )}
      </div>

      {/*
        The footer, and the only thing besides the render that survives at rest. A tile a
        person is scanning has to answer "which part is this" without being hovered.
      */}
      <div className={FOOTER[layout]}>
        {/*
          The name is the link, not the whole card — the keyboard path, the middle-click
          path, and what a screen reader announces. It is the card's only tab stop, which is
          why reaching the fiftieth part costs fifty Tab presses rather than two hundred and
          sixty.

          That count is only defensible because the controls it replaced went somewhere a
          keyboard can reach. They live in this panel *and* on the part's own page, which
          this link goes to — for a while they were in the panel alone, and the panel opens
          on a click of a tile that has no key handler, so Render, Move and the storage path
          existed nowhere a keyboard could get to. WCAG 2.2 SC 2.1.1 is Level A and it asks
          whether a function is available, not which surface offers it. Do not move a
          control out of `parts.$partId.tsx` without checking this again.
        */}
        <div className="flex min-w-0 flex-col gap-1">
          <h2 id={nameId} className={NAME[layout]}>
            <Link
              to="/parts/$partId"
              params={{ partId: part.id }}
              title={clampedName}
              className="ease-mechanical duration-[var(--duration-fast)] hover:underline"
            >
              {part.name}
            </Link>
          </h2>
          {part.partNumber === null ? null : (
            <p className="tabular font-mono text-xs text-[var(--color-muted)]">{part.partNumber}</p>
          )}
        </div>
        {/*
          `CLAUDE.md` says a mesh-derived measurement is labelled approximate *always*. It
          used to sit in the hover panel, where "always" quietly meant "never" — the row was
          clipped off the top of the tile at every desktop width. Always means here.
        */}
        <Measurements part={part} tight={layout === 'gallery'} />
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
function QuickLook({
  part,
  from,
  hostRoot,
  busy,
  onRender,
  onMove,
  onClose,
  pane,
}: {
  part: PartCard
  /** Where this part's render sat on the grid when it was clicked. */
  from: DOMRect
  hostRoot: string | null
  busy: boolean
  onRender: (id: PartId) => void
  /** `null` for a model still in the shared store, which has no directory to rename. */
  onMove: (() => void) | null
  onClose: () => void
  /** Beside the grid on a wide screen; over it, in a dialog, otherwise. */
  pane: boolean
}) {
  const detail = useQuery({
    queryKey: ['part', part.id],
    queryFn: () => fetchPartDetail(part.id),
  })
  const panel = useRef<HTMLDivElement>(null)
  /*
    The authored moment, and the only one in the application.

    `useLayoutEffect` and not `useEffect`: the render has to be measured and moved in the
    same frame it is painted, or it lands at its destination first and then jumps back to
    begin the flight. Keyed on the detail arriving, because the image does not exist until
    then — the panel is open and empty for as long as the fetch takes.
  */
  useLayoutEffect(() => {
    const image = panel.current?.querySelector('img')
    if (image != null) flipFrom(image, from)
  }, [detail.data, from])
  const Frame = pane ? QuickLookPane : Dialog
  return (
    <Frame title={part.name} onClose={onClose}>
      <div ref={panel}>
      {detail.isPending ? (
        <p className="mt-2 text-sm text-[var(--color-muted)]">{strings.quickLook.loading}</p>
      ) : detail.isError ? (
        <p className="mt-2 max-w-prose text-sm text-[var(--color-muted)]">
          {strings.quickLook.failed}
        </p>
      ) : (
        <Detail
          titled
          part={detail.data}
          /*
            The tools the card used to carry. They are here because the card is a picture and
            a name now: a control that hides the render it sits on is a control fighting the
            one job this product has. Owner decision, 2026-09-08 — click gives you the
            essential information and the tools; the full page gives you depth.
          */
          actions={
            <>
              <button
                type="button"
                onClick={() => onRender(part.id)}
                disabled={busy}
                aria-label={strings.render.partFor(part.name)}
                className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
              >
                {strings.render.part}
              </button>
              {onMove === null ? (
                <span className="text-xs text-[var(--color-muted)]">
                  {strings.folders.notMigrated}
                </span>
              ) : (
                <button
                  type="button"
                  onClick={onMove}
                  aria-label={strings.folders.moveToFor(part.name)}
                  className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:-translate-y-px"
                >
                  {strings.folders.moveTo}
                </button>
              )}
            </>
          }
        />
      )}
      </div>
      <ShowInFolder part={part} hostRoot={hostRoot} />
      <div className="mt-4 flex justify-end gap-2">
        <Link
          to="/parts/$partId"
          params={{ partId: part.id }}
          className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {strings.quickLook.fullPage}
        </Link>
      </div>
    </Frame>
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
function Measurements({ part, tight = false }: { part: PartCard; tight?: boolean }) {
  // Narrowed with typeof rather than compared to null: the binding says `number | null`,
  // but the response is cast rather than validated, so a field that disappears upstream
  // arrives here as undefined and would reach .toLocaleString() as one.
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  if (!part.approximate && count === null) {
    return null
  }
  return (
    <p
      // Without the top padding over a gallery render: the overlay is the lower third of the
      // card, and eight pixels of it spent on air is eight pixels more of the part hidden.
      className={
        tight
          ? 'tabular mt-auto flex flex-wrap items-center gap-2 text-xs text-[var(--color-muted)]'
          : 'tabular mt-auto flex flex-wrap items-center gap-2 pt-2 text-xs text-[var(--color-muted)]'
      }
    >
      {count === null ? null : <span>{strings.parts.triangles(count)}</span>}
      <span
        title={strings.parts.approximateDetail}
        className="rounded border border-[var(--color-border)] px-1.5 py-0.5 text-xs tracking-wider uppercase"
      >
        {strings.parts.approximate}
      </span>
    </p>
  )
}

/**
 * The formats among the parts the grid shows for the same category and query, as buttons that
 * narrow the grid to one.
 *
 * The counts ignore the chosen format, so every other format stays choosable. Past the server's
 * exact-count threshold a value arrives with no count, and is shown without one rather than with
 * a guess. A chosen format the current query no longer matches stays on the list at zero, so the
 * choice can still be undone from where it was made.
 */
function FormatFacet({
  library,
  folderId,
  q,
  selected,
  onSelect,
}: {
  library: LibraryId
  folderId?: string
  q?: string
  selected?: string
  onSelect: (format: string | null) => void
}) {
  const facets = useQuery({
    queryKey: ['facets', library, folderId ?? null, q ?? null],
    queryFn: () => fetchFacets(library, folderId, q),
  })
  if (facets.isError) {
    return (
      <p role="alert" className="mb-6 text-xs text-[var(--color-muted)]">
        {strings.facets.failed}
      </p>
    )
  }
  const formats = facets.data?.formats ?? []
  const shown =
    selected === undefined || formats.some(({ value }) => value === selected)
      ? formats
      : [...formats, { value: selected, count: 0 }]
  if (facets.data === undefined || shown.length === 0) return null
  return (
    <section aria-labelledby="format-facet" className="mb-6">
      <h2
        id="format-facet"
        className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase"
      >
        {strings.facets.format}
      </h2>
      <ul role="list" className="space-y-0.5">
        {shown.map(({ value, count }) => (
          <li key={value}>
            <button
              type="button"
              aria-pressed={selected === value}
              aria-label={strings.facets.option(value, count)}
              onClick={() => onSelect(selected === value ? null : value)}
              className="ease-mechanical flex min-h-6 w-full items-center justify-between gap-2 rounded-sm px-2 text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] aria-pressed:bg-[var(--color-raised)] aria-pressed:text-[var(--color-bright)]"
            >
              <span>{strings.facets.name(value)}</span>
              {count === null ? null : (
                <span className="tabular text-xs text-[var(--color-muted)]">
                  {strings.facets.count(count)}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

