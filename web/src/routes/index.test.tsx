import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { Index } from './index'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { PartCard, PartsPage } from '../lib/types'

function renderIndex() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Index />
    </QueryClientProvider>,
  )
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
}) {
  const fetchMock = vi.fn((url: string) => {
    if (url.startsWith('/api/healthz')) return (routes.healthz ?? pending)()
    if (url.includes('/parts')) return (routes.parts ?? pending)()
    return Promise.reject(new Error(`unstubbed request: ${url}`))
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

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
  name: 'NEMA 17 motor mount, 42 mm face',
  partNumber: 'LP-3105-A',
  thumbnail: WEBP_BLUE,
  triangleCount: 12486,
  approximate: true,
  createdAt: '2026-08-14T09:12:44Z',
  updatedAt: '2026-08-14T09:12:44Z',
}

const HEX_NUT: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0002',
  library: DEFAULT_LIBRARY_ID,
  name: 'Hex nut M8, DIN 934',
  partNumber: 'DIN934-M8-A2',
  thumbnail: WEBP_ORANGE,
  triangleCount: 1984,
  approximate: true,
  createdAt: '2026-08-14T09:12:51Z',
  updatedAt: '2026-08-14T09:12:51Z',
}

/** Ingested, but the worker has not rasterized a thumbnail derivative for it yet. */
const SHAFT_COUPLER: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0003',
  library: DEFAULT_LIBRARY_ID,
  name: 'Flexible shaft coupler, 5 mm to 8 mm',
  partNumber: 'LP-4420-B',
  thumbnail: null,
  triangleCount: 7320,
  approximate: true,
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
  for (const claim of ['upload', 'drag', 'drop', 'browse', 'choose a file', 'add file']) {
    expect(rendered).not.toContain(claim)
  }
  expect(screen.queryByRole('button')).toBeNull()
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
