import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { DashboardPage } from './dashboard'
import { DASHBOARD_KEY, DASHBOARD_VERSION, type StoredLayout, type StoredWidget } from '../lib/dashboard'
import { strings } from '../lib/strings'
import { openMenu } from '../test-menu'
import type { KeyResult, LibraryId, PartCard, PartId, Widget, WidgetResult } from '../lib/types'

/**
 * The dashboard, whole.
 *
 * **The one assertion this file exists for is the fetch count.** `phase-6.md`'s first Phase 6 exit
 * is twelve widgets settling in one round trip, and `FEATURES.md` §8 calls the alternative — a
 * query per widget, each on a timer — a self-inflicted DoS. A render test cannot see a timer that
 * has not fired, so the count is taken directly: twelve widgets on the page, one `fetch`, and it
 * goes to `/api/dashboard/resolve`. `components/dashboard/no-polling.test.ts` guards the source
 * beside it, because the two failures look nothing alike.
 *
 * The rest is what the page does when one key comes back badly, and what moving a widget costs —
 * which is nothing, because the arrangement lives in this browser and never in a request.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId
const OTHER = '01931b6e-0000-7000-8000-000000000002' as LibraryId
const NAME = 'Fixtures'
const OTHER_NAME = 'Spares'

/** An `EventSource` that opens nothing, so a test drives the stream itself. */
class FakeSource {
  static made: FakeSource[] = []
  readonly listeners = new Map<string, ((event: unknown) => void)[]>()
  constructor(readonly url: string) {
    FakeSource.made.push(this)
  }
  addEventListener(type: string, listener: (event: unknown) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener])
  }
  close(): void {}
  send(data: string): void {
    for (const listener of this.listeners.get('message') ?? []) listener({ data })
  }
}

function widget(key: string, config: Widget, at: { x: number; y: number; w: number; h: number }, group = 'g1'): StoredWidget {
  return {
    key,
    widget: config,
    group,
    // The name stored beside the widget, as the Add form would have taken it. Two libraries get
    // two names on purpose: that is the whole reason it is stored, since no widget's value names
    // its library and two storage panels would otherwise be the same panel twice.
    libraryName:
      config.kind === 'instanceStorage' ? null : config.library === LIBRARY ? NAME : OTHER_NAME,
    ...at,
  }
}

function card(suffix: string, name: string, path: string): PartCard {
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
    createdAt: '2026-09-26T09:05:00Z',
    updatedAt: '2026-09-26T09:05:00Z',
    removedAt: null,
  }
}

const BRACKET = card('0001', 'Bracket, LP-1042-03', 'mounting/LP-1042-03.stl')
const HOUSING = card('0002', 'Housing, LP-2201-07', 'housings/LP-2201-07.step')

/** Twelve widgets, as the phase exit measures them: every kind, and five kinds twice. */
const TWELVE: StoredWidget[] = [
  widget('w1', { kind: 'storage', library: LIBRARY }, { x: 0, y: 0, w: 4, h: 2 }),
  widget('w2', { kind: 'storage', library: OTHER }, { x: 4, y: 0, w: 4, h: 2 }),
  widget('w3', { kind: 'instanceStorage' }, { x: 0, y: 2, w: 8, h: 2 }),
  widget('w4', { kind: 'recent', library: LIBRARY, limit: 6 }, { x: 0, y: 4, w: 4, h: 4 }),
  widget('w5', { kind: 'recent', library: OTHER, limit: 6 }, { x: 4, y: 4, w: 4, h: 4 }),
  widget('w6', { kind: 'savedFilter', library: LIBRARY, filter: 'f1', limit: 6 }, { x: 8, y: 4, w: 4, h: 4 }),
  widget('w7', { kind: 'facet', library: LIBRARY, facet: 'format', limit: 6 }, { x: 0, y: 8, w: 3, h: 4 }),
  widget('w8', { kind: 'facet', library: LIBRARY, facet: 'material', limit: 6 }, { x: 3, y: 8, w: 3, h: 4 }),
  widget('w9', { kind: 'facet', library: LIBRARY, facet: 'tag', limit: 6 }, { x: 6, y: 8, w: 3, h: 4 }),
  widget('w10', { kind: 'queue', library: LIBRARY }, { x: 9, y: 8, w: 3, h: 2 }),
  widget('w11', { kind: 'queue', library: OTHER }, { x: 0, y: 12, w: 4, h: 2 }),
  widget('w12', { kind: 'duplicates', library: LIBRARY }, { x: 4, y: 12, w: 4, h: 2 }),
]

/** The `ok` value each kind answers with, so every one of the twelve has something to draw. */
function valueFor(config: Widget): WidgetResult {
  switch (config.kind) {
    case 'storage':
      return {
        status: 'ok',
        value: {
          kind: 'storage',
          value: { sourceBytes: 4_194_304, derivativeBytes: 314_572, derivativeRatio: 0.075, removedBytes: 0 },
        },
      }
    case 'instanceStorage':
      return {
        status: 'ok',
        value: {
          kind: 'instanceStorage',
          value: {
            sourceBytes: 8_388_608,
            derivativeBytes: 629_145,
            inlinePreviewBytes: 5_745_760,
            removedBytes: 0,
            quarantinedBytes: 0,
            renderCacheBytes: 0,
            onDiskBytes: null,
            hostStorageRoot: null,
          },
        },
      }
    case 'recent':
      return { status: 'ok', value: { kind: 'recent', value: [BRACKET, HOUSING] } }
    case 'savedFilter':
      return { status: 'ok', value: { kind: 'savedFilter', value: { name: 'Brackets over 40 mm', parts: [BRACKET] } } }
    case 'facet':
      return {
        status: 'ok',
        value: { kind: 'facet', value: [{ value: 'stl', count: 112 }, { value: 'step', count: 44 }] },
      }
    case 'queue':
      return { status: 'ok', value: { kind: 'queue', value: { pending: 3, running: 1, failed: 0 } } }
    case 'duplicates':
      return { status: 'ok', value: { kind: 'duplicates', value: { clusters: 2, unprofiled: 7 } } }
  }
}

type Call = { url: string; method: string; body: { widgets?: { key: string }[] } | undefined }

/**
 * `fetch`, counted. Every request the page makes lands here, and most tests assert how many there
 * were — which is the whole point of the design.
 */
function stub(widgets: readonly StoredWidget[], override: Record<string, WidgetResult> = {}) {
  const calls: Call[] = []
  // The override is the *first* answer only, so a retry of a timed-out key can be seen to replace
  // it. A server that answered `timedOut` for ever would make the retry untestable.
  let answered = 0
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const body = init?.body === undefined ? undefined : JSON.parse(String(init.body))
      calls.push({ url, method: init?.method ?? 'GET', body })
      if (url.includes('/api/dashboard/resolve')) {
        const asked: { key: string }[] = body?.widgets ?? []
        const first = answered === 0
        answered += 1
        const results: KeyResult[] = asked.map((request) => {
          const stored = widgets.find((entry) => entry.key === request.key)
          const fallback: WidgetResult = stored === undefined ? { status: 'timedOut' } : valueFor(stored.widget)
          return { key: request.key, result: (first ? override[request.key] : undefined) ?? fallback }
        })
        return { ok: true, status: 200, json: async () => ({ results }) }
      }
      if (url.includes('/api/libraries')) {
        return {
          ok: true,
          status: 200,
          json: async () => [
            { id: LIBRARY, name: NAME, mode: 'hobby', partCount: 156 },
            { id: OTHER, name: OTHER_NAME, mode: 'hobby', partCount: 12 },
          ],
        }
      }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  return calls
}

function seed(
  widgets: readonly StoredWidget[],
  groups: { id: string; name: string }[] = [{ id: 'g1', name: strings.dashboard.defaultGroup }],
) {
  const layout: StoredLayout = { version: DASHBOARD_VERSION, groups, widgets: [...widgets] }
  window.localStorage.setItem(DASHBOARD_KEY, JSON.stringify(layout))
}

function renderPage() {
  const rootRoute = createRootRoute({ component: () => <DashboardPage library={LIBRARY} /> })
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

function resolves(calls: readonly Call[]): Call[] {
  return calls.filter((call) => call.url.includes('/api/dashboard/resolve'))
}

/** The live region, by the name it carries so this is never an ambiguous query. */
function announced(): string {
  return screen.getByRole('status', { name: strings.dashboard.positions }).textContent ?? ''
}

function stored(): StoredLayout {
  return JSON.parse(window.localStorage.getItem(DASHBOARD_KEY) ?? '{}') as StoredLayout
}

beforeEach(() => {
  window.localStorage.clear()
  FakeSource.made = []
  vi.stubGlobal('EventSource', FakeSource)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

test('the page is titled and says what it is and whose arrangement it is', async () => {
  stub([])
  renderPage()
  await screen.findByRole('heading', { name: strings.dashboard.title, level: 2 })
  expect(document.title).toBe(strings.titles.dashboard)
  screen.getByText(strings.dashboard.lead)
  screen.getByText(strings.dashboard.perBrowser)
})

/** 0 keys is a 422, so an empty dashboard must not ask — and it has something to say instead. */
test('an empty dashboard makes no request at all', async () => {
  const calls = stub([])
  renderPage()
  await screen.findByText(strings.dashboard.empty)
  screen.getByText(strings.dashboard.emptyLead)
  expect(calls).toEqual([])
})

/**
 * The phase exit, as far as a test can take it: twelve widgets, **one** `fetch`, and every panel
 * drawn from that one answer. The lead measures the round trip against a stack; this measures that
 * there is one of it.
 */
test('twelve widgets settle in exactly one request', async () => {
  seed(TWELVE)
  const calls = stub(TWELVE)
  renderPage()
  // Waited on a figure from the answer, not on a heading: the twelve panels are drawn from the
  // stored layout before the resolve lands, so a heading is on the page while every body still
  // says it is loading.
  await screen.findByText(strings.dashboard.duplicatesLine(2))

  expect(calls).toHaveLength(1)
  expect(calls[0]?.url).toBe('/api/dashboard/resolve')
  expect(calls[0]?.method).toBe('POST')
  expect(calls[0]?.body?.widgets?.map((request) => request.key)).toEqual(TWELVE.map((entry) => entry.key))

  // Twelve panels, each named, and every kind's value on the page.
  expect(screen.getAllByRole('article')).toHaveLength(12)
  expect(screen.getAllByText(strings.dashboard.queueLine(3, 1, 0))).toHaveLength(2)
  screen.getByText(strings.dashboard.duplicatesLine(2))
  screen.getByText(strings.dashboard.duplicatesUnprofiled(7))
  expect(screen.getAllByRole('link', { name: BRACKET.name }).length).toBeGreaterThan(0)
  // A saved filter is titled by the name the server just sent, not by the kind's label.
  screen.getByRole('heading', { name: strings.dashboard.inLibrary('Brackets over 40 mm', NAME) })
  // Two libraries, two storage panels, told apart by the name stored beside each widget.
  screen.getByRole('heading', { name: strings.dashboard.inLibrary(strings.dashboard.storageLabel, NAME) })
  screen.getByRole('heading', { name: strings.dashboard.inLibrary(strings.dashboard.storageLabel, OTHER_NAME) })
})

test('every list on the page carries a role', async () => {
  seed(TWELVE)
  stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.duplicatesLine(2))
  for (const list of Array.from(document.querySelectorAll('article ul, article ol'))) {
    expect(list.getAttribute('role')).toBe('list')
  }
})

/**
 * The resolve always answers 200 once the body is valid, so a key that ran out of time says so in
 * its own panel **while the other eleven render**. That is the whole reason the route answers per
 * key instead of failing, and it is what a per-widget endpoint would have bought at the cost of
 * twelve requests.
 */
test('a key that ran out of time offers its own retry while the rest render', async () => {
  seed(TWELVE)
  const calls = stub(TWELVE, { w10: { status: 'timedOut' } })
  renderPage()
  await screen.findByText(strings.dashboard.timedOut)
  // The other eleven are fine.
  screen.getByText(strings.dashboard.duplicatesLine(2))
  expect(screen.getAllByRole('article')).toHaveLength(12)

  const title = strings.dashboard.inLibrary(strings.dashboard.queueLabel, NAME)
  const retry = screen.getByRole('button', { name: strings.dashboard.retryLabel(title) })
  // WCAG 2.5.3: the name a screen reader hears begins with the words on the button.
  expect(retry.textContent).toBe(strings.dashboard.retry)
  expect(retry.getAttribute('aria-label')?.startsWith(strings.dashboard.retry)).toBe(true)

  fireEvent.click(retry)
  await waitFor(() => expect(resolves(calls)).toHaveLength(2))
  // One key, not twelve: the retry asks about the widget that failed and nothing else.
  expect(resolves(calls)[1]?.body?.widgets?.map((request) => request.key)).toEqual(['w10'])
  await waitFor(() => expect(screen.queryByText(strings.dashboard.timedOut)).toBeNull())
})

test("a key that failed says so, and shows the server's reason", async () => {
  seed([TWELVE[0] as StoredWidget])
  stub(TWELVE, { w1: { status: 'failed', message: 'That library is not here any more.' } })
  renderPage()
  await screen.findByText(strings.dashboard.widgetFailed)
  screen.getByText('That library is not here any more.')
})

test('a resolve that was refused outright says so once, for the page', async () => {
  seed(TWELVE)
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: false, status: 503, json: async () => ({}) })),
  )
  renderPage()
  await screen.findByText(strings.dashboard.loadFailed)
  expect(screen.queryByRole('article')).toBeNull()
})

/**
 * Moving a widget is free. The arrangement is this browser's, so there is nothing to tell a server
 * about — and a board that re-resolved on every nudge would be the polling this design refuses,
 * arriving through the back door.
 */
test('the arrow keys move a widget, say where it went, and cost no request', async () => {
  const two = [
    widget('w1', { kind: 'queue', library: LIBRARY }, { x: 0, y: 0, w: 4, h: 2 }),
    widget('w2', { kind: 'duplicates', library: LIBRARY }, { x: 0, y: 2, w: 4, h: 2 }),
  ]
  seed(two)
  const calls = stub(two)
  renderPage()
  const title = strings.dashboard.inLibrary(strings.dashboard.queueLabel, NAME)
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  const handle = screen.getByRole('button', { name: strings.dashboard.moveLabel(title) })
  expect(announced()).toBe('')

  fireEvent.keyDown(handle, { key: 'ArrowDown' })
  // Past the widget below it: rows are computed, so "down" is a reorder and the two swap.
  expect(announced()).toBe(strings.dashboard.moved(title, strings.dashboard.defaultGroup, 1, 3))
  expect(stored().widgets.find((entry) => entry.key === 'w1')?.y).toBe(2)

  fireEvent.keyDown(handle, { key: 'ArrowRight' })
  expect(announced()).toBe(strings.dashboard.moved(title, strings.dashboard.defaultGroup, 2, 3))

  fireEvent.keyDown(handle, { key: 'ArrowUp' })
  expect(stored().widgets.find((entry) => entry.key === 'w1')?.y).toBe(0)

  // Shift resizes, inside the kind's limits, and says the new size rather than the place.
  fireEvent.keyDown(handle, { key: 'ArrowRight', shiftKey: true })
  expect(announced()).toBe(strings.dashboard.resized(title, 5, 2))
  expect(stored().widgets.find((entry) => entry.key === 'w1')?.w).toBe(5)

  // Four moves and a resize, and the page has still made one request in its life.
  expect(resolves(calls)).toHaveLength(1)
})

test('a widget can be taken off the page, and nothing else asks again', async () => {
  seed(TWELVE.slice(0, 2))
  const calls = stub(TWELVE)
  renderPage()
  const title = strings.dashboard.inLibrary(strings.dashboard.storageLabel, NAME)
  await screen.findAllByText(/on disk/)
  const menu = await openMenu(strings.dashboard.widgetMenu(title))
  fireEvent.click(within(menu).getByRole('button', { name: strings.dashboard.removeWidget }))
  await waitFor(() => expect(screen.getAllByRole('article')).toHaveLength(1))
  expect(stored().widgets.map((entry) => entry.key)).toEqual(['w2'])
  expect(resolves(calls)).toHaveLength(1)
})

/**
 * A new widget is one more key, asked for on its own. The library list is read here and only here
 * — a `useQuery` for it beside the board would make the page's one request two.
 */
test('adding a widget reads the libraries only when the form opens, and asks for that key alone', async () => {
  // Across the whole board, so "last" is a row below and not the space beside it.
  seed([{ ...(TWELVE[9] as StoredWidget), x: 0, w: 12 }])
  const calls = stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  expect(calls.filter((call) => call.url.includes('/api/libraries'))).toHaveLength(0)

  fireEvent.click(screen.getByRole('button', { name: strings.dashboard.add }))
  const dialog = await screen.findByRole('dialog')
  await waitFor(() => expect(calls.filter((call) => call.url.includes('/api/libraries'))).toHaveLength(1))
  fireEvent.click(within(dialog).getByRole('button', { name: strings.dashboard.addConfirm }))

  await waitFor(() => expect(resolves(calls)).toHaveLength(2))
  expect(resolves(calls)[1]?.body?.widgets).toHaveLength(1)
  expect(stored().widgets).toHaveLength(2)
  // Added last, under what was already there, and its library's name stored beside it. The key
  // follows the highest one taken — `w10` was seeded, so this is `w11` — which is what keeps a
  // key from being handed out twice after something in the middle was removed.
  const added = stored().widgets.find((entry) => entry.key !== 'w10')
  expect(added?.key).toBe('w11')
  expect(added?.libraryName).toBe(NAME)
  expect(added?.y).toBe(2)
  expect(resolves(calls)[1]?.body?.widgets?.[0]?.key).toBe('w11')
})

/**
 * The server says which library changed; the page asks again for that library's keys **in one
 * call**. Widgets about another library are left alone, and the one about the whole store comes
 * along, because a part added anywhere moves its figures and no notification says "the
 * installation changed".
 */
test('a changed library re-resolves its own keys and the whole-store widget, in one call', async () => {
  const mixed = [
    widget('w1', { kind: 'storage', library: LIBRARY }, { x: 0, y: 0, w: 4, h: 2 }),
    widget('w2', { kind: 'queue', library: OTHER }, { x: 4, y: 0, w: 4, h: 2 }),
    widget('w3', { kind: 'instanceStorage' }, { x: 0, y: 2, w: 8, h: 2 }),
  ]
  seed(mixed)
  const calls = stub(mixed)
  renderPage()
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  expect(FakeSource.made).toHaveLength(1)
  expect(FakeSource.made[0]?.url).toBe('/api/events')
  expect(resolves(calls)).toHaveLength(1)

  FakeSource.made[0]?.send(JSON.stringify({ type: 'changed', library: LIBRARY }))
  await waitFor(() => expect(resolves(calls)).toHaveLength(2), { timeout: 3000 })
  expect(resolves(calls)[1]?.body?.widgets?.map((request) => request.key)).toEqual(['w1', 'w3'])
})

test('a resync asks for everything, once', async () => {
  seed(TWELVE)
  const calls = stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.duplicatesLine(2))
  FakeSource.made[0]?.send(JSON.stringify({ type: 'resync' }))
  await waitFor(() => expect(resolves(calls)).toHaveLength(2), { timeout: 3000 })
  expect(resolves(calls)[1]?.body?.widgets).toHaveLength(12)
})

/** Groups are titled sections with their own grids, and a widget moves between them. */
test('a widget moves to another group, and says which group it is in now', async () => {
  const groups = [
    { id: 'g1', name: strings.dashboard.defaultGroup },
    { id: 'g2', name: 'Housekeeping' },
  ]
  seed([TWELVE[9] as StoredWidget], groups)
  stub(TWELVE)
  renderPage()
  const title = strings.dashboard.inLibrary(strings.dashboard.queueLabel, NAME)
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  screen.getByRole('heading', { name: title })
  expect(screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent)).toEqual([
    strings.dashboard.defaultGroup,
    'Housekeeping',
  ])

  const menu = await openMenu(strings.dashboard.widgetMenu(title))
  fireEvent.click(within(menu).getByRole('button', { name: strings.dashboard.moveToGroup('Housekeeping') }))
  await waitFor(() => expect(stored().widgets[0]?.group).toBe('g2'))
  expect(announced()).toBe(strings.dashboard.moved(title, 'Housekeeping', 1, 1))
})

/**
 * A group is a heading, and its widgets have to be somewhere — so removing one moves them rather
 * than taking them with it, and the dialog says so. Nothing on this page deletes anything.
 */
test('removing a group keeps its widgets, and says where they went', async () => {
  const groups = [
    { id: 'g1', name: strings.dashboard.defaultGroup },
    { id: 'g2', name: 'Housekeeping' },
  ]
  seed([{ ...(TWELVE[9] as StoredWidget), group: 'g2' }], groups)
  stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))

  const menu = await openMenu(strings.dashboard.groupMenu('Housekeeping'))
  fireEvent.click(within(menu).getByRole('button', { name: strings.dashboard.removeGroup('Housekeeping') }))
  const dialog = await screen.findByRole('dialog')
  within(dialog).getByText(strings.dashboard.removeGroupKeeps('Housekeeping', strings.dashboard.defaultGroup))
  fireEvent.click(within(dialog).getByRole('button', { name: strings.dashboard.removeGroupConfirm }))

  await waitFor(() => expect(stored().groups).toHaveLength(1))
  expect(stored().widgets).toHaveLength(1)
  expect(stored().widgets[0]?.group).toBe('g1')
  // Still on the page, in the group it moved to, with the figures it already had.
  screen.getByText(strings.dashboard.queueLine(3, 1, 0))
})

/** The last group cannot go: a widget has to be in one. */
test('the only group cannot be removed', async () => {
  seed([TWELVE[9] as StoredWidget])
  stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  const menu = await openMenu(strings.dashboard.groupMenu(strings.dashboard.defaultGroup))
  const remove = within(menu).getByRole('button', { name: strings.dashboard.removeGroup(strings.dashboard.defaultGroup) })
  expect(remove.hasAttribute('disabled')).toBe(true)
  expect(remove.getAttribute('title')).toBe(strings.dashboard.lastGroup)
})

test('a new group can be added and a widget put in it', async () => {
  seed([TWELVE[9] as StoredWidget])
  stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.queueLine(3, 1, 0))
  fireEvent.click(screen.getByRole('button', { name: strings.dashboard.addGroup }))
  const dialog = await screen.findByRole('dialog')
  fireEvent.change(within(dialog).getByLabelText(strings.dashboard.groupNameLabel), {
    target: { value: 'Housekeeping' },
  })
  fireEvent.click(within(dialog).getByRole('button', { name: strings.dashboard.addGroupConfirm }))
  await waitFor(() => expect(stored().groups).toHaveLength(2))
  expect(stored().groups[1]).toEqual({ id: 'g2', name: 'Housekeeping' })
  await screen.findByRole('heading', { name: 'Housekeeping', level: 3 })
})

/**
 * Coming back to the tab is not a reason to ask again.
 *
 * This is polling by another name, on a schedule set by how often somebody looks at the window —
 * and it is what TanStack Query does by default, with `staleTime: 0` and `refetchOnWindowFocus`.
 * The event stream is what keeps this page current, so both are turned off and this holds them
 * off: twelve widgets, one request, however many times the tab is focused.
 */
test('coming back to the tab does not re-resolve the dashboard', async () => {
  seed(TWELVE)
  const calls = stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.duplicatesLine(2))
  fireEvent.focus(window)
  document.dispatchEvent(new Event('visibilitychange'))
  await new Promise((settle) => {
    setTimeout(settle, 60)
  })
  expect(resolves(calls)).toHaveLength(1)
})

/** The route the resolve was refused by is the one the page reports; a widget never reads its own. */
test('nothing on the page asks for a per-widget endpoint', async () => {
  seed(TWELVE)
  const calls = stub(TWELVE)
  renderPage()
  await screen.findByText(strings.dashboard.duplicatesLine(2))
  expect(calls.every((call) => call.url === '/api/dashboard/resolve')).toBe(true)
})
