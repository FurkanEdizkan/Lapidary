import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { Index } from './index'
import { strings } from '../lib/strings'

function renderIndex() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <Index />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.restoreAllMocks()
})

// The expected text is written out literally rather than read from strings.ts. Asserting
// against the same constant the component renders from would pass even if the string were
// corrupted, because both sides would move together.
test('renders the connected state from a healthy response', async () => {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({ status: 'ok', database: { major: 18, reachable: true } }),
  })
  vi.stubGlobal('fetch', fetchMock)
  renderIndex()
  expect(await screen.findByText('Connected — PostgreSQL 18')).toBeDefined()
  // Pin the endpoint. The stub resolves regardless of what it is called with, so without
  // this a typo in the path would be invisible.
  expect(fetchMock).toHaveBeenCalledWith('/api/healthz')
})

test('renders the checking state while the request is in flight', () => {
  // A promise that never settles holds the query in its pending state.
  vi.stubGlobal('fetch', vi.fn(() => new Promise(() => {})))
  renderIndex()
  expect(screen.getByText('Checking the server…')).toBeDefined()
})

test('renders an actionable message when the server is unreachable', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 503 }))
  renderIndex()
  expect(
    await screen.findByText('Could not reach the server. Check that the api and db services are running.'),
  ).toBeDefined()
})

// Asserted against the strings.ts constant, not a literal, so this test breaks only if the
// component stops rendering strings.emptyLibrary.body at all — e.g. it gets hardcoded or
// dropped — not whenever the copy itself is edited.
test('renders the empty-library copy from strings.ts', () => {
  vi.stubGlobal('fetch', vi.fn(() => new Promise(() => {})))
  renderIndex()
  expect(screen.getByText(strings.emptyLibrary.title)).toBeDefined()
  expect(screen.getByText(strings.emptyLibrary.body)).toBeDefined()
})
