import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { SharedLibraryPage } from './sharing_.shares.$shareId'
import { strings } from '../lib/strings'
import type { MirroredPart, MirroredShare, PeerShareId } from '../lib/types'

/**
 * A shared library, as this installation mirrored it: somebody else's category, so it browses while their machine is
 * asleep. What it must get right is what a person decides by — each part's licence, and a preview — and that it never
 * reads as though something of theirs was lost when the sharer stops offering a part.
 */

const SHARE = '01a0c7e2-4d11-7b20-9a31-7c2e5dab0001' as PeerShareId

const TERRAIN: MirroredShare = {
  id: SHARE,
  deviceId: '7M2JD-4ZQ7M-H8R3N-0V5KQ-9BCPE-6WXT1-5SGVA-2FJ4K-3HQ6M-8NDYR-K0',
  sharer: 'Ayşe’s workshop',
  name: 'Terrain',
  partCount: 2,
  syncedAt: '2026-09-17T01:40:00Z',
}

const CLIFF: MirroredPart = {
  sourcePath: 'rocks/cliff-face-lp-tr-0112.stl',
  name: 'Cliff face, LP-TR-0112',
  partNumber: 'LP-TR-0112',
  tags: ['terrain'],
  licences: ['CC BY-NC 4.0'],
  sizeBytes: 4_812_000,
  format: 'stl',
  thumbnail: true,
}

const STONE: MirroredPart = {
  ...CLIFF,
  sourcePath: 'standing-stone-lp-tr-0140.stl',
  name: 'Standing stone, LP-TR-0140',
  partNumber: 'LP-TR-0140',
  licences: [],
  thumbnail: false,
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

function stub({
  share = { status: 200, body: TERRAIN as unknown },
  pages = [{ parts: [CLIFF, STONE], next: null as string | null }],
}: { share?: { status: number; body: unknown }; pages?: Array<{ parts: MirroredPart[]; next: string | null }> } = {}) {
  const urls: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      urls.push(url)
      if (url.includes('/parts?')) {
        const index = urls.filter((seen) => seen.includes('/parts?')).length - 1
        const page = pages[Math.min(index, pages.length - 1)]
        return { ok: share.status < 300, status: share.status, json: async () => page }
      }
      return { ok: share.status < 300, status: share.status, json: async () => share.body }
    }),
  )
  return urls
}

function renderPage() {
  const rootRoute = createRootRoute({ component: () => <SharedLibraryPage share={SHARE} /> })
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory({ initialEntries: ['/'] }) })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

test('its parts are shown with their licences and previews', async () => {
  stub()
  renderPage()

  await screen.findByText(CLIFF.name)
  expect(screen.getByText(strings.sharing.librarySharedBy('Ayşe’s workshop', 2))).toBeDefined()
  expect(screen.getByText(strings.sharing.licences('CC BY-NC 4.0'))).toBeDefined()
  expect(screen.getByText(strings.sharing.noLicence)).toBeDefined()
  const preview = screen.getByAltText(strings.parts.thumbnailAlt(CLIFF.name)) as HTMLImageElement
  expect(preview.getAttribute('src')).toBe(
    `/api/sharing/shares/${SHARE}/thumbnail?path=rocks%2Fcliff-face-lp-tr-0112.stl`,
  )
  expect(screen.queryByAltText(strings.parts.thumbnailAlt(STONE.name))).toBeNull()
})

test('show more asks for the page after the last part shown', async () => {
  const urls = stub({
    pages: [
      { parts: [CLIFF], next: CLIFF.sourcePath },
      { parts: [STONE], next: null },
    ],
  })
  renderPage()

  fireEvent.click(await screen.findByRole('button', { name: strings.sharing.showMore }))
  await screen.findByText(STONE.name)
  expect(urls.some((url) => url.includes('after=rocks%2Fcliff-face-lp-tr-0112.stl'))).toBe(true)
  expect(screen.queryByRole('button', { name: strings.sharing.showMore })).toBeNull()
})

test('a library no longer offered says so', async () => {
  stub({ share: { status: 404, body: { reason: 'noSuchShare' } } })
  renderPage()
  await screen.findByText(strings.sharing.libraryGone)
})

test('a catalogue not read yet says so rather than looking empty', async () => {
  stub({ share: { status: 200, body: { ...TERRAIN, syncedAt: null } }, pages: [{ parts: [], next: null }] })
  renderPage()
  await screen.findByText(strings.sharing.libraryNotReadYet)
  expect(screen.queryByText(strings.sharing.libraryEmpty)).toBeNull()
})

test('the tab names the shared library', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  stub()
  renderPage()
  await waitFor(() => expect(document.title).toBe(strings.sharing.libraryTitle('Terrain')))
})
