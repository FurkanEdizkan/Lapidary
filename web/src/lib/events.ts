import { useEffect, useRef } from 'react'
import type { AppEvent, LibraryId } from './types'

/**
 * `GET /api/events`, once per tab.
 *
 * **One `EventSource`, reference-counted, shared by every component that asks.** `phase-6.md`
 * fixes it and the batch stream's comment already rejected the alternative: the api holds one
 * `PgListener` per process, and a stream per interested component would be four connections
 * from one page against a server that promised one. So this module owns the connection, hands
 * out subscriptions, and closes it when the last subscriber leaves.
 *
 * **Nothing here polls.** The stream says only which library changed; the client asks again,
 * in one request, and `FEATURES.md` §8 is why there is no timer anywhere near this file.
 *
 * Changes are **coalesced to at most one delivery a second, on the trailing edge**. An upload
 * of two hundred files notifies per row; Postgres collapses what it can inside a transaction
 * and the api's hub groups over 250 ms, and this is the third and last gate — but it has to
 * be trailing, because a leading-only throttle delivers the first notification of a burst and
 * drops the last one, which is the one that says the batch finished.
 */

/** At most one delivery a second, however many notifications arrive inside it. */
export const COALESCE_MS = 1000

type Subscriber = {
  onChanged: (libraries: ReadonlySet<LibraryId>) => void
  onResync: () => void
}

const subscribers = new Set<Subscriber>()
let source: EventSource | null = null
/**
 * How many times this connection has opened. The **first** open is the connection itself and
 * is not a resync: treating it as one would cost every page load a second resolve, and under
 * React's StrictMode remount in development, two.
 */
let opens = 0
let dirty = new Set<LibraryId>()
let timer: ReturnType<typeof setTimeout> | null = null
let lastFlush = 0

function flush(): void {
  timer = null
  lastFlush = Date.now()
  if (dirty.size === 0) return
  const libraries = dirty
  dirty = new Set()
  for (const subscriber of [...subscribers]) subscriber.onChanged(libraries)
}

function schedule(): void {
  if (timer !== null) return
  timer = setTimeout(flush, Math.max(0, COALESCE_MS - (Date.now() - lastFlush)))
}

function resync(): void {
  // Everything is about to be asked for again, so a pending set of libraries is redundant.
  if (timer !== null) {
    clearTimeout(timer)
    timer = null
  }
  dirty = new Set()
  lastFlush = Date.now()
  for (const subscriber of [...subscribers]) subscriber.onResync()
}

function receive(data: string): void {
  let event: AppEvent
  try {
    event = JSON.parse(data) as AppEvent
  } catch {
    // A `data:` line that is not our JSON. Nothing to do with it, and throwing inside a
    // listener would take down a connection that is otherwise working.
    return
  }
  if (event === null || typeof event !== 'object') return
  if (event.type === 'resync') {
    resync()
    return
  }
  if (event.type === 'changed' && typeof event.library === 'string') {
    dirty.add(event.library)
    schedule()
  }
}

function open(): void {
  if (source !== null) return
  // The one thing a runtime can lack. A dashboard that stays current is better than one that
  // does not, and a dashboard that throws on mount is worse than both.
  if (typeof EventSource !== 'function') return
  const stream = new EventSource('/api/events')
  source = stream
  stream.addEventListener('open', () => {
    opens += 1
    // A reopen is a reconnect: the stream carries no history, so anything may have changed
    // while it was down and the only honest answer is to ask for everything again.
    if (opens > 1) resync()
  })
  stream.addEventListener('message', (event: MessageEvent) => {
    receive(String(event.data))
  })
  // No `error` handler that resyncs: the browser reconnects by itself and fires `open` again,
  // which is where a reconnect is handled. Resyncing on every error would re-resolve the whole
  // dashboard once a second while the api is down.
}

function close(): void {
  source?.close()
  source = null
  opens = 0
  dirty = new Set()
  if (timer !== null) {
    clearTimeout(timer)
    timer = null
  }
  lastFlush = 0
}

/**
 * Subscribe this component to the tab's one event stream.
 *
 * Both callbacks are read through a ref, so a caller passing inline arrows — which every
 * caller does — does not tear the subscription down and reopen the connection on each render.
 */
export function useAppEvents(
  onChanged: (libraries: ReadonlySet<LibraryId>) => void,
  onResync: () => void,
): void {
  const changed = useRef(onChanged)
  const resynced = useRef(onResync)
  useEffect(() => {
    changed.current = onChanged
    resynced.current = onResync
  })
  useEffect(() => {
    const subscriber: Subscriber = {
      onChanged: (libraries) => changed.current(libraries),
      onResync: () => resynced.current(),
    }
    subscribers.add(subscriber)
    open()
    return () => {
      subscribers.delete(subscriber)
      if (subscribers.size === 0) close()
    }
  }, [])
}

/** Whether the tab currently holds a stream. For the tests that count connections. */
export function streamOpen(): boolean {
  return source !== null
}
