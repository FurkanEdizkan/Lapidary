import { Link, createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
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
import { flipFrom } from '../lib/flip'
import { Dialog } from '../components/Dialog'
import { ShowInFolder } from '../components/ShowInFolder'
import { Detail } from '../components/PartDetail'
import {
  cardSizeFor,
  layoutFor,
  namesAlwaysFor,
  pageSizeFor,
  setCardSize,
  setLayout,
  setNamesAlways,
  setPageSize,
  type CardSize,
  type Layout,
  type PageSize,
} from '../lib/preferences'
import { TopBar } from '../components/TopBar'
import { ViewMenu } from '../components/ViewMenu'
import { Sidebar } from '../components/Sidebar'
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
  const [layout, setLayoutState] = useState<Layout>(() => layoutFor(library))
  const [cardSize, setCardSizeState] = useState<CardSize>(() => cardSizeFor(library))
  const [namesAlways, setNamesAlwaysState] = useState<boolean>(() => namesAlwaysFor(library))
  /**
   * Whether the rail is showing. Not persisted, unlike everything above it: hiding the
   * sidebar is what somebody does to look at one wide thing for a moment, and a panel that
   * is still gone tomorrow is a panel they have to remember they hid.
   */
  const [sidebarOpen, setSidebarOpen] = useState(true)
  /*
    Hoisted out of the JSX for the reason `ViewMenu`'s `gallery` is: a `layout === 'list'`
    inside a child expression puts the literal `'list'` where `no-bare-strings.test.ts`
    reads it — correctly — as a label reaching the screen.
  */
  const showList = layout === 'list'
  /**
   * The part the rail is showing, and where its render sat when it was clicked.
   *
   * Held here rather than in the card that was clicked, because the rail is a sibling of
   * the grid rather than a child of a tile — which is the whole difference between this and
   * the modal it replaces. A card owning its own panel meant forty components each able to
   * open one, and two open at once was prevented only by the fact that a click closed the
   * other. One selection, in one place, cannot get into that state.
   *
   * The rect rides along because "open" and "opened from here" are the same event: the FLIP
   * flight needs an origin, and the only honest one is where the render was at the moment
   * of the click.
   */
  const [selected, setSelected] = useState<{ part: PartCard; from: DOMRect } | null>(null)
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
    /*
      The shell: a bar, then a row of a rail and a scrolling middle. `h-screen` and not
      `min-h-screen`, because the two panes scroll independently — the rail keeps its
      storage footer on screen while a long category tree moves under it, and that is only
      possible if the shell itself is exactly the height of the window and never taller.
    */
    <div className="flex h-screen flex-col">
      {/*
        Rendered, not assigned. React 19 hoists a `<title>` into the head from wherever it
        is written and removes it again on unmount, so the route that owns the page owns
        its title and `index.html`'s static one stays as the pre-hydration fallback.
        SC 2.4.2, Level A — one title for the whole application titles none of its pages.
      */}
      <title>{strings.titles.library}</title>
      {/*
        The first tab stop on the page, and off-screen until it is one.

        **Before the bar, not after it.** SC 2.4.1 is Level A and asks for a mechanism to
        skip repeated blocks; the bar is now five controls and the rail below it is dozens
        of category rows, all of which repeat on every visit and all of which come before
        the first part in the source order. A skip link placed after the thing it skips is
        not a skip link, which is what this became the moment the bar moved above it.

        Moved by `translate` rather than hidden: `display: none` and `visibility: hidden`
        both remove it from the tab order, which is the one thing it must stay in. The
        target takes `tabIndex={-1}` because a `<main>` is not focusable by default, and a
        fragment link that moves the viewport without moving focus leaves a keyboard user
        exactly where they were.
      */}
      <a
        href="#parts"
        className="ease-mechanical fixed top-4 left-4 z-30 -translate-y-20 rounded-sm border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-2 text-sm duration-[var(--duration-fast)] focus:translate-y-0"
      >
        {strings.skipToParts}
      </a>
      <TopBar
        library={library}
        sidebar={sidebarOpen}
        onToggleSidebar={() => setSidebarOpen((open) => !open)}
      >
        <SearchBox
          q={q ?? ''}
          categoryName={selectedFolderName}
          filtered={folderId !== undefined}
          onSearch={onSearch}
          onWiden={() => onSelectFolder?.(null)}
        />
        {/*
          The spacer that pushes the two trailing controls to the right end of the bar,
          which is where `v2` puts Upload and the account menu. A flexible span rather than
          `ml-auto` on the button, because the bar wraps at narrow widths and `ml-auto` on a
          wrapped row shoves the button to the far edge of a line it is alone on.
        */}
        <span className="flex-1" />
        <ViewMenu
          layout={layout}
          onLayout={(next) => {
            setLayoutState(next)
            setLayout(library, next)
          }}
          cardSize={cardSize}
          onCardSize={(next) => {
            setCardSizeState(next)
            setCardSize(library, next)
          }}
          namesAlways={namesAlways}
          onNamesAlways={(next) => {
            setNamesAlwaysState(next)
            setNamesAlways(library, next)
          }}
          pageSize={pageSize}
          onPageSize={(size) => {
            setPageSizeState(size)
            setPageSize(library, size)
          }}
        />
      </TopBar>
      {/*
        The body: the rail and the grid, side by side, each scrolling on its own. The rail
        and the grid are siblings and the drag between them needs nothing shared — a card
        writes its identity into the drag payload and a category row reads it back on drop,
        so neither holds state for the other.
      */}
      <div className="flex min-h-0 flex-1">
        {!sidebarOpen ? null : (
          <Sidebar
            library={library}
            onSelectLibrary={onSelectLibrary}
            folder={folderId ?? null}
            onSelectFolder={(folder) => onSelectFolder?.(folder)}
            instance={instance.data}
            partsLoaded={loaded.length}
          >
            <NewLibraryButton onSelect={onSelectLibrary} />
          </Sidebar>
        )}
        <main
          id="parts"
          tabIndex={-1}
          className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto px-[18px] py-[13px]"
        >
        <ActionBar
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
            {/*
              One set of cards, two arrangements. Both take the same `loaded` array and
              both open the same panel, so a part cannot mean one thing in the gallery and
              another in the list — the difference is what is on screen at rest, which is
              the difference between "which of these is it" and "which of these is
              tallest".
            */}
            {showList ? (
              <PartList parts={loaded} />
            ) : (
              <Grid
                parts={loaded}
                onOpen={(part, from) => setSelected({ part, from })}
                selectedPart={selected?.part.id}
                cardSize={cardSize}
                namesAlways={namesAlways}
              />
            )}
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
        </main>
        {/*
          The rail, a flex sibling of the grid rather than an overlay on it — see
          `Inspector` for why that is the whole change and not a styling choice.

          Keyed on the part, so switching from one to another remounts rather than updates:
          the FLIP flight runs on mount, and a rail that only re-rendered would show the
          next part's image arriving with no flight at all while the previous one's origin
          rect sat in an effect dependency that had not changed.
        */}
        {selected === null ? null : (
          <Inspector
            key={selected.part.id}
            part={selected.part}
            from={selected.from}
            hostRoot={instance.data?.hostStorageRoot ?? null}
            busy={renderPart.isPending && renderPart.variables === selected.part.id}
            onRender={(part) => renderPart.mutate(part)}
            onClose={() => setSelected(null)}
          />
        )}
      </div>
    </div>
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
  library,
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
  /** Carried only so the Removed link can hand it on; nothing here reads it otherwise. */
  library: LibraryId
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
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
      >
        {strings.scan.start}
      </button>
      <button
        type="button"
        onClick={onSweep}
        disabled={sweepBusy}
        className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
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
        // The library travels with the link. Without it the removed list answered about the
        // seeded library whatever you were looking at, which is a wrong answer rather than a
        // missing one.
        search={library === DEFAULT_LIBRARY_ID ? undefined : { library }}
        // `min-h-6` and not padding alone: WCAG 2.2 SC 2.5.8 measures the target, and a
        // 20px text link in a row of 26px buttons was the smallest thing on the page.
        className="ease-mechanical flex min-h-6 items-center text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
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
        <ul role="list" className="mt-2 space-y-1 text-sm">
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
 * The control that makes a library, at the foot of the rail's library list.
 *
 * # What this used to be
 *
 * A `LibrarySwitcher`: a `<select>` of every library plus this button, which hid the select
 * entirely below two libraries on the argument that a control offering one choice explains
 * nothing. `v2` draws the libraries as rows in the sidebar instead, and a row carries the
 * part count — the figure that tells somebody which of two similarly named libraries is the
 * one they filled, and which a select never had room for. `Sidebar` renders those rows, so
 * what is left here is the half a list of rows cannot do: add one.
 *
 * It stays in this file rather than moving into `Sidebar` because of what it opens.
 * `NewLibraryDialog` navigates on success — somebody who just made a library meant to use
 * it — and the navigation is `onSelectLibrary`, which is the route's search param. The rail
 * takes this as a child for exactly that reason: the rail draws it, the route wires it.
 */
function NewLibraryButton({ onSelect }: { onSelect?: (library: LibraryId) => void }) {
  const [creating, setCreating] = useState(false)
  const queryClient = useQueryClient()
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

  return (
    <div className="mt-1 px-2 text-xs text-[var(--color-muted)]">
      <button
        type="button"
        onClick={() => setCreating(true)}
        className="ease-mechanical w-full rounded-[var(--radius-ctl)] border border-dashed border-[var(--color-border)] px-2 py-[7px] text-left duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]"
      >
        {strings.libraries.create}
      </button>
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
    /*
      A fragment, and the three pieces are siblings in the bar rather than one box.

      The field is width-constrained and the two things that appear beside it — the category
      chip and the "keep typing" note — are not: wrapping all three in one 380px box would
      squeeze a chip carrying a category name into an ellipsis, when the bar it sits in has
      room to spare. In the bar now rather than above the grid, which is where `v2` puts it.
    */
    <>
      {/*
        The field sits *below* the ground rather than on it — `--color-raised` against the
        bar, which is how `v2` draws every input. A control you type into reads as a well;
        one you press reads as a surface.

        `max-w-[380px]` is the design's own ceiling, and it matters more here than the
        minimum does: the bar holds five controls, and a field that grows to fill a 2,560px
        window pushes the view menu off the end of it.
      */}
      <div className="relative flex min-w-[170px] max-w-[380px] flex-1 items-center">
        <span
          aria-hidden="true"
          className="pointer-events-none absolute left-[11px] text-sm text-[var(--color-muted)]"
        >
          ⌕
        </span>
        <input
          type="search"
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          aria-label={strings.search.label}
          placeholder={strings.search.placeholder}
          className="w-full rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] py-[8px] pr-3 pl-[30px] text-[12.5px] focus:border-[var(--color-accent)]"
        />
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
    </>
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

/**
 * The four card widths, as whole Tailwind class strings.
 *
 * Whole strings and not interpolation, which is the same rule the two `intrinsic` values
 * below follow and for the same reason: Tailwind scans source for literals, so a class
 * built at runtime — `` `minmax(${width},1fr)` `` — is a class that was never generated and
 * a grid that silently falls back to one column.
 *
 * `medium` is 11rem, which is what the grid has always drawn at its comfortable density, so
 * the default card is the same size it was before the slider existed. The other three step
 * around it: 8rem is the old compact width, and the two above are for judging a surface
 * rather than triaging a library.
 */
const CARD_COLUMNS: Record<CardSize, string> = {
  small: 'grid-cols-[repeat(auto-fill,minmax(8rem,1fr))] gap-3',
  medium: 'grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4',
  large: 'grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-4',
  huge: 'grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-5',
}

function Grid({
  parts,
  onOpen,
  selectedPart,
  cardSize,
  namesAlways,
}: {
  parts: readonly PartCard[]
  onOpen: (part: PartCard, from: DOMRect) => void
  /** The part the rail is showing, so its tile can say so. */
  selectedPart?: PartId
  cardSize: CardSize
  /** Whether each card paints its caption at rest, or reveals it under the pointer. */
  namesAlways: boolean
}) {
  // Two numbers move together and have to: the column width sets how tall a card ends up,
  // and `contain-intrinsic-size` is the placeholder height for one that has not rendered.
  // Give the compact grid the comfortable card's height and the scrollbar jumps as cards
  // enter and leave — which is what makes `content-visibility` look broken.
  const columns = CARD_COLUMNS[cardSize]
  /*
    The placeholder height for a card that has not rendered, and it is now simply the
    column width — because a card is a square.

    **The measured figures this used to carry no longer apply, and saying why matters.**
    The old note recorded that a compact card was *taller* than a comfortable one (measured
    in Chrome over the real 156-part library: 442-461px against 478-516px), because a
    narrower column wrapped more of a name and more of a "9.7 kB on disk, stored
    uncompressed" line into a footer under the render. `v2` has no footer: the caption is an
    overlay inside the tile, so nothing below the render grows, and a card's height is its
    width plus two hairlines. That removes the trap rather than re-measuring it — a card
    can no longer be a height nobody predicted.

    Whole class strings and not interpolation, the same rule `CARD_COLUMNS` follows:
    Tailwind scans source for literals, and a class built at runtime is a class that was
    never generated. The leading `auto` still means the browser substitutes each card's real
    size once it has rendered one, so these only have to be close on the first paint.
  */
  const intrinsic = CARD_INTRINSIC[cardSize]
  return (
    <ul role="list" className={`grid list-none ${columns}`}>
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
        <li key={part.id} className={`[content-visibility:auto] ${intrinsic}`}>
          <Card
            part={part}
            onOpen={onOpen}
            selected={part.id === selectedPart}
            namesAlways={namesAlways}
          />
        </li>
      ))}
    </ul>
  )
}

/** One per entry in `CARD_COLUMNS`, and the same widths — see `intrinsic` above. */
const CARD_INTRINSIC: Record<CardSize, string> = {
  small: '[contain-intrinsic-size:auto_8rem]',
  medium: '[contain-intrinsic-size:auto_11rem]',
  large: '[contain-intrinsic-size:auto_15rem]',
  huge: '[contain-intrinsic-size:auto_20rem]',
}

/**
 * The origin for a tile with no render yet. `flipFrom` declines on a zero-area rectangle, so
 * a part still waiting on the worker opens its panel without a flight rather than bursting
 * out of a point.
 */
const DEFAULT_ORIGIN = new DOMRect(0, 0, 0, 0)

function Card({
  part,
  onOpen,
  selected,
  namesAlways,
}: {
  part: PartCard
  /**
   * Open this part in the rail, flying its render from where the tile has it.
   *
   * The rect is measured by the card and passed up rather than measured by the rail,
   * because by the time the rail exists this tile may have scrolled, been re-laid-out by a
   * card-size change, or been replaced by the next page. Where the render *was* when it was
   * clicked is the only honest origin.
   */
  onOpen: (part: PartCard, from: DOMRect) => void
  /** Whether this is the part the rail is showing. */
  selected: boolean
  namesAlways: boolean
}) {
  const nameId = `part-name-${part.id}`
  const directory = part.directory
  // A model still in the shared store has no directory to rename, and the move route
  // refuses it. The card withholds the move rather than letting the user discover that
  // from a `409` — the same status the route uses for a name collision, which the UI
  // would otherwise present as one.
  const movable = directory !== null
  /*
    Whether the caption is painted. `namesAlways` pins it; otherwise it arrives with the
    pointer *or with focus* — `group-focus-within`, which is not a nicety.

    Opacity does not remove an element from the tab order, so the name inside the overlay
    stays focusable whether or not it is visible. Without the focus-within half, tabbing
    into a grid would move focus onto a link nobody can see, which is SC 2.4.7 failed
    outright. And `pointer-events` is what keeps the other direction honest: an invisible
    link lying over the bottom third of every tile would swallow the click that is supposed
    to open the panel, and navigate instead.
  */
  /*
    **Two layouts, not one styled two ways.** `v2` draws the caption as a gradient over the
    foot of the render when it is revealed on hover, and as a *solid footer with space
    reserved for it* when it is pinned — `paddingBottom: detail ? '58px' : 0` in the design
    file, beside a background that switches from a gradient to a flat surface.

    That is not decoration. Pinned, the caption is three lines and it is on screen for every
    tile at once; laid over the render it takes the bottom third of every part in the
    library and collides with the "no preview yet" placeholder on the ones the worker has
    not reached. Revealed, it is over one tile for as long as a pointer rests on it, where
    covering the picture costs nothing and reserving space for it would make forty tiles
    permanently smaller for a caption thirty-nine of them are not showing.

    So pinned is a flow footer under a flexible well, and revealed is an absolute overlay.
    The card stays `aspect-square` either way, so `contain-intrinsic-size` is unaffected.
  */
  const caption = namesAlways
    ? 'flex-none border-t border-[var(--color-border)] bg-[var(--color-surface)] px-[9px] pt-[7px] pb-[8px]'
    : /*
        Revealed by the pointer *or by focus* — `group-focus-within`, which is not a nicety.

        Opacity does not remove an element from the tab order, so the name inside the
        overlay stays focusable whether or not it is visible. Without the focus-within half,
        tabbing into a grid would move focus onto a link nobody can see, which is SC 2.4.7
        failed outright. And `pointer-events` is what keeps the other direction honest: an
        invisible link lying over the bottom third of every tile would swallow the click
        that is supposed to open the rail, and navigate instead.
      */
      'absolute inset-x-0 bottom-0 bg-gradient-to-t from-[rgba(18,18,20,0.97)] via-[rgba(18,18,20,0.88)] to-transparent px-[11px] pt-[22px] pb-[11px] pointer-events-none opacity-0 translate-y-2 group-hover:pointer-events-auto group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:translate-y-0 group-focus-within:opacity-100'
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
        // Measured here rather than in the panel, because by the time the panel exists this
        // tile may have been scrolled, re-laid-out by a card-size change, or replaced by the
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
        **The card is the render now, and the caption sits on it.** What stood here was a
        render in a well with a footer of text under it, which made a tile as tall as its
        longest name — `v2` puts the picture edge to edge and floats the words over its
        bottom edge instead, so a wall of parts reads as objects on shelves rather than as a
        table with pictures in the first column.

        `aspect-square` is what makes that safe to virtualize: a card's height is its width,
        so `contain-intrinsic-size` is a figure rather than a measurement — see `Grid`.

        A hairline border at rest and `--color-edge` under the pointer: the card is a control
        and 1.4.11 wants 3:1 on the boundary that identifies one, but only while it is the
        one being addressed. A wall of forty tiles all drawn at 3:1 is a grid of boxes rather
        than a page of parts.
      */
      /*
        `aria-current` and an accent border on the selected tile. The rail shows one part and
        the grid holds forty; without this the only thing saying *which* of them is open is
        the name inside the rail, which is off at the other side of the screen. The attribute
        is what carries that to a screen reader, where a border carries nothing.
      */
      aria-current={selected ? 'true' : undefined}
      className={`ease-mechanical group relative flex aspect-square cursor-pointer flex-col overflow-hidden rounded-md border bg-[var(--color-surface)] duration-[var(--duration-base)] hover:-translate-y-0.5 hover:shadow-[0_12px_26px_rgba(0,0,0,0.45)] ${
        selected
          ? 'border-[var(--color-accent)]'
          : 'border-[var(--color-border)] hover:border-[var(--color-edge)]'
      }`}
    >
      {/*
        The well the render sits in — one step *down* from the card, and the render inset
        from it rather than filling it.

        `v2` paints the thumbnail `center/86%` on `#17171b` rather than cropping it to the
        tile: the render floats with air around it, which is what stops a wall of parts
        becoming a mosaic. `p-[7%]` is the same 86% from the other side, in the one unit that
        stays proportional as the card-size control changes the column width.
      */}
      <div className="flex min-h-0 flex-1 items-center justify-center bg-[var(--color-raised)] p-[7%]">
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

      {/*
        The caption. Which of the two shapes it takes is `caption` above — a flow footer
        when pinned, an absolute gradient overlay when revealed.

        The gradient starts transparent at the top and the stops are the design's: clear
        until halfway, then near-opaque. A flat panel would crop the render at a hard line,
        and the whole reason the picture goes edge to edge is that nothing crops it.
      */}
      <div className={`ease-mechanical flex flex-col gap-[3px] duration-[var(--duration-fast)] ${caption}`}>
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
        <h2 id={nameId} className="truncate text-[12.5px] leading-snug font-semibold text-[var(--color-bright)]">
          <Link
            to="/parts/$partId"
            params={{ partId: part.id }}
            className="ease-mechanical duration-[var(--duration-fast)] hover:underline"
          >
            {part.name}
          </Link>
        </h2>
        {/*
          The part number, in the slot `v2` gives the author's avatar and name.

          It is here rather than nowhere because the design's line is a *person*, and this
          application has no user table to name one — so the choice was to drop the row or
          to put the identifier a mechanical library is actually searched by into it. A card
          in a shop library answers "is this LP-3105-A" far more often than it answers "who
          uploaded this", and the number is the thing somebody arrives with.
        */}
        {part.partNumber === null ? null : (
          <p className="tabular truncate text-[10px] text-[var(--color-muted)]">
            {part.partNumber}
          </p>
        )}
        {/*
          `CLAUDE.md` says a mesh-derived measurement is labelled approximate *always*. It
          used to sit in a hover panel where "always" quietly meant "never" — the row was
          clipped off the top of the tile at every desktop width — and the fix was to move it
          into a footer that could not be clipped.

          It is inside a reveal again, and the thing that made the old placement a lie is
          absent from this one: this overlay is anchored to the bottom edge of the tile and
          cannot be clipped by it, and it renders as one indivisible unit, so no state shows
          a figure without its badge. What a person has to do to see the numbers is point at
          the card, which is the same gesture that opens it.
        */}
        <Measurements part={part} />
      </div>

    </article>
  )
}

/**
 * The part, in a rail beside the grid — `v2`'s inspector.
 *
 * # Why this is not a dialog any more
 *
 * It was one, and the argument for it was right about the problem and wrong about the
 * shape: scanning a library means looking at one part and then the next, and a round trip
 * through a full page and the back button for each of them is what makes that tiring. A
 * modal fixes the round trip and introduces its own version of the same cost — it covers
 * the grid, so every part costs an open and a close, and you cannot see the row you were
 * working along while you read one of them.
 *
 * A rail costs nothing per part. Click a tile, the rail changes; click the next, it changes
 * again. That is the whole interaction `v2` is built around, and it is why the selected
 * tile wears the accent border: with the grid still visible, something has to say which of
 * the forty the rail is about.
 *
 * The consequences of dropping `Dialog` are all deliberate, and each one is a thing that
 * component was doing that this must **not** do:
 *
 *   - **No focus trap.** The grid beside it stays operable, which is the point.
 *   - **No `aria-modal`, no `role="dialog"`.** It is a `complementary` landmark, which is
 *     what a panel of details about the current selection is. Announcing it as a dialog
 *     would promise a trap that is deliberately absent.
 *   - **No portal.** `Dialog` portals to `<body>` because a card's hover `translate` makes
 *     it a containing block for `fixed` descendants and clipped the overlay to an 11rem
 *     tile. This is a flex sibling of `<main>` in normal flow, so there is nothing to
 *     escape from and nothing to clip it.
 *   - **Escape still closes it.** SC 2.1.2 is about not being trapped, and this never traps
 *     — but Escape is what a person presses, and there is no reason to make them find the
 *     button.
 *
 * **It renders `Detail`, the detail page's own article, rather than a version of it.** Two
 * renderings of one measurement that can disagree is a defect here, and measurements are
 * the case that matters: every figure goes through `Figure`, which cannot render a value
 * without its `approximate` flag.
 *
 * **And it fetches under the detail route's own query key**, so opening the rail and then
 * the page costs one request rather than two — the rail warms the cache for the page it
 * links to.
 */
function Inspector({
  part,
  from,
  hostRoot,
  busy,
  onRender,
  onClose,
}: {
  part: PartCard
  /** Where this part's render sat on the grid when it was clicked. */
  from: DOMRect
  hostRoot: string | null
  busy: boolean
  onRender: (id: PartId) => void
  onClose: () => void
}) {
  const [moving, setMoving] = useState(false)
  /*
    A model still in the shared store has no directory to rename and the move route refuses
    it. The rail withholds the control rather than letting a person discover that from a
    `409` — the same status the route uses for a name collision, which the UI would then
    present as one.
  */
  const movable = part.directory !== null
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
    then — the rail is open and empty for as long as the fetch takes.
  */
  useLayoutEffect(() => {
    const image = panel.current?.querySelector('img')
    if (image != null) flipFrom(image, from)
  }, [detail.data, from])

  /*
    Escape closes it, listened for on the document rather than on the rail.

    On the document because focus is usually not in here: a person clicks a tile and the
    rail fills while focus stays on the grid, so a handler bound to this subtree would hear
    nothing. That is the opposite of `Dialog`'s reason for doing the same thing — there,
    focus is trapped inside and falls to `<body>` when a button disables itself — and it
    arrives at the same place.
  */
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close.current()
    }
    document.addEventListener('keydown', escape)
    return () => document.removeEventListener('keydown', escape)
  }, [])

  return (
    <aside
      aria-label={strings.inspector.title}
      className="panel-in flex w-[340px] flex-none flex-col overflow-y-auto border-l border-[var(--color-border)] bg-[var(--color-raised)]"
    >
      <div className="sticky top-0 z-10 flex flex-none items-center gap-2 border-b border-[var(--color-border)] bg-[var(--color-raised)] px-[14px] py-[11px]">
        <h2 className="tabular flex-1 text-[9px] tracking-[0.22em] text-[var(--color-muted)] uppercase">
          {strings.inspector.title}
        </h2>
        <button
          type="button"
          onClick={onClose}
          aria-label={strings.inspector.close}
          className="ease-mechanical grid size-[26px] flex-none place-items-center rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]"
        >
          <span aria-hidden="true">✕</span>
        </button>
      </div>

      <div ref={panel} className="flex-1 px-[14px] py-[13px]">
        {detail.isPending ? (
          <p className="text-sm text-[var(--color-muted)]">{strings.quickLook.loading}</p>
        ) : detail.isError ? (
          <p className="max-w-prose text-sm text-[var(--color-muted)]">
            {strings.quickLook.failed}
          </p>
        ) : (
          <Detail
            part={detail.data}
            /*
              The tools the card used to carry. They are here because the card is a picture
              and a name now: a control that hides the render it sits on is a control
              fighting the one job this product has. Owner decision, 2026-09-08 — click
              gives you the essential information and the tools; the full page gives you
              depth.
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
                {!movable ? (
                  <span className="text-xs text-[var(--color-muted)]">
                    {strings.folders.notMigrated}
                  </span>
                ) : (
                  <button
                    type="button"
                    onClick={() => setMoving(true)}
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
        <ShowInFolder part={part} hostRoot={hostRoot} />
        <div className="mt-4">
          <Link
            to="/parts/$partId"
            params={{ partId: part.id }}
            className="ease-mechanical inline-block rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px"
          >
            {strings.quickLook.fullPage}
          </Link>
        </div>
      </div>

      {/*
        Still a modal, and correctly so: choosing where a model goes is a decision, the grid
        behind it must not be clicked while it is open, and it is the one place a duplicate
        name is answered. `Dialog` portals itself out to `<body>` — nothing here may hoist
        that markup back.
      */}
      {moving ? (
        <MovePartDialog
          part={{ id: part.id, name: part.name }}
          library={part.library}
          onClose={() => setMoving(false)}
        />
      ) : null}
    </aside>
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
  const bbox = usableBbox(part.bboxMm)
  /*
    Whether this line carries a mesh-derived figure, and so whether the badge belongs on it.

    **A bounding box is not evidence of tessellation.** That is the whole of this predicate.
    A B-rep part carries an analytic box, and from Phase 2 that is a part this grid will
    show — so keying the badge to "has a measurement" would stamp `APPROXIMATE` on every
    measured part, analytic ones included. That is the measurement rule broken in the
    direction that matters: a figure claiming to be less exact than it is still misstates
    its provenance, and a user deciding whether to trust a tolerance cannot tell an
    over-cautious label from a true one.

    `part.approximate` means *any* figure on this part is mesh-derived, which is weaker than
    "this box is" and is the strongest claim a card-level flag can make. Phase 2 is when it
    stops being enough — see `PartSummary::volume_mm3` for the trigger to widen it.
  */
  const labelled = part.approximate
  if (bbox === null && !labelled) {
    return null
  }
  return (
    <p className="tabular flex flex-wrap items-center gap-x-[6px] gap-y-[2px] text-[9px] text-[var(--color-dim)]">
      {/*
        The box, and only the box.

        **The triangle count is deliberately not here**, and it used to be. A tile is 176px
        of caption at the default card size, and the count competed for it with the one
        figure somebody scanning a grid is actually comparing — a count says how heavy a
        mesh is, which is not why anyone opens a parts library. `v2`'s card shows dimensions
        and no count, which is the same judgement.

        It is not lost: the list layout gives it a column of its own, where a column of
        counts *can* be compared, and the detail page states it with its own provenance.
      */}
      {bbox === null ? null : <span>{strings.parts.dimensions(bbox)}</span>}
      {!labelled ? null : (
        <span
          title={strings.parts.approximateDetail}
          className="rounded border border-[var(--color-border)] px-1 py-px tracking-wider uppercase"
        >
          {strings.parts.approximate}
        </span>
      )}
    </p>
  )
}

/**
 * A bounding box that can actually be rendered, or `null`.
 *
 * The wire type says `[number, number, number] | null` and the response is cast rather than
 * validated, so this is the one place that checks it is true. Three finite numbers or
 * nothing: `Number.isFinite` refuses `NaN` and both infinities as well as a string, and a
 * partial box must not render as two dimensions and a `NaN` — `CLAUDE.md` treats a figure
 * that states something untrue as a correctness bug, and a box is three figures.
 *
 * Shared by the card, the list row and the list's own sort, so a part that is unmeasured is
 * unmeasured in all three rather than in whichever of them remembered to look.
 */
function usableBbox(
  bbox: PartCard['bboxMm'],
): readonly [number, number, number] | null {
  if (!Array.isArray(bbox) || bbox.length !== 3) return null
  return bbox.every((axis) => typeof axis === 'number' && Number.isFinite(axis)) ? bbox : null
}

/**
 * The same parts, one per row, with the figures in columns.
 *
 * # What this is for
 *
 * A gallery answers "which of these is the one I want" and cannot answer "which of these is
 * tallest": forty square renders at 11rem give no way to compare a number down a column,
 * because there is no column. This is the layout that does, and it is why the bounding box
 * had to reach `PartCard` — a list of names and thumbnails would be a worse gallery rather
 * than a different tool.
 *
 * # It is a table, and it says so
 *
 * `<table>` and not a stack of flex rows. The figures here are a grid of values with row and
 * column headers, which is the one thing table semantics exist for: a screen-reader user
 * moving down the Volume column is told they are in the Volume column, and a `role="list"`
 * of divs cannot say that at all. The cost is that column widths need stating, which
 * `table-fixed` and the widths below do.
 *
 * No `content-visibility` here, unlike the grid. A row is text and one 46px thumbnail, so
 * there is little layout to skip and — more to the point — a row's height does not depend on
 * an image that has not decoded yet, so the placeholder figure the grid needs has no
 * equivalent to be wrong about.
 */
function PartList({ parts }: { parts: readonly PartCard[] }) {
  return (
    <table className="w-full table-fixed border-collapse text-left">
      <caption className="sr-only">{strings.folders.root}</caption>
      <thead>
        <tr className="border-b border-[var(--color-border)]">
          <Th className="w-auto">{strings.layout.columnName}</Th>
          <Th className="hidden w-[9rem] sm:table-cell">{strings.layout.columnPartNumber}</Th>
          <Th className="w-[11rem] text-right">{strings.layout.columnDimensions}</Th>
          <Th className="hidden w-[7rem] text-right md:table-cell">
            {strings.layout.columnVolume}
          </Th>
          <Th className="hidden w-[7rem] text-right lg:table-cell">
            {strings.layout.columnTriangles}
          </Th>
        </tr>
      </thead>
      <tbody>
        {parts.map((part) => (
          <ListRow key={part.id} part={part} />
        ))}
      </tbody>
    </table>
  )
}

/**
 * A column heading. `scope="col"` is what makes the association a screen reader announces —
 * without it a `th` is a styled cell and the column name is never read with the value.
 *
 * The narrow columns drop out below their breakpoints rather than compressing: a part number
 * ellipsised to `LP-10…` identifies nothing, and the two columns that survive at every width
 * are the name and the box, which are the two the layout exists for.
 */
function Th({ className, children }: { className: string; children: React.ReactNode }) {
  return (
    <th
      scope="col"
      className={`tabular pb-2 text-[9px] font-normal tracking-[0.18em] text-[var(--color-muted)] uppercase ${className}`}
    >
      {children}
    </th>
  )
}

function ListRow({ part }: { part: PartCard }) {
  const bbox = usableBbox(part.bboxMm)
  const volume = typeof part.volumeMm3 === 'number' ? part.volumeMm3 : null
  const count = typeof part.triangleCount === 'number' ? part.triangleCount : null
  /*
    Whether this row carries a mesh-derived figure — the same predicate the card uses, plus
    the count, which the card no longer shows and this does.

    **A triangle count is tessellation-derived by construction**, so a row showing one is
    showing a mesh figure whatever the wire's `approximate` says. That is not a hypothetical
    inconsistency to guard against: nothing stops a revision carrying a count beside a flag
    set false, and the card's own history is that the count and the badge were independent
    conditionals until a test paired them.

    One badge for the row rather than one per cell. `approximate` is a fact about the part,
    not about a column, so a badge in each of three cells would be the same word three times
    saying one thing — and forty rows of that is a column of nothing but the word.
  */
  const labelled = part.approximate || count !== null
  return (
    <tr className="ease-mechanical border-b border-[var(--color-border)] duration-[var(--duration-fast)] hover:bg-[var(--color-surface)]">
      {/*
        `th scope="row"`, not a `td`. The name is what identifies the row, and a screen
        reader reading the Volume cell announces the row header with it — "Bearing block,
        608ZZ, Volume, 9.84 cm³" rather than a figure with no subject.
      */}
      <th scope="row" className="py-[7px] pr-3 font-normal">
        <span className="flex min-w-0 items-center gap-3">
          <span className="grid size-[46px] flex-none place-items-center overflow-hidden rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] p-[5px]">
            {part.thumbnail === null ? null : (
              <img
                src={part.thumbnail}
                alt={strings.parts.thumbnailAlt(part.name)}
                className="h-full w-full object-contain"
              />
            )}
          </span>
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <Link
                to="/parts/$partId"
                params={{ partId: part.id }}
                className="ease-mechanical min-w-0 truncate text-[12.5px] font-semibold text-[var(--color-text)] duration-[var(--duration-fast)] hover:underline"
              >
                {part.name}
              </Link>
              {/*
                The row's one badge, beside the thing it is a fact about. `CLAUDE.md` says a
                mesh-derived measurement is labelled always, and the figures it qualifies are
                three cells to the right — which is fine on a row, where the eye and a screen
                reader both travel along one part, and is exactly why it may not be dropped
                just because the columns are elsewhere.
              */}
              {!labelled ? null : (
                <span
                  title={strings.parts.approximateDetail}
                  className="tabular flex-none rounded border border-[var(--color-border)] px-1 py-px text-[9px] tracking-wider text-[var(--color-dim)] uppercase"
                >
                  {strings.parts.approximate}
                </span>
              )}
            </span>
            {/*
              The path under the name, which is a part's identity — two parts can share a
              name and only the path tells them apart. It is the one thing this layout has
              room for that the card does not.

              Plain text, not `ShowInFolder`. That component is a disclosure with a button
              to open it, which is right on a page showing one part and wrong on forty rows:
              it put a 26px control in every row and turned a dense table back into a stack
              of cards. The full host path it reveals is a per-part question, and the rail
              and the detail page both still answer it.
            */}
            <span className="tabular block truncate text-[10px] text-[var(--color-muted)]">
              {part.sourcePath}
            </span>
          </span>
        </span>
      </th>
      <Figure className="hidden sm:table-cell">{part.partNumber}</Figure>
      {/*
        The figures. Their provenance is the one badge in the name cell above; the detail
        page this row links to states each figure's own — see `Figure` in `PartDetail.tsx`.
      */}
      <Figure className="text-right">{bbox === null ? null : strings.parts.dimensions(bbox)}</Figure>
      <Figure className="hidden text-right md:table-cell">
        {volume === null ? null : strings.parts.volume(volume)}
      </Figure>
      <Figure className="hidden text-right lg:table-cell">
        {count === null ? null : strings.parts.triangles(count)}
      </Figure>
    </tr>
  )
}

/**
 * One figure cell, or the mark for a part nobody has measured.
 *
 * An em dash on screen and "Not measured" to a screen reader, which is not the same string
 * twice: a screen reader reads `—` as nothing at all, so a row with three of them announces
 * three empty cells and no reason for them. `aria-hidden` on the dash and a visually hidden
 * word beside it is what makes the cell say one thing rather than none.
 */
function Figure({ className, children }: { className: string; children: string | null }) {
  return (
    <td className={`tabular py-[7px] text-[10.5px] text-[var(--color-muted)] ${className}`}>
      {children === null ? (
        <>
          <span aria-hidden="true">{strings.layout.unmeasured}</span>
          <span className="sr-only">{strings.layout.unmeasuredLabel}</span>
        </>
      ) : (
        children
      )}
    </td>
  )
}
