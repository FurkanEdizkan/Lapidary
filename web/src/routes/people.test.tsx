import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { afterEach, expect, test, vi } from 'vitest'
import { PeoplePage } from './people'
import { strings } from '../lib/strings'
import type { Person } from '../lib/types'

const PEOPLE: Person[] = [
  { id: '0192a8f4-7c1e-7d40-9b3a-5e2f61c0d811', username: 'rig', displayName: null, isAdmin: true },
  { id: '0192a8f4-7c1e-7d40-9b3a-5e2f61c0d812', username: 'ayse.demir', displayName: 'Ayşe Demir', isAdmin: false },
]

afterEach(() => vi.unstubAllGlobals())

function renderPage() {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200, json: async () => PEOPLE })))
  const rootRoute = createRootRoute({ component: () => <PeoplePage /> })
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory({ initialEntries: ['/'] }) })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

/** A17's audit read a row as "rig rig admin you": every value now says what it is, and none is said twice. */
test('a person row labels its username and role, and never repeats the name as the username', async () => {
  renderPage()
  const list = await screen.findByRole('list')
  const [rig, ayse] = within(list).getAllByRole('listitem')
  expect(within(rig!).getAllByText('rig')).toHaveLength(1)
  within(rig!).getByText(strings.people.admin)
  expect(within(rig!).queryByText(strings.people.username)).toBeNull()
  within(ayse!).getByText(strings.people.username)
  within(ayse!).getByText('ayse.demir')
  within(ayse!).getByText(strings.people.member)
})
