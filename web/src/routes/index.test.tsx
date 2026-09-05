import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { Index } from './index'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { BatchStatus, PartCard, PartsPage } from '../lib/types'

/**
 * `Index` takes the batch as a prop rather than reading the search param itself, which is
 * what lets these tests render it with no router in scope. The route component does the
 * `useSearch()` half; see `index.tsx`.
 */
function renderIndex(props: { batch?: string; client?: QueryClient } = {}) {
  const client = props.client ?? newClient()
  return render(
    <QueryClientProvider client={client}>
      <Index batch={props.batch} />
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
  settings?: () => Promise<StubResponse>
  sweep?: () => Promise<StubResponse>
  partThumbnail?: () => Promise<StubResponse>
}) {
  const fetchMock = vi.fn((url: string, init?: { method?: string }) => {
    if (url.startsWith('/api/healthz')) return (routes.healthz ?? pending)()
    // The one route distinguished by method rather than path: `PATCH /api/libraries/{id}`
    // is a prefix of every other library route.
    if (init?.method === 'PATCH') return (routes.settings ?? pending)()
    // Order matters twice over. `/thumbnails` is a suffix of nothing else but `/thumbnail`
    // is a suffix of it, and the per-card route is `/api/parts/{id}/thumbnail` — which the
    // earlier `includes('/parts')` rule would have answered with a page of the grid.
    if (url.endsWith('/thumbnails')) return (routes.sweep ?? pending)()
    if (url.endsWith('/thumbnail')) return (routes.partThumbnail ?? pending)()
    if (url.endsWith('/parts')) return (routes.parts ?? pending)()
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

/** A `BatchStatus` as the API sends it, with the counters a test cares about overridden. */
const batchStatus = (over: Partial<BatchStatus> = {}): BatchStatus => ({
  batchId: BATCH_ID,
  libraryId: DEFAULT_LIBRARY_ID,
  total: 6,
  pending: 6,
  running: 0,
  ingested: 0,
  skipped: 0,
  rendered: 0,
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
    batch: ok(batchStatus({ total: 6, pending: 3, running: 1, ingested: 2 })),
  })
  renderIndex({ batch: BATCH_ID })

  // Two of six settled, so that is what the line says — `total` comes from the batch, not
  // from the grid, which is still empty at this point precisely because the scan is why.
  expect(await screen.findByText(strings.scan.running(2, 6))).toBeTruthy()
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

  // The count belongs on screen; the per-file reason is the failed-file drawer, Phase 2.
  expect(await screen.findByText(strings.scan.failed(1))).toBeTruthy()
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
