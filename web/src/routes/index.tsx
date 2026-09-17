import { createFileRoute } from '@tanstack/react-router'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useId, useRef, useState } from 'react'
import {
  batchEventsUrl,
  blobUrl,
  DEFAULT_LIBRARY_ID,
  downloadBundle,
  fetchBatchStatus,
  fetchFacets,
  fetchHealth,
  fetchInstanceStorage,
  fetchLibrarySettings,
  fetchLibraryStorage,
  fetchParts,
  fetchSavedFilters,
  movePart,
  moveSavedFilter,
  planBundle,
  RefusedError,
  removePart,
  removeSavedFilter,
  renameSavedFilter,
  renderLibraryThumbnails,
  renderPartThumbnail,
  saveFilter,
  setAutoThumbnail,
  startScan,
} from '../lib/api'
import { loadViewerWhenIdle, warmViewer } from '../components/PartDetail'
import { FieldFilters } from '../components/Fields'
import {
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
import { createPrefetch } from '../lib/prefetch'
import { importBundle, uploadFiles } from '../lib/upload'
import type { PickedFile, UploadProgress } from '../lib/upload'
import { FolderTree, MovePartDialog, PickCategoryDialog, refusalMessage, useFolders } from '../components/FolderTree'
import type {
  BatchId,
  BatchStatus,
  FilterSearch,
  SavedFilterId,
  FacetValue,
  FolderId,
  LibraryId,
  MoveDirection,
  PartCard,
  PartId,
  ScanAccepted,
} from '../lib/types'
import { DEFAULT_ORIGIN } from '../components/Card'
import { BULK_CONCURRENCY, Grid, GridSkeleton, MorePages, SelectionBar } from '../components/Grid'
import type { BulkProgress } from '../components/Grid'
import { QuickLook, useWide } from '../components/QuickLook'
import { InstanceStorage, StorageDetails, StorageTotals } from '../components/Storage'
import { LibraryMenu, Toolbar } from '../components/Toolbar'
import { SearchBox } from '../components/Search'
import { AppFrame } from '../components/AppFrame'
import { DropOverlay, ScanProgress, UploadButton, jobsSettled, progressLine } from '../components/Upload'
import type { BatchKind } from '../components/Upload'

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
  ): { batch?: string; folderId?: string; q?: string; library?: string; format?: string; material?: string; tag?: string; part?: string; field?: string; fieldValue?: string; fieldMin?: string; fieldMax?: string } => {
    const batch = search.batch
    const folderId = search.folderId
    const q = search.q
    const library = search.library
    const format = search.format
    const part = search.part
    const material = search.material
    const tag = search.tag
    const field = searchText(search.field)
    const fieldValue = searchText(search.fieldValue)
    const fieldMin = searchText(search.fieldMin)
    const fieldMax = searchText(search.fieldMax)
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
      // A material as the file names it. Case is kept: `S235JR` and `s235jr` are not one name.
      ...(typeof material === 'string' && material.length > 0 ? { material } : {}),
      // A tag as a person wrote it. A tag can be all digits, which arrives as a number for the
      // reason `q` gives.
      ...(typeof tag === 'string' && tag.length > 0
        ? { tag }
        : typeof tag === 'number'
          ? { tag: String(tag) }
          : {}),
      // A field offered as a filter with its value or its range, or none of them, as the route reads them. A
      // key, a value and a bound can each be all digits, which arrives as a number for the reason `q` gives.
      ...(field !== undefined && (fieldValue !== undefined || fieldMin !== undefined || fieldMax !== undefined)
        ? {
            field,
            ...(fieldValue === undefined ? {} : { fieldValue }),
            ...(fieldMin === undefined ? {} : { fieldMin }),
            ...(fieldMax === undefined ? {} : { fieldMax }),
          }
        : {}),
    }
  },
})

/** A search param as text: a string as it is, a number as its digits, and an empty string or anything else as absent. */
function searchText(value: unknown): string | undefined {
  if (typeof value === 'number') return String(value)
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/**
 * Reads the search params and hands them to `Index` as props. `Index` takes the batch and
 * the selected category rather than calling `useSearch` itself so it stays renderable
 * without a router — which is how `index.test.tsx` renders it.
 *
 * The selection lives in the URL for the reason every other filter does: it survives a
 * reload and it is a link a person can send someone.
 */
function RouteComponent() {
  const { batch, folderId, q, library, format, material, tag, part, field, fieldValue, fieldMin, fieldMax } = Route.useSearch()
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
      material={material}
      onSelectMaterial={(value) =>
        void navigate({
          search: (previous) => ({ ...previous, material: value ?? undefined, part: undefined }),
        })
      }
      tag={tag}
      onSelectTag={(value) =>
        void navigate({
          search: (previous) => ({ ...previous, tag: value ?? undefined, part: undefined }),
        })
      }
      field={field}
      fieldValue={fieldValue}
      fieldMin={fieldMin}
      fieldMax={fieldMax}
      onSelectField={(key, value, range) =>
        void navigate({
          search: (previous) => ({
            ...previous,
            field: key ?? undefined,
            fieldValue: value ?? undefined,
            fieldMin: range?.min,
            fieldMax: range?.max,
            part: undefined,
          }),
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
      onApplyFilter={(search) =>
        void navigate({
          // In place of every filter there, and without the open part: a saved filter is a new
          // look at the library, and a step the back button can undo.
          search: (previous) => ({ library: previous.library, ...search }),
        })
      }
    />
  )
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
  material,
  onSelectMaterial,
  tag,
  onSelectTag,
  field,
  fieldValue,
  fieldMin,
  fieldMax,
  onSelectField,
  part,
  onOpenPart,
  onApplyFilter,
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
  /** The material the grid is narrowed to, as the URL carries it. Absent is every material. */
  material?: string
  onSelectMaterial?: (material: string | null) => void
  /** The tag the grid is narrowed to, as the URL carries it. Absent is every tag. */
  tag?: string
  onSelectTag?: (tag: string | null) => void
  /** A custom field offered as a filter, and the value it must hold or, for a number, the range it lies in. */
  field?: string
  fieldValue?: string
  fieldMin?: string
  fieldMax?: string
  /** Writes a field filter to the URL, a value or a range; `null` clears it. */
  onSelectField?: (field: string | null, value: string | null, range?: { min?: string; max?: string }) => void
  /** The part the quick look is open on, as the URL carries it. */
  part?: string
  /** Writes the open part to the URL; `null` closes it. */
  onOpenPart?: (part: PartId | null) => void
  /** Puts a saved filter's filters on the grid in one step, in place of the ones there. */
  onApplyFilter?: (search: FilterSearch) => void
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
  // A category the live tree does not hold: deleted since a saved filter or a link named it. Decided
  // only once the tree has loaded, so a tree still on its way never reads as a deleted category.
  const categoryGone =
    folderId !== undefined &&
    folders.data !== undefined &&
    !folders.data.some((folder) => folder.id === folderId)

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
  const bundlePicker = useRef<HTMLInputElement>(null)
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
    queryKey: ['parts', library, folderId ?? null, q ?? null, pageSize, format ?? null, material ?? null, tag ?? null, field ?? null, fieldValue ?? null, fieldMin ?? null, fieldMax ?? null, order],
    queryFn: ({ pageParam }) =>
      fetchParts(library, pageParam, undefined, folderId, q, pageSize, format, order, material, tag, field, fieldValue, fieldMin, fieldMax),
    initialPageParam: undefined as PartId | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
  })
  // A field filter the server refuses: no longer offered, or its value no longer fits the field. The grid
  // says so, where "check that the api service is running" would be untrue.
  const refusal = field !== undefined && parts.error instanceof RefusedError ? parts.error.reason : undefined
  const rangeGiven = fieldMin !== undefined || fieldMax !== undefined
  // A value that no longer fits reads as a field defined again, as it always has. A bound that is not a number
  // is a slip in the range boxes, not a field gone.
  const fieldGone =
    refusal === 'notAFilter' || refusal === 'notARange' || (refusal === 'wrongType' && !rangeGiven)
  // A field filter no grid can take: a value beside a range, a range that runs backwards, or a bound that is
  // not a number.
  const fieldUnreadable =
    refusal === 'valueAndRange' || refusal === 'emptyRange' || (refusal === 'wrongType' && rangeGiven)
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

  /**
   * Prefetch on intent (`DATA.md` §2.4): a hovered card warms its L0 rung and the viewer itself, and
   * an opened part warms its neighbours' — the parts a person looks at next. L0 and not L1 for the neighbours, because a
   * card carries only its L0 hash and fetching two more details to learn the L1s would cost more
   * than the prefetch saves. Everything in flight is dropped when the grid changes under it.
   */
  const [prefetch] = useState(() => createPrefetch(2))
  const warm = (card: PartCard | undefined) => {
    if (card === undefined || card.tessellationL0 === null) return
    prefetch.request(blobUrl(card.tessellationL0))
    void warmViewer()
  }
  // A tap, or a press with no hover first, gives the hover nothing to warm on, so the grid loads the
  // viewer's code as soon as the browser is idle; its renderer waits for a hover or an open.
  useEffect(loadViewerWhenIdle, [])
  const lookingIndex = looking === undefined ? -1 : loaded.indexOf(looking)
  useEffect(() => {
    if (lookingIndex === -1) return
    warm(loaded[lookingIndex - 1])
    warm(loaded[lookingIndex + 1])
    // Keyed on where the look is, not on `loaded`'s identity, which changes on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lookingIndex])
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
  const scope = [library, folderId ?? '', format ?? '', material ?? '', tag ?? '', q ?? ''].join('\u0000')
  const [selectionScope, setSelectionScope] = useState(scope)
  useEffect(() => () => prefetch.cancel(), [scope, prefetch])
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
  // One request for the whole selection, planned first, so a refusal is said on the grid rather
  // than as a page of JSON the browser navigates to. Then a form post, which streams to disk.
  const [exportNote, setExportNote] = useState<string | null>(null)
  const exportSelected = async () => {
    const ids = [...selected]
    try {
      const answer = await planBundle(library, ids)
      if ('refused' in answer) {
        setExportNote(answer.refused ?? strings.selection.exportFailed)
        return
      }
      downloadBundle(library, ids)
      setExportNote(strings.selection.exporting(answer.plan.parts, answer.plan.revisions, answer.plan.bytes))
    } catch {
      setExportNote(strings.selection.exportFailed)
    }
  }
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
  // A bundle follows the same batch line an upload does, once its import job is queued.
  const importing = useMutation({
    mutationFn: (file: File) => importBundle(library, file),
    onSuccess: (accepted) => {
      setUploadNote(null)
      watch(accepted, 'upload')
    },
    onError: () => setUploadNote(strings.toolbar.importFailed),
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
    <AppFrame
      current="parts"
      library={library}
      skipTo={{ href: '#parts', label: strings.skipToParts }}
      search={
        <SearchBox
          q={q ?? ''}
          categoryName={selectedFolderName}
          filtered={folderId !== undefined}
          onSearch={onSearch}
          onWiden={() => onSelectFolder?.(null)}
        />
      }
      actions={
        <>
          <LibraryMenu
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
            onScan={() => scanNow.mutate()}
            scanBusy={scanNow.isPending}
            onSweep={() => sweep.mutate()}
            sweepBusy={sweep.isPending}
            onSelectLibrary={onSelectLibrary}
            onImport={() => bundlePicker.current?.click()}
            importBusy={importing.isPending}
          />
          <UploadButton onUpload={() => picker.current?.click()} busy={upload.isPending} progress={uploading} />
        </>
      }
      rail={
        <>
          {/*
            The tree and the grid are siblings, and the drag between them needs nothing
            shared: a card writes its identity into the drag payload and a category row
            reads it back on drop, so neither holds state for the other.
          */}
          <SavedFilters
            library={library}
            current={filtersOf({ q, folderId, format, material, tag, field, fieldValue, fieldMin, fieldMax })}
            onApply={(search) => onApplyFilter?.(search)}
          />
          <Facets
            library={library}
            folderId={folderId}
            q={q}
            format={format}
            material={material}
            tag={tag}
            field={field}
            fieldValue={fieldValue}
            fieldMin={fieldMin}
            fieldMax={fieldMax}
            onSelectFormat={(value) => onSelectFormat?.(value)}
            onSelectMaterial={(value) => onSelectMaterial?.(value)}
            onSelectTag={(value) => onSelectTag?.(value)}
            onSelectField={(key, value, range) => onSelectField?.(key, value, range)}
          />
          <FolderTree
            library={library}
            selected={folderId ?? null}
            onSelect={(folder) => onSelectFolder?.(folder)}
          />
          <StorageDetails
            total={storage.data === undefined ? null : storage.data.sourceBytes + storage.data.derivativeBytes}
          >
            {loaded.length === 0 ? null : (
              <>
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
            {/* A failure is the banner over the grid, not a line in a closed disclosure. */}
            {health.isError ? null : (
              <p className="mt-2 text-xs text-[var(--color-muted)]">
                {health.isPending ? strings.health.checking : strings.health.ok(health.data.database.major)}
              </p>
            )}
          </StorageDetails>
        </>
      }
    >
      {/*
        Rendered, not assigned. React 19 hoists a `<title>` into the head from wherever it
        is written and removes it again on unmount, so the route that owns the page owns
        its title and `index.html`'s static one stays as the pre-hydration fallback.
        SC 2.4.2, Level A — one title for the whole application titles none of its pages.
      */}
      <title>{strings.titles.library}</title>
      <div id="parts" tabIndex={-1} className="min-w-0 flex-1">
        <Toolbar
          scope={
            <>
              {/*
                The scope, which `v2` puts above the grid. Two facts a person needs before they
                start scanning rather than after they finish: what they are looking at, and how
                much of it there is. `h2` because it names the region the grid fills; the page's
                `h1` is the application's name in the bar. The count is `.tabular`, so a number
                that changes as pages load does not shift the words beside it, and it is only
                there once there is a count to give.
              */}
              <h2 className="text-[15px] leading-none font-semibold text-[var(--color-bright)]">
                {selectedFolderName ?? strings.folders.root}
              </h2>
              {parts.isSuccess && loaded.length > 0 ? (
                <p className="tabular text-[10.5px] text-[var(--color-muted)]">
                  {parts.hasNextPage
                    ? strings.parts.showingSoFar(loaded.length)
                    : strings.parts.showingAll(loaded.length)}
                </p>
              ) : null}
            </>
          }
          settingsNote={
            settings.isError
              ? strings.library.autoThumbnailFailed
              : librarySettings.isError
                ? strings.library.autoThumbnailUnknown
                : null
          }
          note={note}
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
        />
        <DropOverlay onFiles={startUpload} picker={picker} />
        {upload.isPending && uploading !== undefined ? (
          <p role="status" className="mb-4 text-sm text-[var(--color-muted)]">
            {progressLine(uploading)}
          </p>
        ) : null}
        <input
          ref={bundlePicker}
          type="file"
          accept=".zip,application/zip"
          hidden
          aria-label={strings.toolbar.importBundle}
          onChange={(event) => {
            const file = event.target.files?.[0]
            event.target.value = ''
            if (file !== undefined) importing.mutate(file)
          }}
        />
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
        {health.isError ? (
          <p role="alert" className="mb-4 rounded-[var(--radius-ctl)] border border-[var(--color-bad)] px-3 py-2 text-sm text-[var(--color-text)]">
            {strings.health.failed}
          </p>
        ) : null}
        {parts.isPending ? (
          <GridSkeleton label={strings.parts.loading} />
        ) : fieldGone || fieldUnreadable ? (
          <FilterGone
            text={fieldGone ? strings.fieldGone : strings.fieldUnreadable}
            onWiden={() => onSelectField?.(null, null)}
          />
        ) : parts.isError ? (
          <p className="max-w-prose text-[var(--color-muted)]">{strings.parts.failed}</p>
        ) : loaded.length === 0 ? (
          // An empty page and a page still in flight are different facts, so only a page
          // that came back empty gets the empty state — and which empty state depends on
          // whether a category is filtering it, because "this library is empty" is false
          // and alarming when the library is full and the category is not.
          categoryGone ? (
            <FilterGone text={strings.categoryGone} onWiden={() => onSelectFolder?.(null)} />
          ) : (
            <EmptyLibrary
              filtered={folderId !== undefined}
              categoryName={selectedFolderName}
              query={q ?? null}
              onWiden={() => onSelectFolder?.(null)}
              onClearSearch={() => onSearch?.('')}
            />
          )
        ) : (
          <>
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
                onExport={() => void exportSelected()}
                note={exportNote}
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
              onHover={warm}
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
          </>
        )}
      </div>
      {/*
        The pane is the page's third column on a wide screen: not modal, so the grid beside it
        stays clickable and the next card swaps what it shows.
      */}
      {wide ? look : null}
    </AppFrame>
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
 * Saved filters, above the facets: the grid's filters kept under a name for this library and put
 * back in one step, with the one the grid is showing marked. Drawn only where there is something to
 * list or something to save, so a library nobody has filtered shows nothing here.
 */
function SavedFilters({
  library,
  current,
  onApply,
}: {
  library: LibraryId
  current: FilterSearch
  onApply: (search: FilterSearch) => void
}) {
  const queryClient = useQueryClient()
  const titleId = useId()
  const filters = useQuery({
    queryKey: ['savedFilters', library],
    queryFn: () => fetchSavedFilters(library),
  })
  const [naming, setNaming] = useState(false)
  const [name, setName] = useState('')
  const [refusal, setRefusal] = useState<string | null>(null)
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ['savedFilters', library] })
  const save = useMutation({
    mutationFn: () => saveFilter(library, { name, search: current }),
    onSuccess: (answer) => {
      if (answer.kind === 'refused') {
        setRefusal(answer.message)
        return
      }
      setNaming(false)
      setName('')
      setRefusal(null)
      refresh()
    },
    onError: () => setRefusal(strings.savedFilters.refusedWithoutReason),
  })
  const remove = useMutation({
    mutationFn: (filter: SavedFilterId) => removeSavedFilter(library, filter),
    onSettled: refresh,
  })
  const [renaming, setRenaming] = useState<SavedFilterId | null>(null)
  const [newName, setNewName] = useState('')
  const [renameRefusal, setRenameRefusal] = useState<string | null>(null)
  const rename = useMutation({
    mutationFn: ({ filter, name: next }: { filter: SavedFilterId; name: string }) =>
      renameSavedFilter(library, filter, next),
    onSuccess: (answer) => {
      if (answer.kind === 'refused') {
        setRenameRefusal(answer.message)
        return
      }
      setRenaming(null)
      setRenameRefusal(null)
      refresh()
    },
    onError: () => setRenameRefusal(strings.savedFilters.refusedWithoutReason),
  })
  const move = useMutation({
    mutationFn: ({ filter, direction }: { filter: SavedFilterId; direction: MoveDirection }) =>
      moveSavedFilter(library, filter, direction),
    onSettled: refresh,
  })
  const filtering = Object.keys(current).length > 0
  const saved = filters.data ?? []
  if (!filtering && saved.length === 0 && !filters.isError) return null
  return (
    <section aria-labelledby={titleId} className="mb-6">
      <h2 id={titleId} className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {strings.savedFilters.title}
      </h2>
      {filters.isError ? (
        <p role="alert" className="mb-2 text-xs text-[var(--color-muted)]">
          {strings.savedFilters.failed}
        </p>
      ) : null}
      {saved.length === 0 ? null : (
        <ul role="list" className="space-y-0.5">
          {saved.map((filter, index) =>
            renaming === filter.id ? (
              <li key={filter.id}>
                <form
                  className="space-y-1"
                  onSubmit={(event) => {
                    event.preventDefault()
                    if (newName.trim().length > 0) rename.mutate({ filter: filter.id, name: newName })
                  }}
                >
                  <label className="block text-xs text-[var(--color-muted)]">
                    {strings.savedFilters.renameLabel}
                    <input
                      value={newName}
                      maxLength={80}
                      autoFocus
                      onChange={(event) => setNewName(event.target.value)}
                      className="mt-1 block w-full rounded-sm border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-sm text-[var(--color-text)]"
                    />
                  </label>
                  <div className="flex gap-1">
                    <button
                      type="submit"
                      disabled={rename.isPending || newName.trim().length === 0}
                      className="ease-mechanical min-h-6 rounded-sm border border-[var(--color-edge)] px-2 text-xs duration-[var(--duration-fast)] hover:text-[var(--color-bright)] disabled:opacity-50"
                    >
                      {strings.savedFilters.renameConfirm}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setRenaming(null)
                        setRenameRefusal(null)
                      }}
                      className="ease-mechanical min-h-6 rounded-sm px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
                    >
                      {strings.savedFilters.cancel}
                    </button>
                  </div>
                  {renameRefusal === null ? null : (
                    <p role="alert" className="text-xs text-[var(--color-muted)]">
                      {renameRefusal}
                    </p>
                  )}
                </form>
              </li>
            ) : (
              <li key={filter.id} className="flex items-center gap-1">
                <button
                  type="button"
                  aria-current={sameFilters(filter.search, current) ? 'true' : undefined}
                  onClick={() => onApply(filter.search)}
                  className="ease-mechanical flex min-h-6 min-w-0 flex-1 flex-col items-start justify-center rounded-sm px-2 text-left text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] aria-[current=true]:bg-[var(--color-raised)] aria-[current=true]:text-[var(--color-bright)]"
                >
                  <span className="w-full truncate">{filter.name}</span>
                  {filter.folderGone ? (
                    // Under the name rather than beside it: beside it, the mark took the narrow rail's
                    // width and cut the filter's own name to a few letters.
                    <span className="pb-0.5 text-[10px] leading-none text-[var(--color-muted)]">
                      {strings.savedFilters.folderGone}
                    </span>
                  ) : null}
                </button>
                <button
                  type="button"
                  aria-label={strings.savedFilters.moveUp(filter.name)}
                  disabled={index === 0 || move.isPending}
                  onClick={() => move.mutate({ filter: filter.id, direction: UP })}
                  className="px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)] disabled:opacity-30"
                >
                  {strings.glyphs.moveUp}
                </button>
                <button
                  type="button"
                  aria-label={strings.savedFilters.moveDown(filter.name)}
                  disabled={index === saved.length - 1 || move.isPending}
                  onClick={() => move.mutate({ filter: filter.id, direction: DOWN })}
                  className="px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)] disabled:opacity-30"
                >
                  {strings.glyphs.moveDown}
                </button>
                <button
                  type="button"
                  aria-label={strings.savedFilters.rename(filter.name)}
                  onClick={() => {
                    setRenaming(filter.id)
                    setNewName(filter.name)
                    setRenameRefusal(null)
                  }}
                  className="px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)]"
                >
                  {strings.glyphs.rename}
                </button>
                <button
                  type="button"
                  aria-label={strings.savedFilters.remove(filter.name)}
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(filter.id)}
                  className="px-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-bright)] disabled:opacity-50"
                >
                  {strings.glyphs.remove}
                </button>
              </li>
            ),
          )}
        </ul>
      )}
      {!filtering ? null : naming ? (
        <form
          className="mt-2 space-y-1"
          onSubmit={(event) => {
            event.preventDefault()
            if (name.trim() !== '') save.mutate()
          }}
        >
          <label className="block text-xs text-[var(--color-muted)]">
            {strings.savedFilters.name}
            <input
              value={name}
              maxLength={80}
              onChange={(event) => setName(event.target.value)}
              className="mt-1 block w-full rounded-sm border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-sm text-[var(--color-text)]"
            />
          </label>
          <div className="flex gap-1">
            <button
              type="submit"
              disabled={save.isPending || name.trim() === ''}
              className="ease-mechanical min-h-6 rounded-sm border border-[var(--color-edge)] px-2 text-xs duration-[var(--duration-fast)] hover:text-[var(--color-bright)] disabled:opacity-50"
            >
              {strings.savedFilters.save}
            </button>
            <button
              type="button"
              onClick={() => {
                setNaming(false)
                setRefusal(null)
              }}
              className="ease-mechanical min-h-6 rounded-sm px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
            >
              {strings.savedFilters.cancel}
            </button>
          </div>
          {refusal === null ? null : (
            <p role="alert" className="text-xs text-[var(--color-muted)]">
              {refusal}
            </p>
          )}
        </form>
      ) : (
        <button
          type="button"
          onClick={() => setNaming(true)}
          className="ease-mechanical mt-1 min-h-6 rounded-sm px-2 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          {strings.savedFilters.saveThis}
        </button>
      )}
    </section>
  )
}

/** The two ways a saved filter moves, named once rather than spelled inside the JSX. */
const UP: MoveDirection = 'up'

const DOWN: MoveDirection = 'down'

/**
 * The grid opened, by a saved filter or an old link, on a filter the library no longer holds: a category
 * deleted since, or a field it no longer filters by. It says so, where an empty grid would claim nothing
 * is filed there yet and a failed one would blame the server, and offers the same filters without it.
 */
function FilterGone({
  text,
  onWiden,
}: {
  text: { title: string; body: string; widen: string }
  onWiden: () => void
}) {
  return (
    <section role="status" className="max-w-prose">
      <h2 className="text-[15px] font-semibold text-[var(--color-bright)]">{text.title}</h2>
      <p className="mt-1 text-sm text-[var(--color-muted)]">{text.body}</p>
      <button
        type="button"
        onClick={onWiden}
        className="ease-mechanical mt-3 min-h-6 rounded-sm border border-[var(--color-edge)] px-2 text-sm duration-[var(--duration-fast)] hover:text-[var(--color-bright)]"
      >
        {text.widen}
      </button>
    </section>
  )
}

/** The refusals of a field filter itself, which the grid names. Any other failure of the facets says so. */
const FIELD_REFUSALS: readonly string[] = ['notAFilter', 'wrongType', 'notARange', 'valueAndRange', 'emptyRange']

const FILTER_KEYS = ['q', 'folderId', 'format', 'material', 'tag', 'field', 'fieldValue', 'fieldMin', 'fieldMax'] as const

/** The grid's filters as a saved filter holds them: only the ones that are set. */
function filtersOf(filters: { [key in (typeof FILTER_KEYS)[number]]?: string }): FilterSearch {
  const set: FilterSearch = {}
  for (const key of FILTER_KEYS) {
    const value = filters[key]
    if (value !== undefined) set[key] = value
  }
  return set
}

/** Whether two sets of filters narrow the grid the same way. */
function sameFilters(a: FilterSearch, b: FilterSearch): boolean {
  return FILTER_KEYS.every((key) => a[key] === b[key])
}

/**
 * The facets beside the grid: the formats, the materials and the tags among the parts it shows for
 * the same category and query, as buttons that narrow the grid to one.
 *
 * Each list's counts follow the other lists' choices and never their own. Counts that obeyed their
 * own choice would show every other value as zero, and nobody could choose a second one; counts
 * that ignored the other choice would offer parts the grid is not showing. The server applies the
 * rule, and both choices ride on the request so it can.
 */
function Facets({
  library,
  folderId,
  q,
  format,
  material,
  tag,
  field,
  fieldValue,
  fieldMin,
  fieldMax,
  onSelectFormat,
  onSelectMaterial,
  onSelectTag,
  onSelectField,
}: {
  library: LibraryId
  folderId?: string
  q?: string
  format?: string
  material?: string
  tag?: string
  field?: string
  fieldValue?: string
  fieldMin?: string
  fieldMax?: string
  onSelectFormat: (format: string | null) => void
  onSelectMaterial: (material: string | null) => void
  onSelectTag: (tag: string | null) => void
  onSelectField: (field: string | null, value: string | null, range?: { min?: string; max?: string }) => void
}) {
  const facets = useQuery({
    queryKey: ['facets', library, folderId ?? null, q ?? null, format ?? null, material ?? null, tag ?? null, field ?? null, fieldValue ?? null, fieldMin ?? null, fieldMax ?? null],
    queryFn: () => fetchFacets(library, folderId, q, format, material, tag, field, fieldValue, fieldMin, fieldMax),
  })
  if (facets.isError) {
    // A field filter the server refuses fails the facets with the grid, and the grid says why and offers the way
    // out, where "reload to try again" would not help.
    if (field !== undefined && facets.error instanceof RefusedError && FIELD_REFUSALS.includes(facets.error.reason ?? '')) {
      return null
    }
    return (
      <p role="alert" className="mb-6 text-xs text-[var(--color-muted)]">
        {strings.facets.failed}
      </p>
    )
  }
  if (facets.data === undefined) return null
  return (
    <>
      <FacetList
        id="format-facet"
        title={strings.facets.format}
        values={facets.data.formats}
        selected={format}
        onSelect={onSelectFormat}
        name={strings.facets.name}
        option={strings.facets.option}
      />
      <FacetList
        id="material-facet"
        title={strings.facets.material}
        // `?? []` for a server from before the material facet, which answers formats alone.
        values={facets.data.materials ?? []}
        selected={material}
        onSelect={onSelectMaterial}
        name={(value) => value}
        option={strings.facets.materialOption}
      />
      <FacetList
        id="tag-facet"
        title={strings.facets.tag}
        // `?? []` for a server from before tags, which answers without them.
        values={facets.data.tags ?? []}
        selected={tag}
        onSelect={onSelectTag}
        name={(value) => value}
        option={strings.facets.tagOption}
      />
      <FieldFilters
        library={library}
        field={field}
        fieldValue={fieldValue}
        fieldMin={fieldMin}
        fieldMax={fieldMax}
        // `?? []` for a server from before counts per choice, which answers without them.
        counts={facets.data.fields ?? []}
        onSelect={onSelectField}
      />
    </>
  )
}

/**
 * One facet's values. Past the server's exact-count threshold a value arrives with no count, and is
 * shown without one rather than with a guess. A chosen value the current query no longer matches
 * stays on the list at zero, so the choice can still be undone from where it was made. A facet
 * with nothing to offer — materials, in a library of meshes — is not drawn at all.
 */
function FacetList({
  id,
  title,
  values,
  selected,
  onSelect,
  name,
  option,
}: {
  id: string
  title: string
  values: readonly FacetValue[]
  selected?: string
  onSelect: (value: string | null) => void
  name: (value: string) => string
  option: (value: string, count: number | null) => string
}) {
  const shown =
    selected === undefined || values.some(({ value }) => value === selected)
      ? values
      : [...values, { value: selected, count: 0 }]
  if (shown.length === 0) return null
  return (
    <section aria-labelledby={id} className="mb-6">
      <h2 id={id} className="mb-2 text-xs tracking-wider text-[var(--color-muted)] uppercase">
        {title}
      </h2>
      <ul role="list" className="space-y-0.5">
        {shown.map(({ value, count }) => (
          <li key={value}>
            <button
              type="button"
              aria-pressed={selected === value}
              aria-label={option(value, count)}
              onClick={() => onSelect(selected === value ? null : value)}
              className="ease-mechanical flex min-h-6 w-full items-center justify-between gap-2 rounded-sm px-2 text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-raised)] aria-pressed:bg-[var(--color-raised)] aria-pressed:text-[var(--color-bright)]"
            >
              <span>{name(value)}</span>
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
