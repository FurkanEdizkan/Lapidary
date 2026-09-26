import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { DuplicatesPage } from './duplicates'
import { strings } from '../lib/strings'
import type { DuplicateCluster, LibraryId, PartCard, PartId } from '../lib/types'

/**
 * The review queue, which is the second place a decision about a pair can be made and the only one
 * that shows the whole library at once.
 *
 * What it must get right is what it says when it finds nothing. A page that lists no groups and
 * stops there claims the library was checked; a library where forty parts have no shape profile
 * yet has not been checked, and saying so is the difference between an answer and a silence.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId

function card(suffix: string, name: string, path: string, createdAt: string): PartCard {
  return {
    id: `01931b6e-0000-7000-8000-0000000a${suffix}` as PartId,
    library: LIBRARY,
    revision: `01931b6e-0000-7000-8000-0000000b${suffix}` as PartCard['revision'],
    name,
    partNumber: name.slice(name.lastIndexOf(' ') + 1),
    sourcePath: path,
    thumbnail: null,
    triangleCount: 48112,
    approximate: true,
    tessellationL0: null,
    sourceHash: null,
    sourceBytes: 204800,
    storedBytes: 91204,
    compressed: true,
    directory: null,
    storagePath: null,
    createdAt,
    updatedAt: createdAt,
    removedAt: null,
  }
}

const MOUNTING = card('0001', 'Bracket, LP-1042-03', 'mounting/LP-1042-03.stl', '2026-09-06T10:00:00Z')
const SPARES = card('0002', 'Bracket, LP-1042-03', 'spares/LP-1042-03.stl', '2026-09-26T09:05:00Z')
const HOUSING = card('0003', 'Housing, LP-2201-07', 'housings/LP-2201-07.step', '2026-09-08T10:00:00Z')
const HOUSING_STL = card('0004', 'Housing, LP-2201-07', 'housings/LP-2201-07.stl', '2026-09-09T10:00:00Z')

/** The same bytes twice: a scan found one file under two paths. */
const SAME: DuplicateCluster = { parts: [MOUNTING, SPARES], identical: true }
/** A STEP and an STL of one housing: alike in shape, and not the same bytes at all. */
const ALIKE: DuplicateCluster = { parts: [HOUSING, HOUSING_STL], identical: false }

function stub(clusters: DuplicateCluster[], unprofiled = 0) {
  const calls: { url: string; method: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({
        url,
        method: init?.method ?? 'GET',
        body: init?.body === undefined ? undefined : JSON.parse(String(init.body)),
      })
      if (url.includes('/duplicates')) {
        return { ok: true, status: 200, json: async () => ({ clusters, unprofiled }) }
      }
      // The header's library menu and the rest of `AppFrame`'s reads.
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  return calls
}

function renderPage(since?: string) {
  const rootRoute = createRootRoute({
    component: () => <DuplicatesPage library={LIBRARY} since={since} />,
  })
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

beforeEach(() => {
  vi.unstubAllGlobals()
})

test('the page is titled, and says what it is for', async () => {
  stub([])
  renderPage()
  await screen.findByRole('heading', { name: strings.likeness.queueTitle })
  expect(document.title).toBe(strings.titles.duplicates)
  screen.getByText(strings.likeness.queueLead)
})

test('each group names the part its decisions keep, and which kind of alike it is', async () => {
  // Handed over in the wrong order on purpose: the wire promises an order inside a group and none
  // between them, so the page is what puts the group holding the newest part first.
  stub([ALIKE, SAME])
  renderPage()
  await screen.findByRole('heading', { name: strings.likeness.clusterKept(MOUNTING.sourcePath) })
  /*
    Newest first, and the heading is the group's FIRST part. Those are two different orders and
    both matter: the wire sends the largest part of a group first, which is the one the decisions
    keep, while the groups themselves are sorted by the newest part in them — so the bracket group,
    whose second copy arrived on the 26th, comes before the housing group from the 9th, and each
    heading still names the part that group would keep.
  */
  const headings = screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent)
  expect(headings).toEqual([
    strings.likeness.clusterKept(MOUNTING.sourcePath),
    strings.likeness.clusterKept(HOUSING.sourcePath),
  ])
  screen.getByText(strings.likeness.sameBytes)
  screen.getByText(strings.likeness.alikeShape)
  screen.getByText(strings.likeness.queueCount(2))
})

test('every list on the page carries a role, groups and parts alike', async () => {
  stub([SAME])
  renderPage()
  await screen.findByRole('heading', { name: strings.likeness.clusterKept(MOUNTING.sourcePath) })
  // One list of groups, one list of parts inside each; `AppFrame`'s nav adds none.
  const lists = screen.getAllByRole('list')
  expect(lists.length).toBeGreaterThanOrEqual(2)
  for (const ul of Array.from(document.querySelectorAll('section ul'))) {
    expect(ul.getAttribute('role')).toBe('list')
  }
})

/**
 * The group's first part is what the wire sends first — the largest — and what every decision in
 * the group acts "into". So it carries no buttons of its own, and every other part carries all
 * three.
 */
test('the part a group keeps has no decisions; the rest have all three', async () => {
  stub([SAME])
  renderPage()
  await screen.findByRole('heading', { name: strings.likeness.clusterKept(MOUNTING.sourcePath) })
  const kept = screen.getByText(MOUNTING.sourcePath).closest('li') as HTMLElement
  expect(within(kept).queryByRole('button')).toBeNull()
  const other = screen.getByText(SPARES.sourcePath).closest('li') as HTMLElement
  expect(within(other).getAllByRole('button')).toHaveLength(3)
  within(other).getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) })
})

test('a decision made here folds the part the group does not keep', async () => {
  const calls = stub([SAME])
  renderPage()
  await screen.findByRole('heading', { name: strings.likeness.clusterKept(MOUNTING.sourcePath) })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) }))
  const dialog = await screen.findByRole('dialog')
  within(dialog).getByText(strings.likeness.foldConfirm(SPARES.sourcePath, MOUNTING.sourcePath))
  fireEvent.click(within(dialog).getByRole('button', { name: strings.likeness.foldConfirmAction }))
  await waitFor(() =>
    expect(calls).toContainEqual({
      url: `/api/parts/${SPARES.id}/fold`,
      method: 'POST',
      body: { into: MOUNTING.id },
    }),
  )
})

test('an empty queue says nothing looks alike, not merely nothing', async () => {
  stub([])
  renderPage()
  await screen.findByText(strings.likeness.queueEmpty)
  expect(screen.queryByRole('heading', { level: 3 })).toBeNull()
})

/**
 * The page never implies it checked a library it only partly checked. Said whether or not there
 * are groups, because a library with no groups and forty unprofiled parts is the case where the
 * omission would be most misleading.
 */
test('parts with no profile yet are counted, empty queue or not', async () => {
  stub([], 40)
  renderPage()
  await screen.findByText(strings.likeness.queueEmpty)
  screen.getByText(strings.likeness.queueUnprofiled(40))
  stub([SAME], 1)
  renderPage()
  await screen.findByText(strings.likeness.queueUnprofiled(1))
})

test('a batch narrows the page to what that batch added', async () => {
  const calls = stub([SAME])
  renderPage('2026-09-26T09:00:00Z')
  await screen.findByRole('heading', { name: strings.likeness.clusterKept(MOUNTING.sourcePath) })
  expect(
    calls.some((call) => call.url.includes('since=2026-09-26T09%3A00%3A00Z')),
  ).toBe(true)
})

test('a queue that cannot be read says so', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) =>
      url.includes('/duplicates')
        ? { ok: false, status: 503, json: async () => ({}) }
        : { ok: true, status: 200, json: async () => [] },
    ),
  )
  renderPage()
  await screen.findByText(strings.likeness.failed)
})
