import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { PartPage } from './parts.$partId'
import { strings } from '../lib/strings'
import type { PartDetail } from '../lib/types'

/**
 * The part page, which is where `CLAUDE.md`'s measurement rules actually reach a screen.
 *
 * Most of these assert a refusal rather than a rendering: no volume for an open mesh, no
 * figure without its provenance, no "approximate" on an analytic value. Those are the
 * cases a page that simply printed its JSON would get wrong while looking entirely
 * correct, and none of them is visible from the API tests — the route hands back `null`
 * and it is this component that decides whether `null` reads as zero, as blank, or as a
 * sentence.
 */

const PART: PartDetail = {
  id: '01931b6e-0000-7000-8000-00000000aaaa',
  library: '01931b6e-0000-7000-8000-000000000001',
  revision: '01931b6e-0000-7000-8000-00000000bbbb',
  revLabel: '1',
  name: 'Bearing block, 608ZZ',
  partNumber: 'LP-1042-03',
  sourcePath: 'brackets/steel/LP-1042-03.stl',
  thumbnail: null,
  triangleCount: 48112,
  isWatertight: true,
  bboxMm: { value: [61, 42, 18.5], approximate: true },
  volumeMm3: { value: 21478.5, approximate: true },
  surfaceAreaMm2: { value: 9804.25, approximate: true },
  kernelVersion: 'mesh stl-1+cpu-1',
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceFormat: 'stl',
  sourceBytes: 204800,
  storedBytes: 91204,
  compressed: true,
  tessellationL0: '3333333333333333333333333333333333333333333333333333333333333333',
  tessellationL0Bytes: 7500,
  createdAt: '2026-09-06T10:00:00Z',
  updatedAt: '2026-09-06T10:00:00Z',
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

/** A fetch that answers the detail route with `part`, or a status when given a number. */
function stub(part: PartDetail | number) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      typeof part === 'number'
        ? { ok: false, status: part, json: async () => ({}) }
        : { ok: true, status: 200, json: async () => part },
    ),
  )
}

/** The page needs a router in scope: it renders a `<Link to="/">` back to the grid. */
function renderPage() {
  const rootRoute = createRootRoute({ component: () => <PartPage partId={PART.id} /> })
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

/** The row whose `<dt>` is `label`, so an assertion names the figure it means. */
async function row(label: string): Promise<HTMLElement> {
  const term = await screen.findByText(label)
  const value = term.nextElementSibling
  expect(value).not.toBeNull()
  return value as HTMLElement
}

test('a tessellated figure is labelled and an analytic one is not', async () => {
  // The Phase 2 shape, and the reason the badge is per figure rather than per page: one
  // revision carrying an analytic volume beside a tessellated surface area. A page-level
  // badge is wrong about one of them whichever way it is set.
  stub({
    ...PART,
    volumeMm3: { value: 21478.5, approximate: false },
    surfaceAreaMm2: { value: 9804.25, approximate: true },
  })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).queryByText(strings.detail.approximate)).toBeNull()
  const area = await row(strings.detail.surfaceArea)
  expect(within(area).getByText(strings.detail.approximate)).toBeDefined()
})

test('an open mesh says why it has no volume rather than showing a blank', async () => {
  // "Measurement must not lie" includes declining to measure. A blank here reads as zero,
  // and zero is a number this part does not have.
  stub({ ...PART, isWatertight: false, volumeMm3: null })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).getByText(strings.detail.volumeUnavailable)).toBeDefined()
})

test('a missing volume on a closed mesh is unknown, not a refusal to measure', async () => {
  // The other `null`. "Not available — the mesh is not closed" would be a false
  // explanation for a watertight part whose figure simply was not recorded.
  stub({ ...PART, isWatertight: true, volumeMm3: null })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).getByText(strings.detail.unknown)).toBeDefined()
  expect(within(volume).queryByText(strings.detail.volumeUnavailable)).toBeNull()
})

test('the L0 rung is offered by hash, which is what makes those bytes reachable', async () => {
  // Until this link existed, every ingest wrote a rung that nothing could address:
  // `GET /api/blob/{blake3}` had no possible caller.
  stub(PART)
  renderPage()

  const preview = await row(strings.detail.preview3d)
  const link = within(preview).getByRole('link')
  expect(link.getAttribute('href')).toBe(`/api/blob/${PART.tessellationL0}`)
})

test('a part with no rung says so instead of linking to bytes that do not exist', async () => {
  stub({ ...PART, tessellationL0: null, tessellationL0Bytes: null })
  renderPage()

  const preview = await row(strings.detail.preview3d)
  expect(within(preview).getByText(strings.detail.noPreview3d)).toBeDefined()
  expect(within(preview).queryByRole('link')).toBeNull()
})

test('the path is shown, because it is what tells two parts of the same name apart', async () => {
  // Slice 6a moved identity from the name to the path. Two parts called `bracket` in two
  // folders are one part and one silent skip without it, so a page that omitted it could
  // not answer "which bracket is this".
  stub(PART)
  renderPage()

  const path = await row(strings.detail.sourcePath)
  expect(within(path).getByText('brackets/steel/LP-1042-03.stl')).toBeDefined()
})

test('a part that is gone shows one actionable message', async () => {
  // A 404 and an unreachable api land here alike: this page was reached from a grid that
  // may be stale, and going back is what refreshes it either way.
  stub(404)
  renderPage()

  expect(await screen.findByText(strings.detail.failed)).toBeDefined()
})
