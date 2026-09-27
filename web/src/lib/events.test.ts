import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { COALESCE_MS, streamOpen, useAppEvents } from './events'
import type { LibraryId } from './types'

/**
 * The tab's one event stream.
 *
 * What it must get right is what it does **not** do. It does not open a connection per component
 * — `phase-6.md` puts one `PgListener` in the api process and the batch stream's comment already
 * rejected the alternative. It does not treat the first `open` as a resync, which would cost every
 * page load a second resolve of everything and, under React's StrictMode remount, two. And it does
 * not deliver a notification per row: an upload of two hundred files notifies two hundred times,
 * and this is the last of the three gates between that and the resolve route.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId
const OTHER = '01931b6e-0000-7000-8000-000000000002' as LibraryId

/** An `EventSource` whose opens and messages are this test's to fire. */
class FakeSource {
  static made: FakeSource[] = []
  readonly listeners = new Map<string, ((event: unknown) => void)[]>()
  closed = false

  constructor(readonly url: string) {
    FakeSource.made.push(this)
  }

  addEventListener(type: string, listener: (event: unknown) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener])
  }

  close(): void {
    this.closed = true
  }

  fire(type: string, event: unknown): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event)
  }

  send(data: string): void {
    this.fire('message', { data })
  }
}

function subscribe() {
  const changed: string[][] = []
  const resyncs: number[] = []
  const hook = renderHook(() =>
    useAppEvents(
      (libraries) => changed.push([...libraries].sort()),
      () => resyncs.push(1),
    ),
  )
  return { hook, changed, resyncs }
}

function latest(): FakeSource {
  const source = FakeSource.made.at(-1)
  if (source === undefined) throw new Error('nothing opened a stream')
  return source
}

beforeEach(() => {
  FakeSource.made = []
  vi.useFakeTimers()
  vi.stubGlobal('EventSource', FakeSource)
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

test('two components share one stream, and it closes when the last one leaves', () => {
  const first = subscribe()
  const second = subscribe()
  expect(FakeSource.made).toHaveLength(1)
  expect(latest().url).toBe('/api/events')
  expect(streamOpen()).toBe(true)

  first.hook.unmount()
  expect(latest().closed).toBe(false)
  expect(streamOpen()).toBe(true)

  second.hook.unmount()
  expect(latest().closed).toBe(true)
  expect(streamOpen()).toBe(false)
})

/**
 * The connection opening is the connection opening. Calling it a resync would mean every load of
 * the dashboard cost two resolves — the query's, and one for an event that never happened.
 */
test('the first open is not a resync', () => {
  const { hook, resyncs, changed } = subscribe()
  act(() => {
    latest().fire('open', {})
    vi.advanceTimersByTime(COALESCE_MS * 2)
  })
  expect(resyncs).toEqual([])
  expect(changed).toEqual([])
  hook.unmount()
})

/**
 * A reopen is a reconnect, and the stream carries no history — anything may have changed while it
 * was down, so the only honest answer is to ask for everything once.
 */
test('a reconnect is a resync', () => {
  const { hook, resyncs } = subscribe()
  act(() => {
    latest().fire('open', {})
    latest().fire('open', {})
  })
  expect(resyncs).toHaveLength(1)
  hook.unmount()
})

test('a burst of notifications becomes one delivery naming every library in it', () => {
  const { hook, changed } = subscribe()
  act(() => {
    const source = latest()
    source.fire('open', {})
    for (let index = 0; index < 50; index += 1) {
      source.send(JSON.stringify({ type: 'changed', library: LIBRARY }))
      source.send(JSON.stringify({ type: 'changed', library: OTHER }))
    }
  })
  // Nothing yet: the delivery is on the trailing edge, so the whole burst is still gathering.
  expect(changed).toEqual([])
  act(() => {
    vi.advanceTimersByTime(COALESCE_MS)
  })
  expect(changed).toEqual([[LIBRARY, OTHER].sort()])
  hook.unmount()
})

/**
 * Trailing, not leading, and this is the case that decides it: a leading throttle delivers the
 * first notification of an upload and drops the last one — the one that says the batch finished.
 */
test('a notification arriving inside the same second waits out the rest of it, and is not lost', () => {
  const { hook, changed } = subscribe()
  const source = latest()
  // The first change of a session is not made to wait: nothing has been delivered yet, so the
  // whole second is already behind it.
  act(() => {
    source.fire('open', {})
    source.send(JSON.stringify({ type: 'changed', library: LIBRARY }))
    vi.advanceTimersByTime(1)
  })
  expect(changed).toEqual([[LIBRARY]])

  act(() => {
    vi.advanceTimersByTime(200)
    source.send(JSON.stringify({ type: 'changed', library: OTHER }))
    // 500ms after the delivery, and still nothing: the rest of the second is being waited out.
    vi.advanceTimersByTime(300)
  })
  expect(changed).toHaveLength(1)

  act(() => {
    vi.advanceTimersByTime(600)
  })
  expect(changed).toEqual([[LIBRARY], [OTHER]])
  hook.unmount()
})

test('a resync makes a pending change redundant rather than arriving after it', () => {
  const { hook, changed, resyncs } = subscribe()
  act(() => {
    const source = latest()
    source.fire('open', {})
    source.send(JSON.stringify({ type: 'changed', library: LIBRARY }))
    source.send(JSON.stringify({ type: 'resync' }))
    vi.advanceTimersByTime(COALESCE_MS * 3)
  })
  expect(resyncs).toHaveLength(1)
  expect(changed).toEqual([])
  hook.unmount()
})

test('a data line that is not ours is ignored rather than thrown from a listener', () => {
  const { hook, changed, resyncs } = subscribe()
  act(() => {
    const source = latest()
    source.fire('open', {})
    expect(() => source.send('keep-alive')).not.toThrow()
    source.send(JSON.stringify({ type: 'somethingElse' }))
    source.send(JSON.stringify({ type: 'changed' }))
    source.send('null')
    vi.advanceTimersByTime(COALESCE_MS * 2)
  })
  expect(changed).toEqual([])
  expect(resyncs).toEqual([])
  hook.unmount()
})

/** The one thing a runtime can lack. A page that stays current is better than one that throws. */
test('a runtime without EventSource mounts and does nothing', () => {
  vi.stubGlobal('EventSource', undefined)
  const { hook, changed, resyncs } = subscribe()
  expect(streamOpen()).toBe(false)
  expect(changed).toEqual([])
  expect(resyncs).toEqual([])
  expect(() => hook.unmount()).not.toThrow()
})

/**
 * Every caller passes inline arrows, so the callbacks are new functions on every render. Read
 * through a ref rather than depended on: an effect keyed on them would close the tab's connection
 * and open a new one each time anything re-rendered.
 */
test('re-rendering does not reopen the connection', () => {
  const { hook } = subscribe()
  hook.rerender()
  hook.rerender()
  expect(FakeSource.made).toHaveLength(1)
  hook.unmount()
})
