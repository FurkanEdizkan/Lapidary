import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { RemovedPage } from './removed'
import { strings } from '../lib/strings'
import type { PartCard } from '../lib/types'

/**
 * The removed list, which is two things at once: the only route back to a part somebody
 * removed, and the only place the one irreversible action in the product is reachable
 * from. Both are asserted here, because both are invisible from the API tests — the routes
 * behave correctly whether or not anything ever calls them.
 */

/**
 * Two parts with the same name in different folders. Not a contrivance: since slice 6a the
 * path is what tells them apart, and this list is where getting that wrong destroys the
 * wrong one.
 */
const MOUNTING: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0001',
  library: '01931b6e-0000-7000-8000-000000000001',
  revision: '01931b6e-0000-7000-8000-0000000b0001',
  name: 'Bracket, LP-1042-03',
  sourcePath: 'mounting/LP-1042-03.stl',
  partNumber: 'LP-1042-03',
  thumbnail: null,
  triangleCount: 48112,
  approximate: true,
  tessellationL0: null,
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceBytes: 204800,
  storedBytes: 91204,
  compressed: true,
  createdAt: '2026-09-06T10:00:00Z',
  updatedAt: '2026-09-06T10:00:00Z',
}

const SPARES: PartCard = {
  ...MOUNTING,
  id: '01931b6e-0000-7000-8000-0000000a0002',
  revision: '01931b6e-0000-7000-8000-0000000b0002',
  sourcePath: 'spares/LP-1042-03.stl',
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

/** Records every request, so a test can assert which routes were *not* called. */
function stub(parts: PartCard[], purge: { quarantined: number; quarantinedBytes: number } = {
  quarantined: 1,
  quarantinedBytes: 91204,
}) {
  const calls: { url: string; method: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method ?? 'GET' })
      if (url.includes('/purge')) {
        return { ok: true, status: 200, json: async () => purge }
      }
      if (url.includes('/restore')) {
        return { ok: true, status: 204, json: async () => ({}) }
      }
      return { ok: true, status: 200, json: async () => ({ parts, next: null }) }
    }),
  )
  return calls
}

function renderPage() {
  const rootRoute = createRootRoute({ component: () => <RemovedPage /> })
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

async function rowFor(sourcePath: string): Promise<HTMLElement> {
  const path = await screen.findByText(sourcePath)
  return path.closest('li') as HTMLElement
}

test('the list asks for removed parts and tells two identically named ones apart', async () => {
  const calls = stub([MOUNTING, SPARES])
  renderPage()

  await screen.findByText('mounting/LP-1042-03.stl')
  expect(screen.getByText('spares/LP-1042-03.stl')).toBeDefined()
  // Both rows say the same name. The path is the only thing distinguishing them, which is
  // why it is on the summary at all.
  expect(screen.getAllByText('Bracket, LP-1042-03')).toHaveLength(2)
  expect(calls[0]?.url).toContain('state=removed')
})

test('purge is behind a confirmation that names the path, and declining calls nothing', async () => {
  const calls = stub([MOUNTING, SPARES])
  // `false`: the user read the dialog and said no. Nothing may reach the API — this is the
  // one action in the product that cannot be undone.
  // Typed argument, so the assertion below can read what the dialog actually said.
  const confirm = vi.fn((_message: string) => false)
  vi.stubGlobal('confirm', confirm)
  renderPage()

  const row = await rowFor('spares/LP-1042-03.stl')
  fireEvent.click(within(row).getByText(strings.removal.purge))

  expect(confirm).toHaveBeenCalledWith(
    strings.removal.purgeConfirm('spares/LP-1042-03.stl'),
  )
  // The confirmation has to name *this* row's path. A dialog naming the other part, or
  // naming only the shared name, is one a person cannot answer correctly.
  expect(confirm.mock.calls[0]?.[0]).toContain('spares/')
  expect(calls.some((call) => call.url.includes('/purge'))).toBe(false)
})

test('confirming purge calls the route and reports what is kept, never what is freed', async () => {
  const calls = stub([MOUNTING], { quarantined: 2, quarantinedBytes: 182408 })
  vi.stubGlobal('confirm', vi.fn(() => true))
  renderPage()

  const row = await rowFor('mounting/LP-1042-03.stl')
  fireEvent.click(within(row).getByText(strings.removal.purge))

  await waitFor(() =>
    expect(
      calls.some(
        (call) => call.method === 'POST' && call.url.includes(`${MOUNTING.id}/purge`),
      ),
    ).toBe(true),
  )
  // `CLAUDE.md` requires that this never read as space recovered: a purge frees nothing on
  // the day it runs, and the bytes it names are waiting, not gone.
  const note = await screen.findByText(/kept for 30 days/)
  expect(note.textContent).not.toMatch(/freed/i)
})

test('restore calls the route for the row it was clicked on', async () => {
  const calls = stub([MOUNTING, SPARES])
  renderPage()

  const row = await rowFor('spares/LP-1042-03.stl')
  fireEvent.click(within(row).getByText(strings.removal.restore))

  await waitFor(() =>
    expect(
      calls.some(
        (call) => call.method === 'POST' && call.url.includes(`${SPARES.id}/restore`),
      ),
    ).toBe(true),
  )
  expect(calls.some((call) => call.url.includes(MOUNTING.id))).toBe(false)
})

test('an empty list says so rather than rendering nothing', async () => {
  stub([])
  renderPage()
  await screen.findByText(strings.removal.removedEmpty)
})
