import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { SharingPage } from './sharing'
import { strings } from '../lib/strings'
import type { LibraryId, MirroredShare, Peer, Pull, PullId, ShareRequest, ShareSummary, SharingIdentity } from '../lib/types'

/**
 * The sharing page, which is where two people who know each other pair their installations. What it
 * must get right is visible only here: the id a person reads off to somebody else, exactly what they
 * pasted going to the server, the server's own words when it refuses, and a status that says why
 * somebody is not online rather than only that they are not.
 */

const HERE: SharingIdentity = {
  deviceId: '4ZQ7M-0V5KQ-7M2JD-H8R3N-6WXT1-9BCPE-2FJ4K-5SGVA-8NDYR-3HQ6M-PG',
  name: 'Furkan’s workbench',
}

const AYSE: Peer = {
  deviceId: '7M2JD-4ZQ7M-H8R3N-0V5KQ-9BCPE-6WXT1-5SGVA-2FJ4K-3HQ6M-8NDYR-K0',
  address: '192.168.1.24:8082',
  name: 'Ayşe’s workshop',
  addedAt: '2026-09-16T09:12:00Z',
  lastSeenAt: '2026-09-16T11:40:05Z',
  lastError: null,
  online: true,
}

const MAKERSPACE: Peer = {
  deviceId: 'H8R3N-6WXT1-4ZQ7M-9BCPE-0V5KQ-7M2JD-8NDYR-2FJ4K-5SGVA-3HQ6M-T0',
  address: '100.101.12.7:8082',
  name: 'Kadıköy makerspace',
  addedAt: '2026-09-16T09:30:00Z',
  lastSeenAt: '2026-09-16T10:02:47Z',
  lastError:
    'The installation at 100.101.12.7:8082 turned this one away: it has not added this installation’s device id, or has removed it.',
  online: false,
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

type Call = { url: string; method: string; body: unknown }

/** Answers the page's reads, and records every call so a test can assert which writes were made. */
function stub({
  identity = HERE,
  peers = [AYSE, MAKERSPACE],
  pair = { status: 200, body: AYSE as unknown },
  shares = [],
  theirs = [],
  requests = [],
  pulls = [],
}: {
  identity?: SharingIdentity
  peers?: Peer[]
  pair?: { status: number; body: unknown }
  shares?: ShareSummary[]
  theirs?: MirroredShare[]
  requests?: ShareRequest[]
  pulls?: Pull[]
} = {}) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET'
      calls.push({ url, method, body: init?.body ? JSON.parse(String(init.body)) : undefined })
      const answer = (status: number, body: unknown) => ({
        ok: status < 300,
        status,
        json: async () => body,
      })
      if (url === '/api/sharing/identity') return method === 'GET' ? answer(200, identity) : answer(204, {})
      if (url === '/api/sharing/peers' && method === 'POST') return answer(pair.status, pair.body)
      if (url === '/api/sharing/peers') return answer(200, peers)
      if (url === '/api/shares') return answer(200, shares)
      if (url === '/api/shares/requests') return answer(200, requests)
      if (url === '/api/sharing/pulls') return answer(200, pulls)
      if (method === 'PUT' && url.includes('/grants/')) return answer(204, {})
      if (url.startsWith('/api/sharing/peers/') && url.endsWith('/shares')) return answer(200, theirs)
      if (method === 'DELETE') return answer(204, {})
      return answer(404, {})
    }),
  )
  return calls
}

function renderPage() {
  const rootRoute = createRootRoute({ component: () => <SharingPage /> })
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

test('with sharing switched off, the page says how to switch it on', async () => {
  stub({ identity: { deviceId: null, name: null }, peers: [] })
  renderPage()
  await screen.findByText(strings.sharing.off)
})

test('this installation’s device id is shown, to be read off to somebody', async () => {
  stub()
  renderPage()
  await screen.findByText(HERE.deviceId as string)
})

test('pairing sends exactly what was pasted, then empties the form', async () => {
  const calls = stub()
  renderPage()

  const deviceId = await screen.findByLabelText(strings.sharing.pairDeviceId)
  const address = screen.getByLabelText(strings.sharing.pairAddress)
  fireEvent.change(deviceId, { target: { value: AYSE.deviceId } })
  fireEvent.change(address, { target: { value: AYSE.address } })
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.pair }))

  await waitFor(() =>
    expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
      deviceId: AYSE.deviceId,
      address: AYSE.address,
    }),
  )
  await waitFor(() => expect((deviceId as HTMLInputElement).value).toBe(''))
  expect((address as HTMLInputElement).value).toBe('')
})

test('a refused pairing shows the server’s own words and keeps what was typed', async () => {
  const refusal =
    'That is this installation’s own device id. Paste the id shown on the sharing page of the other machine.'
  stub({ pair: { status: 400, body: { message: refusal, reason: 'ownDeviceId' } } })
  renderPage()

  const deviceId = await screen.findByLabelText(strings.sharing.pairDeviceId)
  fireEvent.change(deviceId, { target: { value: HERE.deviceId } })
  fireEvent.change(screen.getByLabelText(strings.sharing.pairAddress), { target: { value: '127.0.0.1:8082' } })
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.pair }))

  expect((await screen.findByRole('alert')).textContent).toBe(refusal)
  expect((deviceId as HTMLInputElement).value).toBe(HERE.deviceId)
})

/**
 * Online is a word, not only a colour, and somebody offline says why: the last hello's own sentence is
 * what tells a person whether the other side is switched off or has not added them.
 */
test('somebody online reads as online, and somebody offline says why', async () => {
  stub()
  renderPage()

  const ayse = (await screen.findByText('Ayşe’s workshop')).closest('li') as HTMLElement
  expect(within(ayse).getByText(strings.sharing.online)).toBeDefined()

  const makerspace = screen.getByText('Kadıköy makerspace').closest('li') as HTMLElement
  expect(within(makerspace).queryByText(strings.sharing.online)).toBeNull()
  expect(within(makerspace).getByText(MAKERSPACE.lastError as string)).toBeDefined()
})

test('removing somebody deletes that row’s pairing and no other', async () => {
  const calls = stub()
  renderPage()

  const makerspace = (await screen.findByText('Kadıköy makerspace')).closest('li') as HTMLElement
  fireEvent.click(
    within(makerspace).getByRole('button', { name: strings.sharing.removeLabel('Kadıköy makerspace') }),
  )

  await waitFor(() =>
    expect(calls.some((call) => call.method === 'DELETE' && call.url.includes(MAKERSPACE.deviceId))).toBe(true),
  )
  expect(calls.some((call) => call.method === 'DELETE' && call.url.includes(AYSE.deviceId))).toBe(false)
})

/** SC 2.4.2. Seeded with `index.html`'s tag first, so the assertion can fail. */
test('the tab says which page this is', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  stub()
  renderPage()
  await waitFor(() => expect(document.title).toBe(strings.titles.sharing))
})

test('what this installation shares is listed, and stopping one withdraws only that one', async () => {
  const calls = stub({
    shares: [
      { id: '01a07c41-5d22-7b03-9014-7e2f6dab0001', name: 'Terrain', partCount: 34, asksFirst: true },
      { id: '01a07c41-5d22-7b03-9014-7e2f6dab0002', name: 'Fasteners', partCount: 1, asksFirst: false },
    ],
  })
  renderPage()

  await screen.findByText(strings.sharing.ownShareParts(34))
  expect(screen.getByText(strings.sharing.ownShareParts(1))).toBeDefined()
  expect(screen.getAllByText(strings.sharing.asksFirst)).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.stopSharingLabel('Fasteners') }))

  await waitFor(() =>
    expect(calls.some((call) => call.method === 'DELETE' && call.url === '/api/shares/01a07c41-5d22-7b03-9014-7e2f6dab0002')).toBe(true),
  )
  expect(calls.some((call) => call.method === 'DELETE' && call.url.endsWith('0001'))).toBe(false)
})

test('what somebody shares is listed under them, each linking to it', async () => {
  stub({
    theirs: [
      {
        id: '01a0c7e2-4d11-7b20-9a31-7c2e5dab0001',
        deviceId: AYSE.deviceId,
        sharer: 'Ayşe’s workshop',
        name: 'Terrain',
        partCount: 998,
        syncedAt: '2026-09-17T01:40:00Z',
      },
    ],
  })
  renderPage()

  const links = await screen.findAllByRole('link', { name: strings.sharing.theirShareParts('Terrain', 998) })
  expect(links[0]?.getAttribute('href')).toBe('/sharing/shares/01a0c7e2-4d11-7b20-9a31-7c2e5dab0001')
})

test('who asked to pull is listed with the answer given, and letting them pull sends that answer', async () => {
  const calls = stub({
    requests: [
      {
        shareId: '01a07c41-5d22-7b03-9014-7e2f6dab0001',
        shareName: 'Terrain',
        deviceId: AYSE.deviceId,
        name: 'Ayşe’s workshop',
        state: 'asked',
        askedAt: '2026-09-17T02:10:00Z',
      },
      {
        shareId: '01a07c41-5d22-7b03-9014-7e2f6dab0001',
        shareName: 'Terrain',
        deviceId: MAKERSPACE.deviceId,
        name: null,
        state: 'denied',
        askedAt: '2026-09-17T01:55:00Z',
      },
    ],
  })
  renderPage()

  await screen.findByText(strings.sharing.requestLine('Ayşe’s workshop', 'Terrain'))
  expect(screen.getByText(strings.sharing.requestDenied)).toBeDefined()
  const declineAgain = screen.getByRole('button', {
    name: strings.sharing.denyLabel(MAKERSPACE.deviceId, 'Terrain'),
  }) as HTMLButtonElement
  expect(declineAgain.disabled).toBe(true)

  fireEvent.click(screen.getByRole('button', { name: strings.sharing.grantLabel('Ayşe’s workshop', 'Terrain') }))
  await waitFor(() =>
    expect(
      calls.find((call) => call.method === 'PUT'),
    ).toEqual({
      url: `/api/shares/01a07c41-5d22-7b03-9014-7e2f6dab0001/grants/${encodeURIComponent(AYSE.deviceId)}`,
      method: 'PUT',
      body: { granted: true },
    }),
  )
})

test('pulls are listed by the share they were of, and one whose sharer stopped sharing says so', async () => {
  stub({
    pulls: [
      {
        id: '01a0c7e2-4d11-7b20-9a31-7c2e5dab0900' as PullId,
        shareId: null,
        shareName: 'Terrain',
        sharer: 'Ayşe’s workshop',
        libraryId: '01931b6e-0000-7000-8000-000000000001' as LibraryId,
        state: 'failed',
        filesTotal: 138,
        filesDone: 49,
        bytesTotal: 1_077_177_442,
        bytesDone: 370_925_966,
        batchId: null,
        error: 'Ayşe’s workshop no longer shares Terrain with you. Parts already pulled stay.',
      },
    ],
  })
  renderPage()
  await screen.findByText(strings.sharing.pullLine('Terrain', 'Ayşe’s workshop'))
  expect(
    screen.getByText(
      strings.sharing.pullStopped('Ayşe’s workshop no longer shares Terrain with you. Parts already pulled stay.'),
    ),
  ).toBeDefined()
})
