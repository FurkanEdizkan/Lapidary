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
function type(label: string, value: string) {
  const holder = [...document.querySelectorAll('label')].find((l) => l.firstChild?.nodeValue === label)
  const input = holder?.querySelector('input')
  if (input == null) throw new Error(`no field labelled ${label}`)
  fireEvent.change(input, { target: { value } })
}

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

  expect(await screen.findByText('That setup code is not the one this server printed.')).toBeTruthy()
  expect(screen.getByText(strings.setup.step(1, 3))).toBeTruthy()
  const post = fetchMock.mock.calls.find(([url, init]) => url === '/api/setup' && init?.method === 'POST')
  expect(JSON.parse(String(post?.[1]?.body))).toEqual({
    code: 'K7QF-M2XD-PR4T',
    username: 'furkan',
    displayName: 'Furkan Edizkan',
    password: 'correct horse battery',
    importLinks: false,
  })
})
