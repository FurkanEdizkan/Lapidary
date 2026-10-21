import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { SettingsPage } from './settings'
import { strings } from '../lib/strings'
import type { SiteSettings } from '../lib/types'

/**
 * The Settings page (F3): a key is shown by its last four characters, saved, removed, and checked
 * — the typed one before it is saved, the saved one when the field is empty.
 */

const SETTINGS: SiteSettings = {
  enabled: true,
  forcedOff: false,
  sites: [
    { site: 'printables', name: 'Printables', needsKey: false, hint: null, checkedOk: null, checkedAt: null },
    { site: 'thingiverse', name: 'Thingiverse', needsKey: true, hint: '9f2e', checkedOk: true, checkedAt: '2026-10-16T09:30:00Z' },
    { site: 'myminifactory', name: 'MyMiniFactory', needsKey: true, hint: null, checkedOk: null, checkedAt: null },
  ],
}

beforeEach(() => vi.unstubAllGlobals())

function stub(settings: SiteSettings = SETTINGS) {
  const calls: { url: string; method: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      const method = init?.method ?? 'GET'
      calls.push({ url, method, body: init?.body === undefined ? undefined : JSON.parse(init.body) })
      if (url.endsWith('/check')) {
        return { ok: true, status: 200, json: async () => ({ ok: true, account: 'scantheworld', message: 'MyMiniFactory answers: the key works for scantheworld.' }) }
      }
      if (method !== 'GET') return { ok: true, status: 204, json: async () => null }
      if (url === '/api/settings/server') return { ok: true, status: 200, json: async () => ({ port: 32000, lockedBy: 'LAPIDARY_PORT' }) }
      if (url === '/api/settings/workers') return { ok: true, status: 200, json: async () => ({ workers: 2, lockedBy: null }) }
      return { ok: true, status: 200, json: async () => settings }
    }),
  )
  return calls
}

function renderPage() {
  const rootRoute = createRootRoute({ component: () => <SettingsPage /> })
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory({ initialEntries: ['/'] }) })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

const row = async (name: string) => (await screen.findByRole('heading', { name })).closest('li') as HTMLElement

test('each site says what is saved, by its last four characters, and Printables needs no key', async () => {
  stub()
  renderPage()
  expect((await row('Thingiverse')).textContent).toContain(strings.settings.saved('9f2e'))
  expect((await row('MyMiniFactory')).textContent).toContain(strings.settings.noneSaved)
  const printables = await row('Printables')
  expect(printables.textContent).toContain(strings.settings.noKeyNeeded)
  expect(within(printables).queryByLabelText(strings.settings.keyLabel('Printables'))).toBeNull()
})

test('a typed key is checked before it is saved, then saved, and a saved one can be removed', async () => {
  const calls = stub()
  renderPage()
  const mmf = await row('MyMiniFactory')
  fireEvent.change(within(mmf).getByLabelText(strings.settings.keyLabel('MyMiniFactory')), {
    target: { value: 'mmf-0193-token' },
  })
  fireEvent.click(within(mmf).getByRole('button', { name: strings.settings.check }))
  expect(await within(mmf).findByText('MyMiniFactory answers: the key works for scantheworld.')).toBeTruthy()
  expect(calls.find((c) => c.url.endsWith('/myminifactory/check'))?.body).toEqual({ key: 'mmf-0193-token' })
  expect(calls.some((c) => c.method === 'PUT'), 'checking does not save').toBe(false)

  fireEvent.click(within(mmf).getByRole('button', { name: strings.settings.save }))
  await waitFor(() =>
    expect(calls.find((c) => c.method === 'PUT')).toEqual({
      url: '/api/settings/sites/myminifactory',
      method: 'PUT',
      body: { key: 'mmf-0193-token' },
    }),
  )

  const tv = await row('Thingiverse')
  fireEvent.click(within(tv).getByRole('button', { name: strings.settings.checkSaved }))
  await waitFor(() => expect(calls.find((c) => c.url.endsWith('/thingiverse/check'))?.body).toEqual({}))
  fireEvent.click(within(tv).getByRole('button', { name: strings.settings.remove }))
  await waitFor(() =>
    expect(calls.some((c) => c.method === 'DELETE' && c.url === '/api/settings/sites/thingiverse')).toBe(true),
  )
})

test('switched off by deploy/.env, the page says so and nothing can be checked', async () => {
  stub({ ...SETTINGS, enabled: false, forcedOff: true })
  renderPage()
  expect(await screen.findByText(strings.settings.switchedOff)).toBeTruthy()
  const printables = await row('Printables')
  expect((within(printables).getByRole('button', { name: strings.settings.checkSite }) as HTMLButtonElement).disabled).toBe(true)
})

/** A17's audit: the locked port was drawn as an input. A value nobody can change here is a fact, not a field. */
test('a port fixed by deploy/.env reads as a fact, not as a field', async () => {
  stub()
  renderPage()
  const value = await screen.findByText('32000')
  expect(value.tagName).toBe('DD')
  expect(screen.queryByRole('spinbutton', { name: strings.settings.port })).toBeNull()
  screen.getByText(strings.settings.portLocked('LAPIDARY_PORT'))
})

/** A41: the wizard's import switch can be changed afterwards, by an admin, unless deploy/.env decides it. */
test('importing from model sites is switched off from Settings', async () => {
  const calls = stub()
  renderPage()
  const box = (await screen.findByRole('checkbox', { name: new RegExp(strings.settings.importLinks) })) as HTMLInputElement
  expect(box.checked).toBe(true)
  fireEvent.click(box)
  await waitFor(() =>
    expect(calls).toContainEqual({ url: '/api/settings/sites', method: 'PUT', body: { enabled: false } }),
  )
})

/** A41: how many imports run at once is saved from the Server section, which restarts Lapidary. */
test('imports at once is saved as a number', async () => {
  const calls = stub()
  renderPage()
  const field = await screen.findByRole('spinbutton', { name: strings.settings.workers })
  fireEvent.change(field, { target: { value: '3' } })
  fireEvent.click(within(field.closest('form') as HTMLElement).getByRole('button', { name: strings.settings.save }))
  await waitFor(() => expect(calls).toContainEqual({ url: '/api/settings/workers', method: 'PUT', body: { workers: 3 } }))
})
