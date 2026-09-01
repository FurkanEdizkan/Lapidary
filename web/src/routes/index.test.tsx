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

test('renders the connected state from a healthy response', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ status: 'ok', database: { major: 18, reachable: true } }),
    }),
  )
  renderIndex()
  expect(await screen.findByText(strings.health.ok(18))).toBeDefined()
})

test('renders an actionable message when the server is unreachable', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 503 }))
  renderIndex()
  expect(await screen.findByText(strings.health.failed)).toBeDefined()
})
