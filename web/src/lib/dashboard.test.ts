import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import {
  DASHBOARD_KEY,
  DASHBOARD_VERSION,
  MAX_LIMIT,
  MAX_WIDGETS,
  byKey,
  clampLimit,
  libraryOf,
  nextKey,
  readLayout,
  requestOf,
  resolve,
  sanitise,
  widgetFrom,
  writeLayout,
  type StoredLayout,
} from './dashboard'
import type { LibraryId, Widget } from './types'

/**
 * The dashboard's one request, and the arrangement this browser remembers.
 *
 * Two things are load-bearing here and neither is visible on screen. The resolve route refuses a
 * body **whole**, with a 422 — no keys, more than 32, a repeated key, a widget it cannot parse —
 * so a single bad entry in `localStorage` would blank all twelve panels rather than its own.
 * That makes reading storage a trust boundary, and everything below the line is about what
 * survives it. The other is that there is exactly one request: the module has no timer and no
 * per-widget call, which `components/dashboard/no-polling.test.ts` holds it to.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId
const OTHER = '01931b6e-0000-7000-8000-000000000002' as LibraryId

const GROUP = 'Overview'

function stub(answer: unknown, ok = true, status = 200) {
  const calls: { url: string; method: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({
        url,
        method: init?.method ?? 'GET',
        body: init?.body === undefined ? undefined : JSON.parse(String(init.body)),
      })
      return { ok, status, json: async () => answer }
    }),
  )
  return calls
}

beforeEach(() => {
  window.localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

test('every widget on the page is asked for in one POST', async () => {
  const calls = stub({ results: [{ key: 'w1', result: { status: 'timedOut' } }] })
  const widgets: Widget[] = [
    { kind: 'storage', library: LIBRARY },
    { kind: 'instanceStorage' },
    { kind: 'queue', library: OTHER },
  ]
  const answer = await resolve(widgets.map((widget, index) => ({ key: `w${index + 1}`, widget })))
  expect(calls).toHaveLength(1)
  expect(calls[0]).toMatchObject({
    url: '/api/dashboard/resolve',
    method: 'POST',
    body: {
      widgets: [
        { key: 'w1', widget: { kind: 'storage', library: LIBRARY } },
        { key: 'w2', widget: { kind: 'instanceStorage' } },
        { key: 'w3', widget: { kind: 'queue', library: OTHER } },
      ],
    },
  })
  expect(answer.results).toHaveLength(1)
})

test('a refused body is an error, not a page of empty widgets', async () => {
  stub({}, false, 422)
  await expect(resolve([{ key: 'w1', widget: { kind: 'instanceStorage' } }])).rejects.toThrow('422')
})

/**
 * The body is cast, not validated. A 200 carrying something else would reach `.results.map` and
 * take the whole page down, so it is refused here, where the query turns it into a failed load
 * with a message.
 */
test('a 200 without a results list is refused', async () => {
  stub({ widgets: [] })
  await expect(resolve([{ key: 'w1', widget: { kind: 'instanceStorage' } }])).rejects.toThrow(/results/)
})

test('results are looked up by key, and a malformed entry is ignored', () => {
  const results = byKey({
    results: [
      { key: 'w1', result: { status: 'timedOut' } },
      { key: 'w2', result: { status: 'failed', message: 'no such library' } },
      // Not a shape the wire promises; ignored rather than stored under `undefined`.
      null as never,
    ],
  })
  expect(results.get('w1')).toEqual({ status: 'timedOut' })
  expect(results.get('w2')).toMatchObject({ status: 'failed' })
  expect(results.size).toBe(2)
  expect(byKey(undefined).size).toBe(0)
})

/** What `lib/events.ts`'s changed-library rule reads. `null` is the whole installation. */
test('every kind says which library it is about', () => {
  expect(libraryOf({ kind: 'storage', library: LIBRARY })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'recent', library: LIBRARY, limit: 6 })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'savedFilter', library: LIBRARY, filter: 'f', limit: 6 })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'facet', library: LIBRARY, facet: 'tag', limit: 6 })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'queue', library: LIBRARY })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'duplicates', library: LIBRARY })).toBe(LIBRARY)
  expect(libraryOf({ kind: 'instanceStorage' })).toBeNull()
})

test('a limit is kept inside the cap the route enforces', () => {
  expect(clampLimit(6)).toBe(6)
  expect(clampLimit(40)).toBe(MAX_LIMIT)
  expect(clampLimit(0)).toBe(1)
  expect(clampLimit(-3)).toBe(1)
  expect(clampLimit('eight')).toBe(MAX_LIMIT)
  expect(clampLimit(undefined)).toBe(MAX_LIMIT)
})

test('a widget is built from loose fields, or refused', () => {
  expect(widgetFrom({ kind: 'instanceStorage' })).toEqual({ kind: 'instanceStorage' })
  expect(widgetFrom({ kind: 'storage', library: LIBRARY })).toEqual({ kind: 'storage', library: LIBRARY })
  expect(widgetFrom({ kind: 'recent', library: LIBRARY, limit: 99 })).toEqual({
    kind: 'recent',
    library: LIBRARY,
    limit: MAX_LIMIT,
  })
  expect(widgetFrom({ kind: 'facet', library: LIBRARY, facet: 'material', limit: 3 })).toEqual({
    kind: 'facet',
    library: LIBRARY,
    facet: 'material',
    limit: 3,
  })
  // Each of these would be a 422 for the whole page if it reached the route.
  expect(widgetFrom({ kind: 'storage' })).toBeNull()
  expect(widgetFrom({ kind: 'savedFilter', library: LIBRARY, limit: 4 })).toBeNull()
  expect(widgetFrom({ kind: 'facet', library: LIBRARY, facet: 'colour', limit: 4 })).toBeNull()
  expect(widgetFrom({ kind: 'sparkline', library: LIBRARY })).toBeNull()
  expect(widgetFrom(null)).toBeNull()
  expect(widgetFrom('storage')).toBeNull()
})

test('a fresh browser gets one group and no widgets, and asks for nothing', () => {
  const layout = readLayout(GROUP)
  expect(layout).toMatchObject({ version: DASHBOARD_VERSION, widgets: [] })
  expect(layout.groups).toHaveLength(1)
  expect(layout.groups[0]?.name).toBe(GROUP)
})

test('what was stored comes back, group and place and all', () => {
  const stored: StoredLayout = {
    version: DASHBOARD_VERSION,
    groups: [{ id: 'g1', name: GROUP }],
    widgets: [
      {
        key: 'w1',
        widget: { kind: 'duplicates', library: LIBRARY },
        group: 'g1',
        x: 4,
        y: 2,
        w: 4,
        h: 2,
        libraryName: 'Fixtures',
      },
    ],
  }
  writeLayout(stored)
  expect(window.localStorage.getItem(DASHBOARD_KEY)).not.toBeNull()
  expect(readLayout(GROUP)).toEqual(stored)
})

/**
 * A version names what the values *mean*, so a shape this one does not understand is left alone
 * rather than repaired — the same rule `preferences.ts` picked, and for the same reason.
 */
test('a layout from another version is ignored, not guessed at', () => {
  window.localStorage.setItem(DASHBOARD_KEY, JSON.stringify({ version: 2, groups: [], widgets: [{ key: 'w1' }] }))
  expect(readLayout(GROUP).widgets).toEqual([])
})

test('storage holding something that is not ours leaves an empty dashboard', () => {
  window.localStorage.setItem(DASHBOARD_KEY, 'not json at all')
  expect(readLayout(GROUP).widgets).toEqual([])
  window.localStorage.setItem(DASHBOARD_KEY, JSON.stringify([1, 2, 3]))
  expect(readLayout(GROUP).widgets).toEqual([])
})

/**
 * The accessor itself throws in a private window or with site data blocked — it does not merely
 * come back empty. A dashboard that failed to render because somebody turned cookies off would be
 * a poor trade for remembering where a panel was.
 */
test('a browser that refuses storage still opens the page', () => {
  vi.spyOn(window.localStorage, 'getItem').mockImplementation(() => {
    throw new Error('blocked')
  })
  vi.spyOn(window.localStorage, 'setItem').mockImplementation(() => {
    throw new Error('blocked')
  })
  expect(readLayout(GROUP).widgets).toEqual([])
  expect(() => writeLayout({ version: DASHBOARD_VERSION, groups: [], widgets: [] })).not.toThrow()
})

/**
 * Every one of these entries would make the resolve route answer 422 for the whole body, so each
 * is dropped on the way in and the rest of the board still loads. This is the test that matters
 * most in the file.
 */
test('an entry the route would refuse is dropped, and the others survive', () => {
  const layout = sanitise(
    {
      version: DASHBOARD_VERSION,
      groups: [
        { id: 'g1', name: GROUP },
        { id: 'g2', name: 'Storage' },
        // A group with no id, and a second g1: neither can be told apart from something real.
        { name: 'Nameless' },
        { id: 'g1', name: 'Overview again' },
      ],
      widgets: [
        { key: 'w1', widget: { kind: 'storage', library: LIBRARY }, group: 'g2', x: 0, y: 0, w: 4, h: 2 },
        // A kind the server does not know.
        { key: 'w2', widget: { kind: 'sparkline', library: LIBRARY }, group: 'g1' },
        // A saved filter with no filter.
        { key: 'w3', widget: { kind: 'savedFilter', library: LIBRARY, limit: 4 }, group: 'g1' },
        // The same key twice: the route refuses a repeated key.
        { key: 'w1', widget: { kind: 'queue', library: LIBRARY }, group: 'g1' },
        // A limit past the cap, corrected rather than dropped.
        { key: 'w4', widget: { kind: 'recent', library: LIBRARY, limit: 500 }, group: 'g1' },
        // A group that is not there any more: the widget moves home rather than disappearing.
        { key: 'w5', widget: { kind: 'queue', library: LIBRARY }, group: 'g9' },
        // Places that are not numbers.
        { key: 'w6', widget: { kind: 'instanceStorage' }, group: 'g1', x: 'left', y: null, w: {}, h: [] },
      ],
    },
    GROUP,
  )
  expect(layout.groups.map((group) => group.id)).toEqual(['g1', 'g2'])
  expect(layout.widgets.map((widget) => widget.key)).toEqual(['w1', 'w4', 'w5', 'w6'])
  expect(layout.widgets.find((widget) => widget.key === 'w4')?.widget).toMatchObject({ limit: MAX_LIMIT })
  expect(layout.widgets.find((widget) => widget.key === 'w5')?.group).toBe('g1')
  expect(layout.widgets.find((widget) => widget.key === 'w6')).toMatchObject({ x: 0, y: 0, w: 4, h: 2 })
})

test('a layout with no readable group still has one', () => {
  const layout = sanitise({ version: DASHBOARD_VERSION, groups: 'Overview', widgets: [] }, GROUP)
  expect(layout.groups).toEqual([{ id: 'g1', name: GROUP }])
})

/** 33 keys is a 422 for the whole body, so the 33rd never leaves this function. */
test('more widgets than the route accepts are cut to what it accepts', () => {
  const widgets = Array.from({ length: MAX_WIDGETS + 8 }, (_, index) => ({
    key: `w${index}`,
    widget: { kind: 'queue', library: LIBRARY },
    group: 'g1',
    x: 0,
    y: index,
    w: 4,
    h: 2,
  }))
  const layout = sanitise({ version: DASHBOARD_VERSION, groups: [{ id: 'g1', name: GROUP }], widgets }, GROUP)
  expect(layout.widgets).toHaveLength(MAX_WIDGETS)
})

/**
 * Counted, not `crypto.randomUUID()`, which exists only in a secure context: an air-gapped
 * installation reached over plain `http://10.0.0.7:8080` has no `randomUUID`, and Add would throw
 * on exactly the deployments this product is for.
 */
test('the next key follows the highest one taken, and never needs a secure context', () => {
  expect(nextKey([], 'w')).toBe('w1')
  expect(nextKey(['w1', 'w2'], 'w')).toBe('w3')
  // A gap is not reused: the highest wins, so a key cannot be handed out twice.
  expect(nextKey(['w1', 'w7'], 'w')).toBe('w8')
  expect(nextKey(['w1', 'something-else'], 'w')).toBe('w2')
  expect(nextKey(['g1', 'g2'], 'g')).toBe('g3')
})

test('a stored widget becomes exactly the request for it', () => {
  expect(
    requestOf({
      key: 'w1',
      widget: { kind: 'queue', library: LIBRARY },
      group: 'g1',
      x: 0,
      y: 0,
      w: 4,
      h: 2,
      libraryName: null,
    }),
  ).toEqual({ key: 'w1', widget: { kind: 'queue', library: LIBRARY } })
})
