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
 * A real 2x2 lossless WebP, base64'd exactly as `PartCard.thumbnail` carries it. The
 * bytes matter: the thumbnail assertion compares the rendered `src` against this whole
 * string, so an `<img>` wired to `''`, to the part id, or to a hash-addressed URL that
 * slice 1 has no endpoint for all fail rather than pass on the mere presence of a tag.
 */
const WEBP =
  'data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvAUAAAAdQ1LrVv/+BiOh/AAA='

const MOTOR_MOUNT: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0001',
  library: DEFAULT_LIBRARY_ID,
  name: 'NEMA 17 motor mount, 42 mm face',
  partNumber: 'LP-3105-A',
  thumbnail: WEBP,
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
  thumbnail: WEBP,
  triangleCount: 1984,
  approximate: true,
  createdAt: '2026-08-14T09:12:51Z',
  updatedAt: '2026-08-14T09:12:51Z',
}

/** Ingested from a photogrammetry scan: rendered, but no thumbnail derivative yet. */
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
test('renders the empty-library copy from strings.ts', async () => {
  stubFetch({ parts: ok(page([])) })
  renderIndex()
  expect(await screen.findByText(strings.emptyLibrary.title)).toBeDefined()
  expect(screen.getByText(strings.emptyLibrary.body)).toBeDefined()
})

// "We have not asked yet" and "we asked and there is nothing" are different facts, and
// only the second one is the empty state. Without this, a component that renders the
// empty state during the request still passes every other test here, because the pages
// they mock all resolve.
// Ingest is a server-side scan over a mounted directory. The grid has no upload control
// and slice 1 has no endpoint that would give it one, so copy that sends the user looking
// for one is a wrong instruction, not a harmless flourish. This survives a rewording,
// which the assertion above deliberately does not.
test('the empty state points at no upload control, because there is none', async () => {
  stubFetch({ parts: ok(page([])) })
  renderIndex()
  const empty = await screen.findByText(strings.emptyLibrary.body)
  const copy = `${strings.emptyLibrary.title} ${empty.textContent}`.toLowerCase()
  for (const claim of ['upload', 'drag', 'drop', 'browse', 'choose a file', 'add file']) {
    expect(copy).not.toContain(claim)
  }
  expect(screen.queryByRole('button')).toBeNull()
  expect(document.querySelector('input[type="file"]')).toBeNull()
})

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

test('renders a card per part with the thumbnail bytes inline as the image source', async () => {
  const fetchMock = stubFetch({ parts: ok(page([MOTOR_MOUNT, HEX_NUT])) })
  renderIndex()

  const mount = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  const nut = screen.getByRole('article', { name: HEX_NUT.name })
  expect(within(mount).getByText(MOTOR_MOUNT.partNumber!)).toBeDefined()
  expect(within(nut).getByText(HEX_NUT.partNumber!)).toBeDefined()

  // The whole data URL, not merely a non-empty src: this is the one assertion that
  // proves the WebP the endpoint sent is what reaches the browser.
  for (const [card, part] of [
    [mount, MOTOR_MOUNT],
    [nut, HEX_NUT],
  ] as const) {
    const img = within(card).getByRole('img')
    expect(img.getAttribute('src')).toBe(part.thumbnail)
    expect(img.getAttribute('src')).toMatch(/^data:image\/webp;base64,[A-Za-z0-9+/=]+$/)
    expect(img.getAttribute('alt')).toBe(strings.parts.thumbnailAlt(part.name))
  }

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

// CLAUDE.md: mesh-derived measurements are labelled "approximate" in the UI, always. The
// triangle count is the only figure this card shows and it is tessellation-derived by
// construction, so it must never appear without the label.
test('shows the triangle count only alongside the approximate label', async () => {
  stubFetch({ parts: ok(page([MOTOR_MOUNT])) })
  renderIndex()
  const card = await screen.findByRole('article', { name: MOTOR_MOUNT.name })
  expect(within(card).getByText(strings.parts.triangles(MOTOR_MOUNT.triangleCount!))).toBeDefined()
  expect(within(card).getByText(strings.parts.approximate)).toBeDefined()
})

// The flag means "any figure on this part is mesh-derived", so the label is keyed to the
// flag itself and not to the presence of a triangle count. Both fixtures here withhold
// the count, which is what makes the two mistakes distinguishable: keying off
// triangleCount would drop the label from the first card and keying off nothing at all
// would add it to the second.
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
  expect(within(mesh).getByText(strings.parts.approximate)).toBeDefined()
  expect(within(brep).queryByText(strings.parts.approximate)).toBeNull()
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
