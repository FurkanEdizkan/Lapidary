import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { ShareDialog } from './ShareDialog'
import { strings } from '../lib/strings'
import type { FolderNode, LicenceWarning, Peer } from '../lib/types'

/**
 * Sharing a category offers everything under it to the people ticked here, so the dialog's one job is to say
 * what that is — how many parts, how many carry no licence or a non-commercial one, and who it reaches —
 * before anything is sent. The warning never blocks (owner's decision, 2026-09-16), so what is asserted is
 * that it is shown; the member list is asserted on what leaves in the body, since that is what the share is.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001'
const TERRAIN: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0001',
  parentId: null,
  name: 'Terrain',
  slug: 'Terrain',
  partCount: 34,
}

type Call = { url: string; method: string; body: unknown }

beforeEach(() => {
  vi.unstubAllGlobals()
})

const WORKSHOP: Peer = {
  deviceId: 'a3f1c2d40e5b6798a3f1c2d40e5b6798a3f1c2d40e5b6798a3f1c2d40e5b6798',
  name: 'Ayşe’s workshop',
  address: 'workshop.lan:8443',
  addedAt: '2026-09-14T09:12:00Z',
  lastSeenAt: '2026-09-17T08:03:00Z',
  lastError: null,
  online: true,
}
const BENCH: Peer = {
  deviceId: 'b7d90e12f3a4b5c6b7d90e12f3a4b5c6b7d90e12f3a4b5c6b7d90e12f3a4b5c6',
  name: 'Mehmet’s bench',
  address: 'bench.lan:8443',
  addedAt: '2026-09-15T16:40:00Z',
  lastSeenAt: null,
  lastError: null,
  online: false,
}

function stub(
  warning: LicenceWarning,
  shared: { status: number; body: unknown } = { status: 200, body: {} },
  paired: Peer[] = [],
) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET'
      calls.push({ url, method, body: init?.body ? JSON.parse(String(init.body)) : undefined })
      if (url.includes('/shares/preview')) return { ok: true, status: 200, json: async () => warning }
      if (url.endsWith('/shares') && method === 'POST') {
        return { ok: shared.status < 300, status: shared.status, json: async () => shared.body }
      }
      if (url.endsWith('/sharing/peers')) return { ok: true, status: 200, json: async () => paired }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  return calls
}

function renderDialog() {
  const onClose = vi.fn()
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ShareDialog library={LIBRARY} folder={TERRAIN} onClose={onClose} />
    </QueryClientProvider>,
  )
  return onClose
}

test('the warning is counted and shown before anything is shared', async () => {
  const calls = stub({ parts: 34, unrecorded: 5, nonCommercial: 3 })
  renderDialog()

  await screen.findByText(strings.sharing.shareBody(34))
  expect(screen.getByText(strings.sharing.shareUnrecorded(5))).toBeDefined()
  expect(screen.getByText(strings.sharing.shareNonCommercial(3))).toBeDefined()
  expect(calls[0]?.url).toContain(`folderId=${TERRAIN.id}`)
  expect(calls.some((call) => call.method === 'POST')).toBe(false)
})

test('a category whose parts are all licensed says so rather than saying nothing', async () => {
  stub({ parts: 12, unrecorded: 0, nonCommercial: 0 })
  renderDialog()
  await screen.findByText(strings.sharing.shareLicencesClear)
})

test('confirming shares that category and closes', async () => {
  const calls = stub({ parts: 34, unrecorded: 0, nonCommercial: 0 })
  const onClose = renderDialog()

  await screen.findByText(strings.sharing.shareBody(34))
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.shareConfirm }))

  await waitFor(() => expect(onClose).toHaveBeenCalled())
  const posted = calls.find((call) => call.method === 'POST')
  expect(posted?.url).toBe(`/api/libraries/${LIBRARY}/shares`)
  expect(posted?.body).toEqual({ folderId: TERRAIN.id })
})

test('a refusal keeps the dialog open and says the server’s words', async () => {
  const refusal =
    'This library has no such category, so it cannot be shared. It may have been deleted: reload the tree and choose it again.'
  stub({ parts: 34, unrecorded: 0, nonCommercial: 0 }, { status: 404, body: { message: refusal, reason: 'noSuchCategory' } })
  const onClose = renderDialog()

  await screen.findByText(strings.sharing.shareBody(34))
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.shareConfirm }))

  expect((await screen.findByRole('alert')).textContent).toBe(refusal)
  expect(onClose).not.toHaveBeenCalled()
})

/** Sharing sends a category to other people, so the dialog opens on the answer that sends nothing. */
test('cancel is where the dialog opens, and it shares nothing', async () => {
  const calls = stub({ parts: 34, unrecorded: 0, nonCommercial: 0 })
  const onClose = renderDialog()

  const cancel = await screen.findByRole('button', { name: strings.sharing.shareCancel })
  expect(document.activeElement).toBe(cancel)
  fireEvent.click(cancel)
  expect(onClose).toHaveBeenCalled()
  expect(calls.some((call) => call.method === 'POST')).toBe(false)
})

test('asking first is sent only when it is chosen', async () => {
  const calls = stub({ parts: 34, unrecorded: 0, nonCommercial: 0 })
  const onClose = renderDialog()

  await screen.findByText(strings.sharing.shareBody(34))
  fireEvent.click(screen.getByLabelText(strings.sharing.askFirstLabel))
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.shareConfirm }))

  await waitFor(() => expect(onClose).toHaveBeenCalled())
  expect(calls.find((call) => call.method === 'POST')?.body).toEqual({ folderId: TERRAIN.id, asksFirst: true })
})

test('a new share goes to everybody paired, each of them named', async () => {
  const calls = stub({ parts: 34, unrecorded: 0, nonCommercial: 0 }, { status: 200, body: {} }, [WORKSHOP, BENCH])
  const onClose = renderDialog()

  await screen.findByRole('checkbox', { name: /Ayşe/ })
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.shareConfirm }))

  await waitFor(() => expect(onClose).toHaveBeenCalled())
  expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
    folderId: TERRAIN.id,
    memberDeviceIds: [WORKSHOP.deviceId, BENCH.deviceId],
  })
})

test('unticking somebody leaves them out of who the folder goes to', async () => {
  const calls = stub({ parts: 34, unrecorded: 0, nonCommercial: 0 }, { status: 200, body: {} }, [WORKSHOP, BENCH])
  const onClose = renderDialog()

  fireEvent.click(await screen.findByRole('checkbox', { name: /Mehmet/ }))
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.shareConfirm }))

  await waitFor(() => expect(onClose).toHaveBeenCalled())
  expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
    folderId: TERRAIN.id,
    memberDeviceIds: [WORKSHOP.deviceId],
  })
})
