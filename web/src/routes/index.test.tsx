import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { Index } from './index'
import { routeTree } from '../routeTree.gen'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { BatchStatus, FolderNode, LibraryStorage, PartCard, PartsPage } from '../lib/types'

/**
 * `Index` takes the batch as a prop rather than reading the search param itself, which is
 * what lets these tests render it with no router in scope. The route component does the
 * `useSearch()` half; see `index.tsx`.
 */
function renderIndex(
  props: {
    batch?: string
    folderId?: string
    onSelectFolder?: (folder: string | null) => void
    client?: QueryClient
  } = {},
) {
  const client = props.client ?? newClient()
  return render(
    <QueryClientProvider client={client}>
      <Index batch={props.batch} folderId={props.folderId} onSelectFolder={props.onSelectFolder} />
    </QueryClientProvider>,
  )
}

/**
 * Taken as a parameter so a test can spy on the client's `invalidateQueries` *before* the
 * component mounts, rather than racing the first effect.
 */
function newClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

/** The subset of `Response` these tests hand back. */
type StubResponse = { ok: boolean; status?: number; json?: () => Promise<unknown> }

/** Never settles, which holds the query that made the request in its pending state. */
const pending = () => new Promise<StubResponse>(() => {})

const ok = (body: unknown) => async (): Promise<StubResponse> => ({ ok: true, json: async () => body })

/**
 * The page makes two requests through one `fetch`, so a blanket mock would feed the
 * healthz body to the parts query and vice versa. Dispatch on the URL instead. Routes
 * left unstubbed hang rather than resolve, so a test never accidentally asserts against
 * a body it did not ask for, and an unrecognised path rejects loudly instead of quietly
 * pending forever.
 */
function stubFetch(routes: {
  healthz?: () => Promise<StubResponse>
  parts?: () => Promise<StubResponse>
  batch?: () => Promise<StubResponse>
  library?: () => Promise<StubResponse>
  storage?: () => Promise<StubResponse>
  settings?: () => Promise<StubResponse>
  sweep?: () => Promise<StubResponse>
  partThumbnail?: () => Promise<StubResponse>
  scan?: () => Promise<StubResponse>
  folders?: () => Promise<StubResponse>
  move?: () => Promise<StubResponse>
  folderDelete?: () => Promise<StubResponse>
}) {
  const fetchMock = vi.fn((url: string, init?: { method?: string }) => {
    if (url.startsWith('/api/healthz')) return (routes.healthz ?? pending)()
    // Above the settings rule below, which claims every `PATCH` there is. The move is a
    // `PATCH` too, and answering it with a `LibrarySettings` body would leave a move test
    // asserting against a shape it never asked for.
    if (init?.method === 'PATCH' && url.startsWith('/api/parts/')) return (routes.move ?? pending)()
    if (url.startsWith('/api/folders/')) return (routes.folderDelete ?? pending)()
    // The one route distinguished by method rather than path: `PATCH /api/libraries/{id}`
    // is a prefix of every other library route.
    if (init?.method === 'PATCH') return (routes.settings ?? pending)()
    // Order matters twice over. `/thumbnails` is a suffix of nothing else but `/thumbnail`
    // is a suffix of it, and the per-card route is `/api/parts/{id}/thumbnail` — which the
    // earlier `includes('/parts')` rule would have answered with a page of the grid.
    if (url.endsWith('/thumbnails')) return (routes.sweep ?? pending)()
    if (url.endsWith('/thumbnail')) return (routes.partThumbnail ?? pending)()
    // `endsWith` alone stops matching the moment the grid filters by a category, and the
    // request then falls through to the bare-library rule — a settings body where a page
    // of parts was expected.
    if (url.endsWith('/parts') || url.includes('/parts?')) return (routes.parts ?? pending)()
    if (url.endsWith('/scan')) return (routes.scan ?? pending)()
    // Before the bare-library rule, which every library route is a prefix of.
    if (url.endsWith('/folders')) return (routes.folders ?? pending)()
    // Before the bare-library rule below, which every library route is a prefix of.
    if (url.endsWith('/storage')) return (routes.storage ?? pending)()
    if (url.includes('/jobs/')) return (routes.batch ?? pending)()
    // Last of the library routes, because the settings read is the bare path every one of
    // the others is built on. Unstubbed it hangs like the rest, which is what leaves the
    // toggle in its unknown state for every test that is not about it.
    if (url.startsWith('/api/libraries/')) return (routes.library ?? pending)()
    return Promise.reject(new Error(`unstubbed request: ${url}`))
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

/** A batch id shaped like the uuid v7 `enqueue_scan` issues. */
const BATCH_ID = '01a0699a-9ece-7073-a74b-c977ee7335ff'

/**
 * The batch id a trigger route hands back. Deliberately not `BATCH_ID`: the assertions
 * below check that the id from the `202` is the one polled, which cannot fail if the
 * page could have got the same id from anywhere else.
 */
const RENDER_BATCH_ID = '01a069c4-1d3e-7a10-b6f2-4f0c8b2d5e91'

/** The same, for the scan button, and distinct for the same reason. */
const SCAN_BATCH_ID = '01a06a11-77b2-7c4d-9f18-2ab6e0c31d45'

/** A `BatchStatus` as the API sends it, with the counters a test cares about overridden. */
const batchStatus = (over: Partial<BatchStatus> = {}): BatchStatus => ({
  batchId: BATCH_ID,
  libraryId: DEFAULT_LIBRARY_ID,
  total: 6,
  scanned: 0,
  pending: 6,
  running: 0,
  ingested: 0,
  skipped: 0,
  rendered: 0,
  migrated: 0,
  migrating: 0,
  failedTotal: 0,
  failed: [],
  startedAt: '2026-09-03T23:28:56.014618Z',
  finishedAt: null,
  ...over,
})

/**
 * Two real lossless WebPs — 2x2 blue and 4x2 orange — base64'd exactly as
 * `PartCard.thumbnail` carries them. They differ deliberately: when every fixture shared
 * one payload, `src === part.thumbnail` compared identical strings, so a card rendering
 * `parts[0].thumbnail` for every part — every user seeing the wrong preview — passed the
 * whole suite. Distinct bytes are what make the card-to-thumbnail association testable.
 */
const WEBP_BLUE = 'data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvAUAAAAdQ1LrVv/+BiOh/AAA='
const WEBP_ORANGE = 'data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvA0AAAAdQvJoXpf+BiOh/AAA='

const MOTOR_MOUNT: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0001',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0001',
  name: 'NEMA 17 motor mount, 42 mm face',
  partNumber: 'LP-3105-A',
  thumbnail: WEBP_BLUE,
  triangleCount: 12486,
  approximate: true,
  sourceHash: '33237f7971cb1497a5417c667e9a459c240943c7378b44fcd4f6404590363895',
  sourceBytes: 624_384,
  storedBytes: 197_012,
  compressed: true,
  directory: 'libraries/default/Motors/NEMA 17 motor mount, 42 mm face',
  createdAt: '2026-08-14T09:12:44Z',
  updatedAt: '2026-08-14T09:12:44Z',
}

const HEX_NUT: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0002',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0002',
  name: 'Hex nut M8, DIN 934',
  partNumber: 'DIN934-M8-A2',
  thumbnail: WEBP_ORANGE,
  triangleCount: 1984,
  approximate: true,
  sourceHash: 'a0763a33d499b598864ddd26eeca15f6d9794ce44185883fd969888941de365d',
  sourceBytes: 99_284,
  storedBytes: 26_741,
  compressed: true,
  directory: 'libraries/default/Fasteners/Hex nut M8, DIN 934',
  createdAt: '2026-08-14T09:12:51Z',
  updatedAt: '2026-08-14T09:12:51Z',
}

/**
 * Ingested, but the worker has not rasterized a thumbnail derivative for it yet. Also
 * the only 3MF of the three, so it is the fixture that carries the `AsIs` storage
 * shape: a 3MF is already a zip, the ingest policy stores it uncompressed, and its two
 * sizes agree. The other two would let a card that rendered `sourceBytes` where
 * `storedBytes` belongs pass unnoticed.
 */
const SHAFT_COUPLER: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0003',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0003',
  name: 'Flexible shaft coupler, 5 mm to 8 mm',
  partNumber: 'LP-4420-B',
  thumbnail: null,
  triangleCount: 7320,
  approximate: true,
  sourceHash: 'c6b1d88498005800fb68ccc2f54588d00bbc1603243fcef5ef8f8000d1be2a70',
  sourceBytes: 148_930,
  storedBytes: 148_930,
  compressed: false,
  directory: 'libraries/default/Couplers/Flexible shaft coupler, 5 mm to 8 mm',
  createdAt: '2026-08-14T09:13:02Z',
  updatedAt: '2026-08-14T09:13:02Z',
}

const page = (parts: PartCard[]): PartsPage => ({ parts, next: null })

beforeEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

// For these health-check states, the expected text is written out literally rather than
// read from strings.ts. Asserting against the same constant the component renders from
// would pass even if the string were corrupted, because both sides would move together.
test('renders the connected state from a healthy response', async () => {
  const fetchMock = stubFetch({
    healthz: ok({ status: 'ok', database: { major: 18, reachable: true } }),
  })
  renderIndex()
  expect(await screen.findByText('Connected — PostgreSQL 18')).toBeDefined()
  // Pin the endpoint. The stub resolves regardless of what it is called with, so without
  // this a typo in the path would be invisible.
  expect(fetchMock).toHaveBeenCalledWith('/api/healthz')
})

test('renders the checking state while the request is in flight', () => {
  stubFetch({})
  renderIndex()
  expect(screen.getByText('Checking the server…')).toBeDefined()
})

test('renders an actionable message when the server is unreachable', async () => {
  stubFetch({ healthz: async () => ({ ok: false, status: 503 }) })
  renderIndex()
  expect(
    await screen.findByText('Could not reach the server. Check that the api and db services are running.'),
  ).toBeDefined()
})

// Asserted against the strings.ts constant, not a literal: React Testing Library compares
// rendered text, not import provenance, so this still passes if the component is hardcoded
// to today's copy. It catches the component drifting from strings.ts later, when the copy
// next changes and the component does not follow — a wiring test, not a content pin.
// Provenance itself is enforced at the source level, in no-bare-strings.test.ts.
test('renders the empty-library copy from strings.ts', async () => {
  stubFetch({ parts: ok(page([])) })
  renderIndex()
  expect(await screen.findByText(strings.emptyLibrary.title)).toBeDefined()
  expect(screen.getByText(strings.emptyLibrary.body)).toBeDefined()
})

// Ingest is a server-side scan over a mounted directory. The grid has no upload control
// and slice 1 has no endpoint that would give it one, so copy that sends the user looking
// for one is a wrong instruction, not a harmless flourish. Read out of the DOM rather
// than off the constants, so a hardcoded prompt in the component is caught too, and
// phrased as an invariant so it survives a rewording.
test('the empty state points at no upload control, because there is none', async () => {
  stubFetch({ parts: ok(page([])) })
  renderIndex()
  await screen.findByText(strings.emptyLibrary.body)
  const rendered = (document.body.textContent ?? '').toLowerCase()
  expect(rendered.length).toBeGreaterThan(40)
  const claims = ['upload', 'drag', 'drop', 'browse', 'choose a file', 'add file']
  for (const claim of claims) {
    expect(rendered).not.toContain(claim)
  }
  // Narrowed from "no buttons at all" once the action bar landed: the page now offers
  // controls that act on parts already ingested, and those are not upload controls. What
  // still must not exist is a control that promises to take a file — checked by
  // accessible name, so an icon-only button labelled only by `aria-label` is covered too,
  // which the blanket assertion this replaces would have missed.
  for (const control of screen.queryAllByRole('button')) {
    const name = (control.getAttribute('aria-label') ?? control.textContent ?? '').toLowerCase()
    for (const claim of claims) {
      expect(name).not.toContain(claim)
    }
  }
  expect(document.querySelector('input[type="file"]')).toBeNull()
})

// "We have not asked yet" and "we asked and there is nothing" are different facts, and
// only the second one is the empty state. Without this, a component that renders the
// empty state during the request still passes every other test here, because the pages
// they mock all resolve.
test('does not claim the library is empty while the request is still in flight', () => {
  stubFetch({})
  renderIndex()
  expect(screen.getByText(strings.parts.loading)).toBeDefined()
  expect(screen.queryByText(strings.emptyLibrary.title)).toBeNull()
})

test('does not show the empty state when the library has parts', async () => {
  stubFetch({ parts: ok(page([MOTOR_MOUNT])) })
  renderIndex()
  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(screen.queryByText(strings.emptyLibrary.title)).toBeNull()
  expect(screen.queryByText(strings.emptyLibrary.body)).toBeNull()
})

test('renders a card per part with that part own thumbnail bytes inline', async () => {
  const fetchMock = stubFetch({ parts: ok(page([MOTOR_MOUNT, HEX_NUT])) })
  renderIndex()

  const mount = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  const nut = screen.getByRole('article', { name: HEX_NUT.name })
  expect(within(mount).getByText(MOTOR_MOUNT.partNumber!)).toBeDefined()
  expect(within(nut).getByText(HEX_NUT.partNumber!)).toBeDefined()

  // The two payloads differ, so this is an association assertion and not merely a
  // presence one: a card wired to the first part's thumbnail fails on the second card.
  expect(within(mount).getByRole('img').getAttribute('src')).toBe(WEBP_BLUE)
  expect(within(nut).getByRole('img').getAttribute('src')).toBe(WEBP_ORANGE)
  for (const card of [mount, nut]) {
    expect(within(card).getByRole('img').getAttribute('src')).toMatch(
      /^data:image\/webp;base64,[A-Za-z0-9+/=]+$/,
    )
  }

  // Alt text literal, not read back from strings.ts: an alt of '' or 'image' would still
  // satisfy the constant-based form, and the description is what a screen-reader user
  // gets instead of the render.
  expect(within(mount).getByRole('img').getAttribute('alt')).toBe(
    'Rendered preview of NEMA 17 motor mount, 42 mm face',
  )
  expect(within(nut).getByRole('img').getAttribute('alt')).toBe('Rendered preview of Hex nut M8, DIN 934')

  // Keyset paging is not wired yet, but the library in the path is: pin it, since the
  // stub answers any URL containing "/parts".
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/libraries/01931b6e-0000-7000-8000-000000000001/parts',
  )
})

test('shows a placeholder instead of an empty image when a part has no thumbnail', async () => {
  stubFetch({ parts: ok(page([SHAFT_COUPLER])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: SHAFT_COUPLER.name })
  expect(within(card).queryByRole('img')).toBeNull()
  expect(within(card).getByText(strings.parts.noThumbnail)).toBeDefined()
})

// A page is a mix in practice — the worker rasterizes as it goes — and the two cases were
// only ever rendered alone. A missing thumbnail must not shift the neighbouring card's
// bytes onto the wrong part, nor suppress the render of the part that does have one.
test('renders a thumbnailed part and a thumbnail-less part side by side', async () => {
  stubFetch({ parts: ok(page([SHAFT_COUPLER, HEX_NUT])) })
  renderIndex()

  const coupler = await screen.findByRole('article', { name: SHAFT_COUPLER.name })
  const nut = screen.getByRole('article', { name: HEX_NUT.name })
  expect(within(coupler).queryByRole('img')).toBeNull()
  expect(within(coupler).getByText(strings.parts.noThumbnail)).toBeDefined()
  expect(within(nut).getByRole('img').getAttribute('src')).toBe(WEBP_ORANGE)
  expect(within(nut).queryByText(strings.parts.noThumbnail)).toBeNull()
  expect(screen.getAllByRole('img')).toHaveLength(1)
})

// CLAUDE.md: mesh-derived measurements are labelled "approximate" in the UI, always. The
// triangle count is tessellation-derived by construction, so it must never appear
// unlabelled. Both strings are literals: this is the exact wording the non-negotiable
// exists to produce, and a badge reading "Exact" over a mesh figure is the failure mode.
test('shows the triangle count only alongside the approximate label', async () => {
  stubFetch({ parts: ok(page([MOTOR_MOUNT])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(within(card).getByText('12,486 triangles')).toBeDefined()
  const badge = within(card).getByText('Approximate')
  expect(badge.getAttribute('title')).toBe(
    'At least one figure on this part is measured from tessellated geometry rather than from analytic CAD entities.',
  )
})

// The one case that made the rule an accident rather than a guarantee: a part carrying a
// triangle count while the wire says approximate=false. No fixture paired those, and the
// count and the badge were independent conditionals, so the count rendered unlabelled —
// latent only because the ingest path currently hardcodes the flag to true. A triangle
// count IS a mesh-derived figure, so the label is not optional here; the component makes
// the pair indivisible rather than trusting the flag.
test('labels a triangle count even when the wire claims the part is not approximate', async () => {
  const inconsistent: PartCard = { ...MOTOR_MOUNT, approximate: false }
  stubFetch({ parts: ok(page([inconsistent])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: inconsistent.name })
  expect(within(card).getByText('12,486 triangles')).toBeDefined()
  expect(within(card).getByText('Approximate')).toBeDefined()
})

// strings.parts.triangles has a singular branch, and nothing exercised it: every fixture
// carried a plural count, so inlining the formatter as
// `{n.toLocaleString('en-US')} triangles` would render "1 triangles" to a user with all
// tests green. A single-facet mesh is what a conformance probe looks like in a real
// library, so the fixture is not contrived.
test('renders the singular form for a one-triangle mesh', async () => {
  const singleFacet: PartCard = {
    ...MOTOR_MOUNT,
    id: '01931b6e-0000-7000-8000-0000000a0005',
    name: 'STL conformance probe, single facet',
    partNumber: 'LP-0001-T',
    triangleCount: 1,
  }
  stubFetch({ parts: ok(page([singleFacet])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: singleFacet.name })
  expect(within(card).getByText('1 triangle')).toBeDefined()
  expect(within(card).queryByText('1 triangles')).toBeNull()
  expect(within(card).getByText('Approximate')).toBeDefined()
})

// The binding says `number | null`, but fetchParts casts the response rather than
// validating it, so a field the server stops sending arrives as undefined and reaches
// the formatter. The card must degrade to "no count" rather than throwing the whole grid
// away; the cast here is the point of the test, not an oversight.
test('survives a triangle count the server stopped sending', async () => {
  const drifted = { ...MOTOR_MOUNT, triangleCount: undefined } as unknown as PartCard
  stubFetch({ parts: ok(page([drifted])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(within(card).queryByText(/triangle/)).toBeNull()
  expect(within(card).getByText('Approximate')).toBeDefined()
})

// The flag means "any figure on this part is mesh-derived", so a part can be approximate
// with no count on the card at all. Both fixtures here withhold the count, which is what
// distinguishes the two mistakes: keying the label to triangleCount alone would drop it
// from the first card, and keying it to nothing would add it to the second.
test('labels a mesh-derived part approximate and leaves an analytic one unlabelled', async () => {
  const meshDerived: PartCard = {
    ...SHAFT_COUPLER,
    name: 'Bearing block SK8, 8 mm shaft',
    partNumber: 'SK8-01',
    triangleCount: null,
    approximate: true,
  }
  const analytic: PartCard = {
    ...SHAFT_COUPLER,
    id: '01931b6e-0000-7000-8000-0000000a0004',
    name: 'Sensor bracket, 20 x 40 extrusion',
    partNumber: 'LP-2210-C',
    triangleCount: null,
    approximate: false,
  }
  stubFetch({ parts: ok(page([meshDerived, analytic])) })
  renderIndex()

  const mesh = await screen.findByRole('article', { name: meshDerived.name })
  const brep = screen.getByRole('article', { name: analytic.name })
  expect(within(mesh).getByText('Approximate')).toBeDefined()
  expect(within(brep).queryByText('Approximate')).toBeNull()
})

// Literal, not the constant, for the same reason the health failure above is literal:
// this copy has to say what broke *and* what to do about it, and an assertion that reads
// from strings.ts moves with any edit that guts it.
test('renders an actionable message when the parts request fails', async () => {
  stubFetch({ parts: async () => ({ ok: false, status: 500 }) })
  renderIndex()
  expect(
    await screen.findByText(
      'Could not load the parts in this library. Check that the api service is running, then reload.',
    ),
  ).toBeDefined()
})

// The grid asks for one page and the server caps it at 50, so a user who scans 200 STLs
// sees 50 cards. Nothing on the page said so: they scroll to the bottom, do not find the
// part they came for, and conclude it was never ingested. Literals, not the constants —
// this copy exists to state a specific fact, and reading it back from strings.ts would
// pass just as happily against wording that no longer states it.
test('says how much of the library is on screen when the whole of it fits', async () => {
  stubFetch({ parts: ok(page([MOTOR_MOUNT, HEX_NUT])) })
  renderIndex()
  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(screen.getByText('Showing all 2 parts.')).toBeDefined()
})

test('says the grid is truncated when the server hands back another cursor', async () => {
  // `next` non-null is the server's own "there is more behind this page" — a full page
  // hands back a cursor, a short one hands back null.
  const truncated: PartsPage = {
    parts: [MOTOR_MOUNT, HEX_NUT],
    next: '01931b6e-0000-7000-8000-0000000a0002',
  }
  stubFetch({ parts: ok(truncated) })
  renderIndex()
  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(
    screen.getByText(
      'Showing the first 2 parts. This library has more — paging through them arrives with the virtualized grid.',
    ),
  ).toBeDefined()
  // And it must not also claim to be showing all of them.
  expect(screen.queryByText('Showing all 2 parts.')).toBeNull()
})

test('does not count parts before the page has arrived, or when there are none', async () => {
  // A count rendered against a pending or empty page is a claim about a library nothing
  // has looked at yet. The empty state already says what is true there.
  stubFetch({})
  const pendingRender = renderIndex()
  expect(screen.queryByText(/^Showing/)).toBeNull()
  pendingRender.unmount()

  stubFetch({ parts: ok(page([])) })
  renderIndex()
  await screen.findByText(strings.emptyLibrary.title)
  expect(screen.queryByText(/^Showing/)).toBeNull()
})

const HEALTHY = { status: 'ok', database: { major: 18, reachable: true } }

test('shows how far a running scan has got', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    // Seven jobs: the walk, which is done, and the six files it found.
    batch: ok(batchStatus({ total: 7, scanned: 1, pending: 3, running: 1, ingested: 2 })),
  })
  renderIndex({ batch: BATCH_ID })

  // Literal, never `strings.scan.running(2, 6)`: an assertion built from the same
  // template it is checking compares the template against itself and passes whatever
  // numbers, nouns or word order the template grows. That is how "1 of 4 files" for a
  // three-file folder survived a green suite.
  expect(await screen.findByText('Scanning — 2 of 6 files.')).toBeTruthy()
  expect(fetchMock).toHaveBeenCalledWith(
    `/api/libraries/${DEFAULT_LIBRARY_ID}/jobs/${BATCH_ID}`,
  )
})

test('reports files that could not be read alongside the progress', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(
      batchStatus({
        total: 6,
        pending: 0,
        ingested: 5,
        failedTotal: 1,
        failed: [{ path: 'truncated-lp-9999-00.stl', reason: 'truncated', attempts: 1 }],
        finishedAt: '2026-09-03T23:28:56.086374Z',
      }),
    ),
  })
  renderIndex({ batch: BATCH_ID })

  // The count and the reason are different claims and both belong on screen — see the
  // test below for why the reason is the one that cannot be dropped.
  expect(await screen.findByText(strings.scan.failed(1))).toBeTruthy()
})

// What makes moving the directory walk into a job honest rather than merely convenient.
// The walk used to run inside the request, so an unreadable `/ingest` mount answered the
// operator with a 500 naming the mount; it now fails a job in this batch instead, and
// `batch_status` has carried that message in `failures[].last_error` since slice 2 with
// nothing rendering it. A count alone ("1 file could not be read") cannot tell anyone the
// mount is missing, which is the failure this whole shape has to stay visible for.
test('a failed job shows the reason it failed, not only that it failed', async () => {
  const reason =
    'Could not read the ingest directory /ingest: No such file or directory (os error 2). ' +
    'Check that the mount is present and readable on the worker, then start the scan again.'
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(
      batchStatus({
        total: 1,
        pending: 0,
        failedTotal: 1,
        // A `scan_directory` job has no path: it is the directory that failed, and the
        // reason names it. `COALESCE(payload->>'path', p.name, '')` gives back `''`.
        failed: [{ path: '', reason, attempts: 1 }],
        finishedAt: '2026-09-05T09:14:02.114Z',
      }),
    ),
  })
  renderIndex({ batch: BATCH_ID })

  // `findByText` throws when absent, so this cannot pass for a reason that never rendered.
  expect(await screen.findByText(reason)).toBeTruthy()
})

// A per-file failure keeps its path, and the two halves must both reach the screen: the
// reason alone does not say which file, and slice 2's `path` column exists for that.
test('a per-file failure names the file alongside the reason', async () => {
  const failure = {
    path: 'spacer-lp-2001-00.stl',
    reason:
      'Could not read this STL — it declares 24 facets but the file ends after 11. ' +
      'Re-export from your CAD tool and retry.',
    attempts: 3,
  }
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(
      batchStatus({
        total: 2,
        pending: 0,
        ingested: 1,
        failedTotal: 1,
        failed: [failure],
        finishedAt: '2026-09-05T09:14:02.114Z',
      }),
    ),
  })
  renderIndex({ batch: BATCH_ID })

  expect(
    await screen.findByText(strings.failure.line(failure.path, failure.reason)),
  ).toBeTruthy()
})

// The server caps `failed` at 100 while `failedTotal` is the real number. A list that
// simply stops is a measurement that lies by omission, which is the same fault the
// truncated grid needs `parts.showingFirstPage` for.
test('a failure list capped by the server says how many it is not showing', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(
      batchStatus({
        total: 150,
        pending: 0,
        ingested: 30,
        failedTotal: 120,
        failed: [{ path: 'vee-block-lp-4410-01.stl', reason: 'not watertight', attempts: 1 }],
        finishedAt: '2026-09-05T09:14:02.114Z',
      }),
    ),
  })
  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.failure.more(119))).toBeTruthy()
})

test('stops polling once the batch reports it finished', async () => {
  // Mutation guard for spec §11's last risk. With `refetchInterval` left as a constant
  // instead of returning false on `finishedAt`, the count keeps climbing after the batch
  // is done and a backgrounded tab asks about a completed scan forever.
  let calls = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: async () => {
      calls += 1
      return {
        ok: true,
        json: async () =>
          calls === 1
            ? batchStatus({ pending: 4, ingested: 2 })
            : batchStatus({
                pending: 0,
                ingested: 6,
                finishedAt: '2026-09-03T23:28:56.086374Z',
              }),
      }
    },
  })
  renderIndex({ batch: BATCH_ID })

  // Testing Library's default findBy timeout is 1000ms, exactly the poll interval, so the
  // second poll and the timeout race each other. Give it headroom rather than shortening
  // the interval, which is the thing under test.
  await screen.findByText(strings.scan.finished(6, 0), undefined, { timeout: 4000 })
  const afterFinish = calls

  // Longer than the 1000ms poll interval, so a poll that never stopped would show up here.
  await new Promise((resolve) => setTimeout(resolve, 1400))
  expect(calls).toBe(afterFinish)
})

test('does not ask about a batch when the URL names none', async () => {
  const fetchMock = stubFetch({ healthz: ok(HEALTHY), parts: ok(page([])) })
  renderIndex()

  await screen.findByText(strings.emptyLibrary.title)
  const asked = fetchMock.mock.calls.some(([url]) => url.includes('/jobs/'))
  expect(asked).toBe(false)
})

test('says so when the batch in the URL is not one this library can show', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: async () => ({ ok: false, status: 404 }),
  })
  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.scan.unknown)).toBeTruthy()
})

test('refetches the grid as the worker commits parts', async () => {
  // "The grid fills in as the worker commits parts" (spec §10) is a claim about two
  // queries, not one: the batch poll sees progress, and the parts query — a separate
  // cache entry that nothing else would invalidate — has to be told the library changed
  // underneath it. Without that, a scan finishes and the grid still says empty until the
  // user reloads.
  let partsCalls = 0
  let batchCalls = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: async () => {
      partsCalls += 1
      return { ok: true, json: async () => (partsCalls === 1 ? page([]) : page([MOTOR_MOUNT])) }
    },
    batch: async () => {
      batchCalls += 1
      return {
        ok: true,
        json: async () =>
          batchCalls === 1
            ? batchStatus({ pending: 6, ingested: 0 })
            : batchStatus({ pending: 4, ingested: 2 }),
      }
    },
  })
  renderIndex({ batch: BATCH_ID })

  // Nothing has settled on the first poll, so the grid is left alone and stays empty.
  await screen.findByText(strings.emptyLibrary.title)
  expect(partsCalls).toBe(1)

  // The second poll reports two files in, and the card arrives without a reload.
  expect(
    await screen.findByRole('article', { name: MOTOR_MOUNT.name }, { timeout: 4000 }),
  ).toBeTruthy()
  expect(partsCalls).toBeGreaterThan(1)
})

// The test this task exists for. `Outcome::Rendered` is a terminal outcome the settled
// count did not know about, and a thumbnail sweep settles EVERY job as `rendered` — so a
// count that omits it reads 0 for the whole batch, the invalidation never fires, and the
// grid keeps showing "No preview yet" over parts whose previews are rendered and stored.
// No backend test can see that: every job succeeded and every row is correct.
//
// The fixture is the load-bearing part. `ingested`, `skipped` and `failedTotal` are all
// zero on purpose — the obvious fixture, a batch with a couple of ingests in it, passes
// happily with `rendered` dropped from the count, which is exactly the drift this exists
// to catch. The assertion below pins that property so a later edit cannot quietly restore
// it. Driven from `?batch=` rather than from the sweep button, so nothing but the settled
// count can be what refetches the grid.
test('a batch whose jobs all settle as rendered refetches the grid exactly once', async () => {
  const swept = batchStatus({
    total: 4,
    pending: 0,
    running: 0,
    rendered: 4,
    finishedAt: '2026-09-05T10:14:02.116Z',
  })
  expect(swept.ingested + swept.skipped + swept.failedTotal).toBe(0)
  expect(swept.rendered).toBe(4)

  // Same part either side of the sweep: without a preview, then with one. That is the
  // user-visible symptom the count controls — a blank card that never fills in.
  const rendered: PartCard = { ...SHAFT_COUPLER, thumbnail: WEBP_ORANGE }
  let partsCalls = 0
  let batchCalls = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: async () => {
      partsCalls += 1
      return {
        ok: true,
        json: async () => (partsCalls === 1 ? page([SHAFT_COUPLER]) : page([rendered])),
      }
    },
    batch: async () => {
      batchCalls += 1
      return {
        ok: true,
        json: async () => (batchCalls === 1 ? batchStatus({ total: 4, pending: 4 }) : swept),
      }
    },
  })

  const client = newClient()
  // Spied before mount, and calling through, so the refetch it triggers still happens.
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  renderIndex({ batch: BATCH_ID, client })

  // Nothing has settled on the first poll: the card is there and still has no preview.
  const before = await screen.findByRole('article', { name: SHAFT_COUPLER.name })
  expect(within(before).getByText(strings.parts.noThumbnail)).toBeDefined()
  expect(partsCalls).toBe(1)

  // The second poll reports four renders, and the preview arrives with no reload.
  const card = await screen.findByRole('article', { name: SHAFT_COUPLER.name }, { timeout: 4000 })
  await waitFor(
    () => expect(within(card).getByRole('img').getAttribute('src')).toBe(WEBP_ORANGE),
    { timeout: 4000 },
  )
  expect(partsCalls).toBe(2)

  // And the line above the grid says what happened, in copy that is true of a render:
  // `strings.scan.finished` reads the ingest counters and would call these four rendered
  // previews "Scan complete — 0 added."
  expect(screen.getByText(strings.render.finished(4))).toBeDefined()

  // Exactly one, not "at least one": the effect is keyed on the settled count, so a poll
  // that reports the same numbers again must cost no refetch. Filtered by key, because
  // the count is the claim — an invalidation of some other cache entry is not this one.
  const partsInvalidations = invalidate.mock.calls.filter(([filters]) => {
    const key = (filters as { queryKey?: unknown } | undefined)?.queryKey
    return Array.isArray(key) && key[0] === 'parts'
  })
  expect(partsInvalidations).toHaveLength(1)
  expect(partsInvalidations[0]?.[0]).toEqual({ queryKey: ['parts', DEFAULT_LIBRARY_ID] })
})

/**
 * The blind window this fix round closes. `migrating` counts ROWS of kind
 * `migrate_storage`, not settled outcomes, so it is already 1 the instant the worker's
 * startup enqueue creates the batch — before anything has run. The worker queues a
 * migration on its own at startup, so this IS the ordinary shape of a migration batch
 * this page ever learns about, not an edge case: `migrated` (settled outcomes) would
 * still read 0 here, and if `kind` fell through to that, an operator watching their
 * files get relocated would see "Reading the folder…" for the whole first run —
 * worst on a large corpus, where that first run is slowest.
 */
test('a migration batch reads as a migration before its first job has settled', async () => {
  const running = batchStatus({
    total: 1,
    pending: 0,
    running: 1,
    migrating: 1,
    migrated: 0,
  })
  expect(running.finishedAt).toBeNull()

  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(running),
  })

  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.migrate.running)).toBeTruthy()
  expect(screen.queryByText('Reading the folder…')).toBeNull()
})

/**
 * The regression this task exists to close. Nothing on this page starts a migration —
 * the worker queues it on its own at startup — so the only way this page ever learns
 * about one is a batch id it never clicked into, exactly like the render sweep's own
 * `curl` gap above. Before `migrating` existed on `BatchStatus`, this batch's `rendered`
 * count was 0 just like a scan's, so `kind` fell through to `'scan'` and an operator
 * watching their files get relocated read "Scan complete — 3 added." over three files
 * that were only moved, not added.
 */
test('a batch whose jobs all settle as migrated reads as a migration, not a scan', async () => {
  const migrated = batchStatus({
    total: 3,
    pending: 0,
    running: 0,
    migrating: 3,
    migrated: 3,
    finishedAt: '2026-09-06T10:14:02.116Z',
  })
  expect(migrated.ingested + migrated.skipped + migrated.rendered + migrated.failedTotal).toBe(0)

  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(migrated),
  })

  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.migrate.finished(0))).toBeTruthy()
  // Neither of the other two kinds' copy leaked in — `scan.finished` would call three
  // moved files "added", and `render.finished` would call them rendered previews. Not a
  // bare `/preview/i` check: the action bar's own static copy ("Generate missing
  // previews", "Render preview") always contains that word and would make this
  // assertion fire on correct output.
  expect(screen.queryByText(/Scan complete/)).toBeNull()
  expect(screen.queryByText(strings.render.finished(3))).toBeNull()
})

/**
 * The copy a migration failure gets, and the copy it must never get.
 *
 * `strings.migrate` used to carry only `running` and `finished`, so a `migrate_storage`
 * job that failed fell through to `strings.scan` and told an operator that "1 file could
 * not be read. It will not appear in the grid" — about a model that already exists, is
 * already in the grid, and whose bytes were never touched. A migration relocates a file a
 * part already has; the worst a failure can do is leave it where it was. Wording a
 * non-destructive failure as data loss is the one class of copy mistake this product
 * treats as a correctness bug, so both halves are pinned here: the migration wording
 * present, and the scan wording absent.
 *
 * The completion line is checked in the same test because the two are one sentence to a
 * reader: `finished` asserted "this library's files are now in their model folders"
 * regardless of `failedTotal`, so a migration that moved four steps out of five claimed
 * every file was home directly beside a line saying one was not.
 */
test('a failed migration is not described in the words a failed scan uses', async () => {
  const reason =
    'The stored copy of Basalt cliff face, 180 mm span does not match the hash recorded \
for it — it reads as 4f6a91c2… where the database says 8b30d5ae… . The blob may have been \
corrupted or written by something other than Lapidary; re-scan this part from its source \
file. Nothing was moved or removed.'
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(
      batchStatus({
        total: 5,
        pending: 0,
        running: 0,
        migrating: 5,
        migrated: 4,
        failedTotal: 1,
        failed: [{ path: '', reason, attempts: 3 }],
        finishedAt: '2026-09-06T10:14:02.116Z',
      }),
    ),
  })
  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.migrate.failed(1))).toBeTruthy()
  // The sentence this whole finding is about: existing models described as about to
  // vanish from a grid they are already in.
  expect(screen.queryByText(strings.scan.failed(1))).toBeNull()
  // And the completion line qualified by the failure rather than talking over it.
  expect(screen.getByText(strings.migrate.finished(1))).toBeTruthy()
  expect(screen.queryByText(strings.migrate.finished(0))).toBeNull()
  // The reason itself still reaches the screen, which is where "1 step" gets its detail.
  expect(screen.getByText(strings.failure.line('', reason))).toBeTruthy()
})

/**
 * The other half of the same fall-through. A status poll that stops answering rendered
 * `scan.unknown` — "No scan with that id has run in this library" — to an operator who
 * never started a scan: nothing on this page can start a migration, the worker queues it
 * at boot, and the batch is one this browser was only ever watching.
 *
 * The poll has to succeed once before it can fail as a migration: `kind` is read off the
 * counters, so a first request that errors has nothing to read and falls back to `scan`,
 * which is correct there — a mistyped `?batch=` id genuinely is a scan id as far as
 * anything here can tell.
 */
test('a migration whose status stops answering does not report a scan nobody started', async () => {
  let calls = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: async () => {
      calls += 1
      return calls === 1
        ? { ok: true, json: async () => batchStatus({ total: 4, pending: 3, running: 1, migrating: 4 }) }
        : { ok: false, status: 404 }
    },
  })
  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.migrate.running)).toBeTruthy()
  // Past the 1000 ms poll interval: the second request is the one that fails.
  expect(await screen.findByText(strings.migrate.unknown, {}, { timeout: 4000 })).toBeTruthy()
  expect(screen.queryByText(strings.scan.unknown)).toBeNull()
})

// The converse, pinned beside the test above so the two cannot drift: adding a third
// batch kind must not change how an ordinary scan, with no jobs of either other kind
// settled, reads. `migrating` and `migrated` both default to 0 in every fixture already
// — this is what proves that default keeps the scan path silent rather than merely
// asserting it does.
test('a batch with nothing migrating or rendered still reads as a scan', async () => {
  const scanned = batchStatus({
    total: 4,
    scanned: 1,
    pending: 0,
    running: 0,
    ingested: 3,
    finishedAt: '2026-09-06T10:14:02.116Z',
  })
  expect(scanned.migrating).toBe(0)
  expect(scanned.migrated).toBe(0)
  expect(scanned.rendered).toBe(0)

  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: ok(scanned),
  })

  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText(strings.scan.finished(3, 0))).toBeTruthy()
  expect(screen.queryByText(/Move complete/)).toBeNull()
})

/**
 * The test this whole slice-4 addendum exists for. A library switched off has to render
 * off — the failure it replaces is a toggle that showed design §3.2's default and told an
 * owner who had turned rendering off that it was on.
 *
 * `checked === false` alone does not say that: an unknown toggle is unchecked too. So the
 * mixed state is asserted first, while the read is in flight, and its absence is asserted
 * after — otherwise a component that never resolved anything would pass the one assertion
 * this test is named for.
 */
test('the auto-thumbnail toggle shows off for a library the server says is off', async () => {
  let release: (response: StubResponse) => void = () => {}
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    library: () => new Promise<StubResponse>((resolve) => (release = resolve)),
  })
  renderIndex()

  // Before the read lands there is no position to take, and a confident "on" here is the
  // same lie in a shorter window.
  const toggle = (await screen.findByRole('checkbox', {
    name: strings.library.autoThumbnail,
  })) as HTMLInputElement
  expect(toggle.indeterminate).toBe(true)
  expect(toggle.checked).toBe(false)
  expect(toggle.disabled).toBe(true)

  release({ ok: true, json: async () => ({ autoThumbnail: false }) })

  await waitFor(() => expect(toggle.disabled).toBe(false))
  expect(toggle.checked).toBe(false)
  expect(toggle.indeterminate).toBe(false)
  expect(fetchMock).toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}`)
  expect(screen.queryByText(strings.library.autoThumbnailUnknown)).toBeNull()
})

// The read is the toggle's starting position, so a read that never answers must not be
// papered over with the default — that is the same wrong "on" arriving by another route.
// The control stays mixed and unclickable, and says why.
test('a settings read that fails leaves the toggle unknown rather than guessing', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    library: async () => ({ ok: false, status: 503 }),
  })
  renderIndex()

  expect(await screen.findByText(strings.library.autoThumbnailUnknown)).toBeTruthy()
  const toggle = screen.getByRole('checkbox', {
    name: strings.library.autoThumbnail,
  }) as HTMLInputElement
  expect(toggle.indeterminate).toBe(true)
  expect(toggle.checked).toBe(false)
  expect(toggle.disabled).toBe(true)
})

// A PATCH the server refused leaves the library where the GET said it was. `variables`
// covers the round trip and nothing after it: a value the server rejected is not a
// position this library is in, and now that there is something true to fall back to,
// holding the failed click on screen under a "could not change this" message would show
// two contradictory facts at once.
test('a rejected setting change falls back to what the server said, not to the click', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    library: ok({ autoThumbnail: true }),
    settings: async () => ({ ok: false, status: 503 }),
  })
  renderIndex()

  const toggle = (await screen.findByRole('checkbox', {
    name: strings.library.autoThumbnail,
  })) as HTMLInputElement
  await waitFor(() => expect(toggle.checked).toBe(true))

  fireEvent.click(toggle)

  expect(await screen.findByText(strings.library.autoThumbnailFailed)).toBeTruthy()
  expect(toggle.checked).toBe(true)
  expect(toggle.indeterminate).toBe(false)
})

// The write half, over a library the server says is on. The request is asserted whole,
// header and body included: `derive.rs`'s `bad_body` names the Content-Type explicitly,
// and a PATCH without it is a 400 the user would see as the setting silently refusing to
// change.
test('the auto-thumbnail toggle sends the setting and reflects what the server echoes', async () => {
  // Held open on purpose. A stub that resolves immediately never renders the in-flight
  // frame, and that frame is where this control was wrong: react-query clears a
  // mutation's `data` the moment it goes pending, so a position read from `data` alone
  // springs back to its old value and sits there, disabled, until the response lands.
  let release: (response: StubResponse) => void = () => {}
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    library: ok({ autoThumbnail: true }),
    settings: () => new Promise<StubResponse>((resolve) => (release = resolve)),
  })
  renderIndex()

  // The server's answer, not a default: this library is on.
  const toggle = await screen.findByRole('checkbox', { name: strings.library.autoThumbnail })
  await waitFor(() => expect((toggle as HTMLInputElement).checked).toBe(true))

  fireEvent.click(toggle)
  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: '{"autoThumbnail":false}',
    }),
  )
  expect((toggle as HTMLInputElement).checked).toBe(false)

  // And the echo is what it settles on, not the click.
  release({ ok: true, json: async () => ({ autoThumbnail: false }) })
  await waitFor(() => expect((toggle as HTMLInputElement).checked).toBe(false))
})

// Two cards, and the second one is clicked: with one card on screen a handler wired to
// `parts[0].id` cannot fail, which is the same hole the two distinct WebP fixtures were
// introduced to close. The batch id in the `202` is then asserted to be the one polled —
// that is what proves the acceptance was consumed rather than merely received.
test('the per-card action renders that part, and polls the batch it was handed', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT, HEX_NUT])),
    partThumbnail: ok({ batchId: RENDER_BATCH_ID, queued: 1 }),
    batch: ok(batchStatus({ batchId: RENDER_BATCH_ID, total: 1, pending: 1 })),
  })
  renderIndex()

  const nut = await screen.findByRole('article', { name: HEX_NUT.name })
  fireEvent.click(within(nut).getByRole('button', { name: strings.render.partFor(HEX_NUT.name) }))

  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(`/api/parts/${HEX_NUT.id}/thumbnail`, {
      method: 'POST',
    }),
  )
  expect(fetchMock).not.toHaveBeenCalledWith(`/api/parts/${MOTOR_MOUNT.id}/thumbnail`, {
    method: 'POST',
  })

  // A batch of one, watched through the same poll a scan uses, and reported in the copy
  // that is true of a render — `strings.scan.finished` would call 151 rendered previews
  // "Scan complete — 0 added."
  expect(await screen.findByText(strings.render.running(0, 1))).toBeTruthy()
  expect(fetchMock).toHaveBeenCalledWith(
    `/api/libraries/${DEFAULT_LIBRARY_ID}/jobs/${RENDER_BATCH_ID}`,
  )
})

/**
 * The task's own exit criterion: a scan started from the browser, no terminal.
 *
 * Two claims, and the second is the one that would have been missed. The route it POSTs
 * to is `/api/libraries/{id}/scan` on the api service — `deploy/web/Caddyfile` proxies
 * `/api/*` there and to nothing else, so a route mounted under `Role::Worker` is a button
 * that cannot work. And the batch it polls has to read as a *scan*: the progress copy is
 * picked from what was clicked, not from the counters, and before this button existed
 * "this page started it" meant "a preview render" — a scan inheriting that would report a
 * folder of 150 new parts as "Rendering previews — 0 of 1."
 */
test('the scan button starts a scan and watches it as a scan, not as a render', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    // What the route really answers: one job, the walk. The files it finds join this same
    // batch afterwards, which is why `total` is 1 on the first poll and climbs later.
    scan: ok({ batchId: SCAN_BATCH_ID, queued: 1 }),
    batch: ok(batchStatus({ batchId: SCAN_BATCH_ID, total: 1, pending: 0, running: 1 })),
  })
  renderIndex()

  fireEvent.click(await screen.findByRole('button', { name: strings.scan.start }))

  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}/scan`, {
      method: 'POST',
    }),
  )
  // The walk has not finished, so the batch holds one job and zero known files. The old
  // line read "Scanning — 0 of 1 files.", counting the walk as a file.
  expect(await screen.findByText('Reading the folder…')).toBeTruthy()
  expect(screen.queryByText(/of 1 files/)).toBeNull()
  expect(screen.queryByText(strings.render.running(0, 1))).toBeNull()
  expect(fetchMock).toHaveBeenCalledWith(
    `/api/libraries/${DEFAULT_LIBRARY_ID}/jobs/${SCAN_BATCH_ID}`,
  )
})

// The batch a scan grows under the poll. `batch_status` computes `total` by counting rows
// with that `batch_id` and stores no total, so the walk enqueueing its files into its own
// batch is what the progress line reads — and the alternative, a fresh batch for the
// files, would leave this line saying `1 of 1` while 150 files were still queued.
test('the progress line follows a batch whose total grows after the first poll', async () => {
  let polls = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([])),
    batch: async () => {
      polls += 1
      return {
        ok: true,
        json: async () =>
          polls === 1
            ? // The walk itself, still running.
              batchStatus({ total: 1, scanned: 0, pending: 0, running: 1 })
            : // It found three files and put them in this batch; it is done itself.
              batchStatus({ total: 4, scanned: 1, pending: 3, running: 0 }),
      }
    },
  })
  renderIndex({ batch: BATCH_ID })

  expect(await screen.findByText('Reading the folder…')).toBeTruthy()
  // Three files found, none settled — not "1 of 4", which counted the finished walk as a
  // settled file and the walk job as a fourth file.
  expect(
    await screen.findByText('Scanning — 0 of 3 files.', undefined, { timeout: 4000 }),
  ).toBeTruthy()
})

// `queued: 0` on a library that exists means every part already has a preview. That is a
// success, and reading it as a failure is the easy mistake — a library that does not
// exist answers 404 instead, which is what the error copy is for. Such a batch has no
// status resource either, so polling it would 404 a moment later and tell the user their
// successful action failed.
test('a sweep that finds nothing missing reads as success, not as an error', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    sweep: ok({ batchId: RENDER_BATCH_ID, queued: 0 }),
  })
  renderIndex()

  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  fireEvent.click(screen.getByRole('button', { name: strings.render.sweep }))

  expect(await screen.findByText(strings.render.nothingMissing)).toBeTruthy()
  expect(screen.queryByText(strings.render.queueFailed)).toBeNull()
  expect(fetchMock).toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}/thumbnails`, {
    method: 'POST',
  })
  expect(fetchMock.mock.calls.filter(([url]) => String(url).includes('/jobs/'))).toHaveLength(0)
})

/**
 * A revision whose source `file` row is gone. Not something ingest writes — it is what a
 * half-repaired database looks like — and the part still has to appear, because its owner
 * is the one person who needs to find it to delete or re-scan it. All four source fields
 * are absent together, which is the shape `PartCard` documents.
 */
const RECOVERED_BRACKET: PartCard = {
  ...SHAFT_COUPLER,
  id: '01931b6e-0000-7000-8000-0000000a0006',
  revision: '01931b6e-0000-7000-8000-0000000b0006',
  name: 'Angle bracket, 40 x 40 x 3 mm',
  partNumber: 'LP-1042-03',
  sourceHash: null,
  sourceBytes: null,
  storedBytes: null,
  compressed: null,
}

/**
 * A library's totals as the API sends them. The numbers are slice 4's own measured ones —
 * 5,718,866 bytes of inline previews over 151 parts — so the ratio on screen is a figure
 * that was actually observed rather than one invented to round nicely.
 */
const LIBRARY_STORAGE: LibraryStorage = {
  sourceBytes: 12_480_000,
  derivativeBytes: 5_718_866,
  derivativeRatio: 5_718_866 / 12_480_000,
}

// The download is a plain anchor on purpose: the browser reads `Content-Disposition`,
// which is where the RFC 5987 filename lives, and a fetch into a blob URL would rename
// every file. Two cards, and both are asserted, so a link wired to `parts[0].revision` —
// every user downloading the same part — fails on the second.
test('each card links to its own revision and asks for the original bytes', async () => {
  const fetchMock = stubFetch({ healthz: ok(HEALTHY), parts: ok(page([MOTOR_MOUNT, HEX_NUT])) })
  renderIndex()

  const mount = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  const nut = screen.getByRole('article', { name: HEX_NUT.name })

  // `getByRole` throws when there is no link, so this cannot pass over a card that
  // renders no download control at all — which is the shape slice 4's SET-B ruling
  // caught, an assertion equally true of an element that is not there.
  const mountLink = within(mount).getByRole('link', {
    name: strings.download.originalFor(MOTOR_MOUNT.name),
  })
  const nutLink = within(nut).getByRole('link', {
    name: strings.download.originalFor(HEX_NUT.name),
  })

  // The revision, never the part: a download URL names a revision, and every fixture
  // here carries a revision id that differs from its part id so that a link built from
  // the wrong one cannot pass.
  expect(mountLink.getAttribute('href')).toBe(
    `/api/revisions/${MOTOR_MOUNT.revision}/download?variant=original`,
  )
  expect(nutLink.getAttribute('href')).toBe(
    `/api/revisions/${HEX_NUT.revision}/download?variant=original`,
  )
  // Called out on its own as well: the route 400s without `variant`, and a URL that
  // dropped it would still carry the revision id and still look entirely plausible.
  expect(mountLink.getAttribute('href')).toContain('variant=original')
  // An anchor the browser treats as a download, not a navigation.
  expect(mountLink.getAttribute('download')).not.toBeNull()
  // And nothing fetched it. A `fetch` here would discard `Content-Disposition` and hand
  // the user a file named after the revision id.
  expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/download'))).toBe(false)

  // The hash beside the link is this card's own, at the length the card renders it, with
  // the whole digest available to check the downloaded file against (DATA.md §5.1).
  const shortHash = within(mount).getByText(MOTOR_MOUNT.sourceHash!.slice(0, 12))
  expect(shortHash.getAttribute('title')).toBe(MOTOR_MOUNT.sourceHash)
  expect(within(nut).getByText(HEX_NUT.sourceHash!.slice(0, 12))).toBeDefined()
})

// Literals, not the constants: this copy exists to state two specific facts — what the
// file occupies, and whether zstd bought anything — and reading it back from strings.ts
// would pass just as happily against wording that states neither. The pair of fixtures is
// the point: the compressed one and the `AsIs` one take different branches.
test('the card says what the file costs on disk and whether it was compressed', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([MOTOR_MOUNT, SHAFT_COUPLER])) })
  renderIndex()

  const mount = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  const coupler = screen.getByRole('article', { name: SHAFT_COUPLER.name })
  expect(within(mount).getByText('197 kB on disk, compressed from 624.4 kB')).toBeDefined()
  expect(within(coupler).getByText('148.9 kB on disk, stored uncompressed')).toBeDefined()
})

// Four nulls. The card must still render, and the download must not look available.
test('a revision with no source file keeps its card and offers no download', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([RECOVERED_BRACKET, HEX_NUT])) })
  renderIndex()

  // In this order, and all three. "There is no link on this card" is also true of a card
  // that never rendered, so the card and its message are asserted first — otherwise this
  // test passes over a grid that dropped the part entirely, which is the failure it
  // exists to forbid.
  const card = await screen.findByRole('article', { name: RECOVERED_BRACKET.name })
  expect(within(card).getByText(strings.download.noSource)).toBeDefined()
  expect(within(card).queryByRole('link')).toBeNull()
  // No size line invented out of nulls either.
  expect(within(card).queryByText(/on disk/)).toBeNull()

  // And the neighbouring part still has its link, so the absence above is this card's
  // and not the page failing to render links at all.
  const nut = screen.getByRole('article', { name: HEX_NUT.name })
  expect(
    within(nut).getByRole('link', { name: strings.download.originalFor(HEX_NUT.name) }),
  ).toBeDefined()
})

// Spec §4: source total, derivative total, and the ratio between them — the figure that
// made slice 4's 92.5% drop legible, for a user's own library. Literal again, because the
// direction of the ratio is the whole meaning and a line reading "273% of source" would
// satisfy any assertion built out of the same constant.
test('the library totals report both storage classes and the ratio between them', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    storage: ok(LIBRARY_STORAGE),
  })
  renderIndex()

  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(
    await screen.findByText('Sources 12.5 MB on disk · derivatives 5.7 MB, 45.8% of source.'),
  ).toBeDefined()
  expect(fetchMock).toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}/storage`)
})

// The combination with no honest sentence: the row says compressed, and the size it was
// compressed from did not arrive. `storedCompressed` cannot be built without the second
// figure, and `storedRaw` would print "stored uncompressed" over a row that says the
// opposite — a false claim, not a cautious one, against CLAUDE.md's measurement rule.
// The card states the size and says nothing about compression.
test('a compressed part whose ingested size is missing claims no compression state', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(
      page([{ ...MOTOR_MOUNT, sourceBytes: null } as unknown as PartCard]),
    ),
    storage: ok(LIBRARY_STORAGE),
  })
  renderIndex()

  const card = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  // The size is still stated — a card that dropped the line entirely would satisfy the
  // absence assertion below while telling the user less than it knows.
  expect(within(card).getByText('197 kB on disk')).toBeDefined()
  expect(within(card).queryByText(/stored uncompressed/)).toBeNull()
  expect(within(card).queryByText(/compressed from/)).toBeNull()
})

// A library whose parts all lack a source row divides by nothing, so the server sends
// `derivativeRatio: null` and the sentence has to stop after the two totals. The
// percentage clause is not merely redundant there — collapsing the branch renders
// `NaN% of source`, which is a figure rather than an omission and reads as a real one.
test('a library with no source bytes reports its totals without a ratio', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    storage: ok({ sourceBytes: 0, derivativeBytes: 40_960, derivativeRatio: null }),
  })
  renderIndex()

  expect(await screen.findByText('Sources 0 B on disk · derivatives 41 kB.')).toBeDefined()
  expect(screen.queryByText(/NaN/)).toBeNull()
  expect(screen.queryByText(/% of source/)).toBeNull()
})

// The panel reads one field the response is not validated against, so the same drift
// `SourceFile` narrows for reaches it too: `=== null` waves `undefined` through into
// `(undefined * 100)`. Asserting the absence of `NaN` alone would pass against a panel
// that rendered nothing at all, so the totals themselves are asserted first.
test('a ratio the server stopped sending renders as no ratio, not as NaN', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    storage: ok({ sourceBytes: 12_480_000, derivativeBytes: 5_718_866 }),
  })
  renderIndex()

  expect(
    await screen.findByText('Sources 12.5 MB on disk · derivatives 5.7 MB.'),
  ).toBeDefined()
  expect(screen.queryByText(/NaN/)).toBeNull()
})

// A total that cannot be read is not a total of zero, and silence is what a reader would
// take it for. The grid still renders, because one failed panel must not take the page
// with it — which is why the card is asserted before the message.
test('a storage read that fails says so instead of going quiet', async () => {
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    storage: () => Promise.reject(new Error('api unreachable')),
  })
  renderIndex()

  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(
    await screen.findByText(
      'Could not read what this library occupies. Check that the api service is running, then reload.',
    ),
  ).toBeDefined()
})

// The totals move when the library does. A scan that adds parts and leaves the panel
// showing the pre-scan figures is worse than a panel that never rendered: the number is
// there, it is wrong, and nothing about it looks stale.
test('finishing a scan re-reads what the library occupies', async () => {
  let storageReads = 0
  stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    storage: () => {
      storageReads += 1
      return ok(LIBRARY_STORAGE)()
    },
    scan: ok({ batchId: SCAN_BATCH_ID, queued: 1 }),
    batch: ok(batchStatus({ batchId: SCAN_BATCH_ID, total: 1, pending: 0, ingested: 1 })),
  })
  renderIndex()

  await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  const before = storageReads
  fireEvent.click(await screen.findByRole('button', { name: strings.scan.start }))

  await waitFor(() => expect(storageReads).toBeGreaterThan(before))
})

/**
 * The category tree beside the grid, and the one half of it that needs a real router: the
 * selection is a search param, so proving it lands in the URL means rendering the route
 * rather than the component. `FolderTree.test.tsx` covers everything that does not need
 * one.
 *
 * `createMemoryHistory` rather than a stub: `routeTree.gen.ts` imports its routes plainly
 * and `vitest.config.ts` does not load the router plugin, so the generated tree renders
 * here as it ships.
 */
function renderApp(client: QueryClient = newClient()) {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
  return router
}

const TERRAIN: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0001',
  parentId: null,
  name: 'Terrain',
  partCount: 34,
}
const ROCKS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0002',
  parentId: TERRAIN.id,
  name: 'Rocks',
  partCount: 12,
}

/**
 * A card as it arrives once a model has its own directory. The intersection narrows
 * `PartCard.directory` from `string | null` to the one case each fixture is for, so the
 * assertions below can name `CLIFF_FACE.directory` directly instead of re-narrowing a
 * value the fixture already fixed.
 *
 * The path is the server's, verbatim, and store-relative — it names a place inside the
 * storage volume, not a path on the reader's machine, because the api serving it is in a
 * container and does not know what that volume is mounted as outside one. It is never
 * rebuilt here from category names either: the server disambiguates colliding directory
 * names and a client cannot know when it did.
 */
const CLIFF_FACE: PartCard & { directory: string } = {
  id: '01931b6e-0000-7000-8000-0000000a0007',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0007',
  name: 'Basalt cliff face, 180 mm span',
  partNumber: 'LP-7710-C',
  thumbnail: WEBP_BLUE,
  triangleCount: 148_302,
  approximate: true,
  sourceHash: '5b8c1f2e9a47d0c3b6154e88f0a2d97361cc4e5b0f18a7d2946b3e5107cd82af',
  sourceBytes: 7_412_880,
  storedBytes: 2_104_331,
  compressed: true,
  createdAt: '2026-08-30T11:04:19Z',
  updatedAt: '2026-08-30T11:04:19Z',
  directory: 'libraries/default/Terrain/Rocks/basalt_cliff_face',
}

/** Ingested before the folder layout existed, so it is still in the shared store. */
const OLD_BRACKET: PartCard & { directory: null } = {
  id: '01931b6e-0000-7000-8000-0000000a0008',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0008',
  name: 'Corner bracket, 40 x 40 mm, 4 mm wall',
  partNumber: 'LP-2280-A',
  thumbnail: WEBP_ORANGE,
  triangleCount: 3204,
  approximate: true,
  sourceHash: 'e41d2b7c05986aa3f0d4b8172c9e5a63d081fb42c7e9503a186dd47b2c9f0e35',
  sourceBytes: 212_004,
  storedBytes: 64_118,
  compressed: true,
  createdAt: '2026-05-02T08:41:07Z',
  updatedAt: '2026-05-02T08:41:07Z',
  directory: null,
}

test('selecting a category puts it in the URL and asks the grid for that category', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    folders: ok([TERRAIN, ROCKS]),
    storage: ok(LIBRARY_STORAGE),
  })
  const router = renderApp()

  fireEvent.click(await screen.findByRole('button', { name: 'Rocks' }))

  // In the URL, not in component state: the filter survives a reload and is a link
  // someone can send.
  await waitFor(() => expect(router.state.location.searchStr).toContain(`folderId=${ROCKS.id}`))
  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/libraries/${DEFAULT_LIBRARY_ID}/parts?folderId=${ROCKS.id}`,
    ),
  )

  fireEvent.click(screen.getByRole('button', { name: strings.folders.root }))
  // Cleared means absent, never `?folderId=`: an absent parameter is what the route reads
  // as the whole library.
  await waitFor(() => expect(router.state.location.searchStr).not.toContain('folderId'))
})

test('a category in the URL filters the first request the grid makes', async () => {
  const fetchMock = stubFetch({
    healthz: ok(HEALTHY),
    parts: ok(page([MOTOR_MOUNT])),
    folders: ok([TERRAIN, ROCKS]),
    storage: ok(LIBRARY_STORAGE),
  })
  renderIndex({ folderId: ROCKS.id })

  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(
      `/api/libraries/${DEFAULT_LIBRARY_ID}/parts?folderId=${ROCKS.id}`,
    ),
  )
  // Unfiltered is a different request, not this one with an empty parameter.
  expect(fetchMock).not.toHaveBeenCalledWith(`/api/libraries/${DEFAULT_LIBRARY_ID}/parts`)
})

test('show in folder reveals the directory as copyable text and opens nothing', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([CLIFF_FACE])), folders: ok([TERRAIN]) })
  renderIndex()

  const card = await screen.findByRole('article', { name: CLIFF_FACE.name })
  fireEvent.click(
    within(card).getByRole('button', { name: strings.folders.showInFolderFor(CLIFF_FACE.name) }),
  )

  expect(within(card).getByText(CLIFF_FACE.directory)).toBeDefined()
  expect(within(card).getByText(strings.folders.directoryHint)).toBeDefined()
  // No browser opens a host file manager, and `file://` navigation from a page is blocked
  // everywhere — so nothing here pretends to. The path is text, and there is no link.
  expect(document.querySelector('a[href^="file:"]')).toBeNull()

  // jsdom has no clipboard, which is the same shape as an insecure context: the copy
  // control must not throw there, and the path stays on screen either way.
  fireEvent.click(within(card).getByRole('button', { name: strings.folders.copyPath }))
  expect(within(card).getByText(CLIFF_FACE.directory)).toBeDefined()
})

test('a model still in the shared store says so rather than showing an invented path', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([OLD_BRACKET])), folders: ok([TERRAIN]) })
  renderIndex()

  const card = await screen.findByRole('article', { name: OLD_BRACKET.name })
  fireEvent.click(
    within(card).getByRole('button', { name: strings.folders.showInFolderFor(OLD_BRACKET.name) }),
  )

  expect(within(card).getByText(strings.folders.directoryPending)).toBeDefined()
  // And the move is withheld with its reason rather than offered and refused at the
  // server with a 409 — the same status a name collision uses, which the UI would then
  // present as one.
  expect(
    within(card).queryByRole('button', { name: strings.folders.moveToFor(OLD_BRACKET.name) }),
  ).toBeNull()
  expect(within(card).getByText(strings.folders.notMigrated)).toBeDefined()
  expect(card.getAttribute('draggable')).toBe('false')
})

test('a card offers the move chooser, and is draggable for the tree to catch', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([CLIFF_FACE])), folders: ok([TERRAIN, ROCKS]) })
  renderIndex()

  const card = await screen.findByRole('article', { name: CLIFF_FACE.name })
  expect(card.getAttribute('draggable')).toBe('true')

  // The keyboard path: a plain button on the card, no pointer gesture anywhere in it.
  fireEvent.click(
    within(card).getByRole('button', { name: strings.folders.moveToFor(CLIFF_FACE.name) }),
  )
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText(strings.folders.moveTitle(CLIFF_FACE.name))).toBeDefined()
  expect(
    within(dialog).getByRole('button', { name: strings.folders.moveInto(ROCKS.name) }),
  ).toBeDefined()
})

test('right-clicking a card opens the same chooser the button does', async () => {
  stubFetch({ healthz: ok(HEALTHY), parts: ok(page([CLIFF_FACE])), folders: ok([TERRAIN, ROCKS]) })
  renderIndex()

  const card = await screen.findByRole('article', { name: CLIFF_FACE.name })
  // The pointer gesture a file manager would give you, opening the same chooser rather
  // than a menu of its own — so neither path can drift from the other.
  fireEvent.contextMenu(card)

  expect(
    within(await screen.findByRole('dialog')).getByRole('button', {
      name: strings.folders.moveInto(ROCKS.name),
    }),
  ).toBeDefined()
})
