import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { SharedLibraryPage } from './sharing_.shares.$shareId'
import { strings } from '../lib/strings'
import type { LibraryId, MirroredPart, MirroredShare, PeerShareId, Pull, PullId } from '../lib/types'

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
  readFrom: null,
  readFromName: null,
  asOf: '2026-09-17T01:40:00Z',
  seeding: true,
  heldFiles: 1,
  listedFiles: 2,
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

const LIBRARIES = [
  { id: '01931b6e-0000-7000-8000-000000000001', name: 'Workshop', mode: 'hobby', partCount: 412 },
  { id: '01931b6e-0000-7000-8000-000000000002', name: 'Tabletop terrain', mode: 'hobby', partCount: 0 },
]

const QUEUED: Pull = {
  id: '01a0c7e2-4d11-7b20-9a31-7c2e5dab0900' as PullId,
  shareId: SHARE,
  shareName: 'Terrain',
  sharer: 'Ayşe’s workshop',
  libraryId: LIBRARIES[1]!.id as LibraryId,
  state: 'queued',
  filesTotal: 0,
  filesDone: 0,
  bytesTotal: 0,
  bytesDone: 0,
  batchId: null,
  error: null,
}

let posted: Array<{ url: string; body: unknown }> = []

function stub({
  share = { status: 200, body: TERRAIN as unknown },
  pages = [{ parts: [CLIFF, STONE], next: null as string | null }],
  pull = null as Pull | null,
  pulled = QUEUED,
}: {
  share?: { status: number; body: unknown }
  pages?: Array<{ parts: MirroredPart[]; next: string | null }>
  pull?: Pull | null
  pulled?: Pull
} = {}) {
  posted = []
  const urls: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      urls.push(url)
      if (init?.method === 'POST') {
        posted.push({ url, body: JSON.parse(init.body ?? 'null') })
        return { ok: true, status: 202, json: async () => pulled }
      }
      if (url.endsWith('/seeding')) {
        posted.push({ url, body: JSON.parse(init?.body ?? 'null') })
        return { ok: true, status: 204, json: async () => null }
      }
      if (url.endsWith('/pause') || url.endsWith('/resume')) {
        posted.push({ url, body: null })
        return { ok: true, status: 204, json: async () => null }
      }
      if (url.endsWith('/pull')) {
        return { ok: true, status: 200, json: async () => pull }
      }
      if (url === '/api/libraries') {
        return { ok: true, status: 200, json: async () => LIBRARIES }
      }
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

test('pull all records a pull into the chosen library and follows it', async () => {
  stub()
  renderPage()

  const into = (await screen.findByLabelText(strings.sharing.pullInto)) as HTMLSelectElement
  await waitFor(() => expect(into.options.length).toBe(2))
  fireEvent.change(into, { target: { value: LIBRARIES[1]!.id } })
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.pullAll }))

  await waitFor(() =>
    expect(posted).toEqual([{ url: `/api/sharing/shares/${SHARE}/pulls`, body: { libraryId: LIBRARIES[1]!.id } }]),
  )
})

test('a pull fetching says how far it has got, and cannot be started twice', async () => {
  stub({
    pull: { ...QUEUED, state: 'fetching', filesTotal: 138, filesDone: 12, bytesTotal: 1_048_576_000, bytesDone: 94_371_840 },
  })
  renderPage()
  await screen.findByText(strings.sharing.pullFetching(12, 138, 94_371_840, 1_048_576_000))
  expect((screen.getByRole('button', { name: strings.sharing.pullAll }) as HTMLButtonElement).disabled).toBe(true)
})

test('a pull that found everything already here says nothing was fetched', async () => {
  stub({ pull: { ...QUEUED, state: 'done' } })
  renderPage()
  await screen.findByText(strings.sharing.pullDone(0))
})

test('a stopped pull says why', async () => {
  stub({ pull: { ...QUEUED, state: 'failed', error: 'Terrain is no longer shared with this installation.' } })
  renderPage()
  await screen.findByText(strings.sharing.pullStopped('Terrain is no longer shared with this installation.'))
})

test('a library already pulled offers the library it was pulled into', async () => {
  stub({ pull: { ...QUEUED, state: 'done', filesTotal: 138 } })
  renderPage()
  await screen.findByText(strings.sharing.pullDone(138))
  const into = screen.getByLabelText(strings.sharing.pullInto) as HTMLSelectElement
  await waitFor(() => expect(into.value).toBe(LIBRARIES[1]!.id))
})

test('a pull waiting for the sharer says so, and can be paused', async () => {
  stub({ pull: { ...QUEUED, state: 'waiting', error: 'Waiting for Ayşe’s workshop to let you pull Terrain.' } })
  renderPage()
  await screen.findByText('Waiting for Ayşe’s workshop to let you pull Terrain.')
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.pause }))
  await waitFor(() => expect(posted.map((call) => call.url)).toEqual([`/api/sharing/pulls/${QUEUED.id}/pause`]))
})

test('a paused pull says what it kept, and resumes', async () => {
  stub({ pull: { ...QUEUED, state: 'paused', filesDone: 40, bytesDone: 312_000_000 } })
  renderPage()
  await screen.findByText(strings.sharing.pullPaused)
  expect((screen.getByRole('button', { name: strings.sharing.pullAll }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.resume }))
  await waitFor(() => expect(posted.map((call) => call.url)).toEqual([`/api/sharing/pulls/${QUEUED.id}/resume`]))
})

/**
 * Sharing S7: a folder read from another of its people while its owner was away.
 *
 * What is on the page is somebody else's reading of it, which may be older than the owner's own, so the page
 * says whose and when rather than letting it pass for a reading of its own.
 */
test('a folder read through somebody else says whose reading it is', async () => {
  stub({
    share: {
      status: 200,
      body: {
        ...TERRAIN,
        readFrom: 'b7d90e12f3a4b5c6b7d90e12f3a4b5c6b7d90e12f3a4b5c6b7d90e12f3a4b5c6',
        readFromName: 'Mira’s studio',
        asOf: '2026-09-17T09:12:00Z',
      },
    },
  })
  renderPage()

  await screen.findByText(strings.sharing.libraryRelayed('Mira’s studio', '2026-09-17T09:12:00Z'))
  expect(screen.queryByText(strings.sharing.librarySynced('2026-09-17T01:40:00Z'))).toBeNull()
})

/**
 * Sharing S8: this installation is one of the machines a folder's files can come from, and says so.
 *
 * Switching it off is not leaving the folder, so the line under the switch says what actually changes rather
 * than leaving somebody to guess what they just gave up.
 */
test('a held folder says what can be served from here, and the switch stops it', async () => {
  stub()
  renderPage()

  await screen.findByText(strings.sharing.seedingHeld(1, 2))
  const seeding = screen.getByLabelText(strings.sharing.seedingLabel)
  fireEvent.click(seeding)

  await waitFor(() =>
    expect(posted.some((call) => call.url.endsWith(`/api/sharing/shares/${SHARE}/seeding`))).toBe(true),
  )
  expect(posted.find((call) => call.url.endsWith('/seeding'))?.body).toEqual({ seeding: false })
})

test('a folder not being served says the folder and what was pulled stay', async () => {
  stub({ share: { status: 200, body: { ...TERRAIN, seeding: false } } })
  renderPage()

  await screen.findByText(strings.sharing.seedingOffNote)
  expect(screen.queryByText(strings.sharing.seedingHeld(1, 2))).toBeNull()
})
