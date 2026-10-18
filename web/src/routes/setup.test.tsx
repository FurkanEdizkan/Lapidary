import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, test, vi } from 'vitest'
import { strings } from '../lib/strings'
import { SetupPage } from './setup'

afterEach(() => vi.unstubAllGlobals())

const STATUS = {
  required: true,
  suggestedName: 'Furkan Edizkan',
  hostIngestRoot: '/srv/models',
  hostInboxRoot: null,
  hostStorageRoot: '/srv/lapidary',
  importLinksForcedOff: false,
}

const FOLDER = { path: '/srv/models', store: '/srv/lapidary', lockedBy: null, storeLockedBy: null, canChoose: true }

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

function renderSetup() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SetupPage />
    </QueryClientProvider>,
  )
}

/** A field by its label's own first words: a label holds its hint as well. */
function fieldFor(label: string): HTMLInputElement {
  const holder = [...document.querySelectorAll('label')].find((l) => l.firstChild?.nodeValue === label)
  const input = holder?.querySelector('input')
  if (input == null) throw new Error(`no field labelled ${label}`)
  return input
}

function type(label: string, value: string) {
  fireEvent.change(fieldFor(label), { target: { value } })
}

/** The stepper's current step, by `aria-current` (there must be exactly one). */
function current(): string | null {
  const marked = document.querySelectorAll('[aria-current="step"]')
  expect(marked).toHaveLength(1)
  return marked[0]!.textContent
}

/** A motion-capable browser: the intro's path. `test-setup` has no `matchMedia`, which reads as reduced motion. */
function motionAllowed() {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }))
}

/** A fresh module, so the once-per-page-load intro has not played yet. */
async function freshSetup() {
  vi.resetModules()
  const { SetupPage: Page } = await import('./setup')
  const { strings: fresh } = await import('../lib/strings')
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Page />
    </QueryClientProvider>,
  )
  return fresh
}

function statusOnly() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => (url === '/api/setup/storage' ? json(200, FOLDER) : json(200, STATUS))),
  )
}

test('under reduced motion the intro never plays: the wizard is there at once', async () => {
  statusOnly()
  renderSetup()
  expect(await screen.findByRole('heading', { name: strings.setup.title })).toBeTruthy()
  expect(screen.queryByRole('button', { name: strings.setup.skip })).toBeNull()
  expect(document.title === '' || document.querySelector('title')?.textContent === strings.titles.setup).toBe(true)
})

test('Skip ends the intro at its end state, with no accent left', async () => {
  motionAllowed()
  statusOnly()
  const fresh = await freshSetup()
  // The title is there while the intro plays; the wizard is not.
  expect(document.querySelector('title')?.textContent).toBe(fresh.titles.setup)
  const skip = screen.getByRole('button', { name: fresh.setup.skip })
  expect(screen.queryByRole('heading', { name: fresh.setup.title })).toBeNull()
  fireEvent.click(skip)
  const soon = { timeout: 200 }
  expect(await screen.findByRole('heading', { name: fresh.setup.title }, soon)).toBeTruthy()
  await waitFor(() => expect(screen.queryByRole('button', { name: fresh.setup.skip })).toBeNull(), soon)
  expect(document.querySelector('[data-draw-head]')).toBeNull()
  expect(document.activeElement?.tagName).toBe('H2')
})

test('any key ends the intro', async () => {
  motionAllowed()
  statusOnly()
  const fresh = await freshSetup()
  expect(screen.getByRole('button', { name: fresh.setup.skip })).toBeTruthy()
  fireEvent.keyDown(window, { key: 'Tab' })
  // At once: well inside the intro's own 900 ms, which would end it anyway.
  const soon = { timeout: 200 }
  expect(await screen.findByRole('heading', { name: fresh.setup.title }, soon)).toBeTruthy()
  await waitFor(() => expect(screen.queryByRole('button', { name: fresh.setup.skip })).toBeNull(), soon)
})

test('the stepper marks the current step, and Next and Back move it, keeping what was typed', async () => {
  statusOnly()
  renderSetup()
  await screen.findByDisplayValue('Furkan Edizkan')
  expect(current()).toBe('Account')
  type(strings.setup.codeLabel, 'K7QF-M2XD-PR4T')
  type(strings.setup.usernameLabel, 'furkan')
  type(strings.setup.passwordLabel, 'correct horse battery')
  type(strings.setup.confirmLabel, 'correct horse battery')
  fireEvent.click(screen.getByRole('button', { name: strings.setup.next }))
  await screen.findByText(strings.setup.serverTitle)
  expect(current()).toBe('Server')
  // One step at a time: the account step has left.
  expect(document.querySelectorAll('section')).toHaveLength(1)
  expect(screen.getByText(strings.setup.step(2, 3, 'Server'))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: strings.setup.next }))
  await screen.findByText(strings.setup.optionsTitle, { selector: 'h3' })
  expect(current()).toBe('Options')
  // Done steps say so to a screen reader as well as with the check.
  expect(screen.getAllByText(strings.setup.stepDone)).toHaveLength(2)
  fireEvent.click(screen.getByRole('button', { name: strings.setup.back }))
  fireEvent.click(await screen.findByRole('button', { name: strings.setup.back }))
  await screen.findByText(strings.setup.accountTitle)
  expect(current()).toBe('Account')
  expect(fieldFor(strings.setup.usernameLabel).value).toBe('furkan')
})

test('an install that is set up says so, with Sign in as the way in', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => json(200, { ...STATUS, required: false })),
  )
  renderSetup()
  expect(await screen.findByRole('heading', { name: strings.setup.alreadyTitle })).toBeTruthy()
  expect(screen.getByText(strings.setup.alreadyLead)).toBeTruthy()
  expect(screen.getByRole('link', { name: strings.login.submit }).getAttribute('href')).toBe('/login')
  expect(document.querySelector('[aria-current="step"]')).toBeNull()
})

test('the wizard checks the account, shows the server, and a wrong code sends it back to step one', async () => {
  const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/setup' && init?.method === 'POST') {
      return json(403, { code: 'wrong_code', message: 'That setup code is not the one this server printed.' })
    }
    if (url === '/api/setup/storage') return json(200, FOLDER)
    if (url === '/api/setup/storage/folders' && init?.method === 'POST') {
      const { path, name } = JSON.parse(String(init.body)) as { path: string; name: string }
      return json(201, {
        path: `${path}/${name}`,
        root: '/',
        parent: path,
        folders: [],
        choosable: true,
        problem: null,
      })
    }
    if (url.startsWith('/api/setup/storage/folders')) {
      const path = new URL(url, 'http://lapidary.test').searchParams.get('path') ?? '/srv/models'
      return json(200, {
        path,
        root: '/',
        parent: path === '/' ? null : '/',
        folders: path === '/srv/models' ? ['printables'] : [],
        choosable: true,
        problem: null,
      })
    }
    return json(200, STATUS)
  })
  vi.stubGlobal('fetch', fetchMock)
  renderSetup()

  // The name this install already used is offered.
  expect(await screen.findByDisplayValue('Furkan Edizkan')).toBeTruthy()
  type(strings.setup.codeLabel, 'K7QF-M2XD-PR4T')
  type(strings.setup.usernameLabel, 'furkan')
  type(strings.setup.passwordLabel, 'correct horse battery')
  type(strings.setup.confirmLabel, 'correct horse batterz')
  fireEvent.click(screen.getByRole('button', { name: strings.setup.next }))
  expect(screen.getByRole('alert').textContent).toBe(strings.setup.passwordsDiffer)

  type(strings.setup.confirmLabel, 'correct horse battery')
  fireEvent.click(screen.getByRole('button', { name: strings.setup.next }))
  expect(await screen.findByText('/srv/lapidary')).toBeTruthy()
  expect(screen.getByText('/srv/models')).toBeTruthy()
  // The folder picker, before anybody can sign in, asks with the setup code.
  const [chooseModels] = await screen.findAllByRole('button', { name: strings.folderPicker.choose })
  if (chooseModels === undefined) throw new Error('no Choose button')
  fireEvent.click(chooseModels)
  fireEvent.click(await screen.findByRole('button', { name: strings.folderPicker.open('printables') }))
  expect(await screen.findByDisplayValue('/srv/models/printables')).toBeTruthy()
  // Listed, so it can be chosen.
  await screen.findByText(strings.folderPicker.empty)
  // New folder makes one inside it, with the setup code, and opens it.
  // Escape leaves the name, not the whole picker.
  fireEvent.click(screen.getByRole('button', { name: strings.folderPicker.newFolder }))
  fireEvent.keyDown(screen.getByLabelText(strings.folderPicker.newName), { key: 'Escape' })
  expect(screen.queryByLabelText(strings.folderPicker.newName)).toBeNull()
  expect(screen.getByRole('dialog')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: strings.folderPicker.newFolder }))
  fireEvent.change(screen.getByLabelText(strings.folderPicker.newName), { target: { value: 'Benchy' } })
  fireEvent.click(screen.getByRole('button', { name: strings.folderPicker.make }))
  expect(await screen.findByDisplayValue('/srv/models/printables/Benchy')).toBeTruthy()
  const made = fetchMock.mock.calls.find(
    ([url, init]) => String(url) === '/api/setup/storage/folders' && init?.method === 'POST',
  )
  expect(JSON.parse(String(made?.[1]?.body))).toEqual({ path: '/srv/models/printables', name: 'Benchy' })
  expect(new Headers(made?.[1]?.headers).get('x-setup-code')).toBe('K7QF-M2XD-PR4T')
  await waitFor(() =>
    expect(screen.getByRole('button', { name: strings.folderPicker.use })).toHaveProperty('disabled', false),
  )
  fireEvent.click(screen.getByRole('button', { name: strings.folderPicker.use }))
  expect(screen.getByText('/srv/models/printables/Benchy')).toBeTruthy()
  // Lapidary's own files have a picker of their own.
  expect(screen.getAllByRole('button', { name: strings.folderPicker.choose })).toHaveLength(2)
  const listed = fetchMock.mock.calls.find(([url]) => String(url).startsWith('/api/setup/storage/folders'))
  expect(new Headers(listed?.[1]?.headers).get('x-setup-code')).toBe('K7QF-M2XD-PR4T')

  fireEvent.click(screen.getByRole('button', { name: strings.setup.next }))
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: strings.setup.finish }))

  // Back on step one, with the server's sentence under the code it is about, and the code field focused.
  expect(await screen.findByText('That setup code is not the one this server printed.')).toBeTruthy()
  expect(screen.getByText(strings.setup.step(1, 3, 'Account'))).toBeTruthy()
  expect(current()).toBe('Account')
  const codeField = fieldFor(strings.setup.codeLabel)
  expect(codeField.getAttribute('aria-invalid')).toBe('true')
  expect(document.activeElement).toBe(codeField)
  expect(codeField.closest('label')?.nextElementSibling?.getAttribute('role')).toBe('alert')
  const post = fetchMock.mock.calls.find(([url, init]) => url === '/api/setup' && init?.method === 'POST')
  expect(JSON.parse(String(post?.[1]?.body))).toEqual({
    code: 'K7QF-M2XD-PR4T',
    username: 'furkan',
    displayName: 'Furkan Edizkan',
    password: 'correct horse battery',
    importLinks: false,
  })
})
