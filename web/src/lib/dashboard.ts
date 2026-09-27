import type {
  LibraryId,
  ResolveRequest,
  ResolveResponse,
  Widget,
  WidgetRequest,
  WidgetResult,
} from './types'

/**
 * The dashboard's one request, and the layout this browser remembers.
 *
 * Its own module rather than more of `lib/api.ts`, for the reason `lib/likeness.ts` is:
 * one feature, one vocabulary, and here a rule that has to be kept in one place. **The
 * dashboard costs one request.** There is no per-widget endpoint and there is no polling —
 * `FEATURES.md` §8 calls per-widget polling a self-inflicted DoS, and with twelve widgets on a
 * page it would be one. Twelve widgets settle in one resolve; a key that ran out of time
 * offers the person a retry, and `lib/events.ts` re-resolves on the server saying something
 * changed. Neither is a timer. A test under `components/dashboard/` fails if
 * `refetchInterval` appears anywhere in this feature.
 *
 * **The layout is this browser's**, in `localStorage`, exactly as `preferences.ts` argues for
 * the grid's: there is no user table until Phase 8, so a column would make one operator's
 * arrangement everybody's and be wrong the day auth arrives. Every read and write is wrapped,
 * because `localStorage` does not merely come back empty in a private window — the accessor
 * throws.
 */

/** Namespaced and versioned by shape, as `phase-6.md` fixes it: `lapidary.dashboard`, v1. */
export const DASHBOARD_KEY = 'lapidary.dashboard'
export const DASHBOARD_VERSION = 1

/**
 * `ResolveRequest`'s own ceiling, restated here because this is the side that must not exceed
 * it: the route refuses a body of more than 32 keys **whole**, with a 422, so one widget too
 * many would blank the entire dashboard rather than itself. Adding is refused at 32 instead.
 */
export const MAX_WIDGETS = 32

/**
 * The largest `limit` any widget is offered.
 *
 * `phase-6.md` caps `recent` and `savedFilter` at 12 and G4 clamps to `1..=12`. G4 also settled
 * `facet`, which the design left open, at `1..=24` — and the form still offers 12 for it, which is
 * inside that and deliberate: twelve rows is already more than a widget four rows tall shows
 * without scrolling, and one ceiling a person can learn is better than two. It is one number if
 * anybody wants the other twelve.
 */
export const MAX_LIMIT = 12

/** A named group of widgets: one titled `<section>` with its own grid. */
export type Group = { id: string; name: string }

/**
 * One widget as the layout stores it: what it is, where it sits, and which group it is in.
 *
 * `x`/`y`/`w`/`h` are `layout.ts`'s `Tile` by structure, deliberately without importing it —
 * the geometry module is pure and generic over anything carrying those four numbers, so
 * nothing here depends on it and it depends on nothing here.
 *
 * `libraryName` is a **snapshot taken when the widget was added**, and the only reason it is
 * stored is that no `WidgetValue` except `FilteredParts` names its library: two storage
 * widgets for two libraries would otherwise be two identical panels. It goes stale on a
 * rename, which is exactly what `FilteredParts.name` exists to avoid, and the honest trade is
 * that reading `/api/libraries` on load would cost the second request this page does not have.
 */
export type StoredWidget = {
  key: string
  widget: Widget
  group: string
  x: number
  y: number
  w: number
  h: number
  libraryName: string | null
}

export type StoredLayout = { version: number; groups: Group[]; widgets: StoredWidget[] }

/**
 * `POST /api/dashboard/resolve` — every widget on the page, in one call.
 *
 * Always 200 once the body is valid, so a non-`ok` response here is the body being refused
 * (422: no keys, more than 32, a repeated key) or the api being unreachable — never one
 * widget having failed, which arrives as that key's own `failed` result.
 */
export async function resolve(widgets: readonly WidgetRequest[]): Promise<ResolveResponse> {
  const body: ResolveRequest = { widgets: [...widgets] }
  const response = await fetch('/api/dashboard/resolve', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!response.ok) {
    throw new Error(`resolve returned ${response.status}`)
  }
  const answer = (await response.json()) as ResolveResponse
  // Cast, not validated — so the one field every widget reads is checked before `.find` on it
  // takes the whole page down. Same defence `fetchFolds` applies for the same reason.
  if (answer === null || typeof answer !== 'object' || !Array.isArray(answer.results)) {
    throw new Error('resolve answered something without a results list')
  }
  return answer
}

/** Each key's result, for a widget to look its own up by. */
export function byKey(response: ResolveResponse | undefined): Map<string, WidgetResult> {
  const results = new Map<string, WidgetResult>()
  for (const entry of response?.results ?? []) {
    if (entry !== null && typeof entry === 'object' && typeof entry.key === 'string') {
      results.set(entry.key, entry.result)
    }
  }
  return results
}

/**
 * Which library a widget is about, or `null` for the one that is about the whole installation.
 *
 * Exhaustive over the Rust union, so a kind added there fails `tsc` here as well as in the
 * registry. `lib/events.ts` reads this to re-resolve only the keys of the library that
 * changed.
 */
export function libraryOf(widget: Widget): LibraryId | null {
  switch (widget.kind) {
    case 'instanceStorage':
      return null
    case 'storage':
    case 'recent':
    case 'savedFilter':
    case 'facet':
    case 'queue':
    case 'duplicates':
      return widget.library
  }
}

/** The request for one stored widget. */
export function requestOf(stored: StoredWidget): WidgetRequest {
  return { key: stored.key, widget: stored.widget }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function text(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function count(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.round(value) : fallback
}

const FACETS = ['format', 'material', 'tag'] as const

/**
 * A widget's configuration, built field by field from something loose, or `null`.
 *
 * Both the reader of `localStorage` and the Add form go through it, and that is deliberate: the
 * form must not be able to build a widget the reader would throw away, and neither of them
 * should carry its own copy of "a saved filter needs a filter" or of the `limit` cap.
 *
 * `localStorage` is a trust boundary and this is the one that matters most on this page: the
 * resolve route refuses a body **whole**, so one widget carrying a `kind` the server does not
 * know, or a `limit` past the cap, would answer 422 for all twelve and leave the dashboard
 * blank with nothing to say. Anything unreadable is dropped here instead, where the rest of
 * the board still loads.
 *
 * A `switch` and not a table: it is exhaustive over `Widget['kind']`, so a kind added in Rust
 * fails to compile here until the web can store it.
 */
export function widgetFrom(raw: unknown): Widget | null {
  if (!isRecord(raw)) return null
  const library = text(raw.library)
  const kind = raw.kind
  switch (kind) {
    case 'instanceStorage':
      return { kind }
    case 'storage':
    case 'queue':
    case 'duplicates':
      return library === null ? null : { kind, library }
    case 'recent':
      return library === null ? null : { kind, library, limit: clampLimit(raw.limit) }
    case 'savedFilter': {
      const filter = text(raw.filter)
      if (library === null || filter === null) return null
      return { kind, library, filter, limit: clampLimit(raw.limit) }
    }
    case 'facet': {
      const facet = FACETS.find((known) => known === raw.facet)
      if (library === null || facet === undefined) return null
      return { kind, library, facet, limit: clampLimit(raw.limit) }
    }
    default:
      return null
  }
}

/** 1 to {@link MAX_LIMIT}. A widget asking for more is corrected, not dropped. */
export function clampLimit(value: unknown): number {
  return Math.min(MAX_LIMIT, Math.max(1, count(value, MAX_LIMIT)))
}

/** The group every new dashboard starts with, and the one orphaned widgets fall back to. */
function firstGroup(name: string): Group {
  return { id: 'g1', name }
}

/**
 * The stored layout, or an empty one.
 *
 * `defaultGroupName` comes from the caller rather than from `strings.ts`, so this module can
 * be read by a test without the string table and a locale change cannot rename a group
 * somebody is looking at — the name is stored the moment a dashboard is first written.
 */
export function readLayout(defaultGroupName: string): StoredLayout {
  let raw: unknown
  try {
    const stored = window.localStorage.getItem(DASHBOARD_KEY)
    if (stored === null) return { version: DASHBOARD_VERSION, groups: [firstGroup(defaultGroupName)], widgets: [] }
    raw = JSON.parse(stored)
  } catch {
    // Blocked, unavailable, or not JSON. An empty dashboard is the correct answer to all
    // three, and none of them is worth a message.
    return { version: DASHBOARD_VERSION, groups: [firstGroup(defaultGroupName)], widgets: [] }
  }
  return sanitise(raw, defaultGroupName)
}

/**
 * What survives a read: a version this shape understands, one group at least, unique keys,
 * every widget readable, and no more than the route will accept.
 *
 * A wrong `version` is not repaired. `preferences.ts` picked the same rule and said why: a
 * version names what the values *mean*, so a later shape can take `v2` and leave these
 * entries to be ignored rather than misread.
 */
export function sanitise(raw: unknown, defaultGroupName: string): StoredLayout {
  const empty: StoredLayout = { version: DASHBOARD_VERSION, groups: [firstGroup(defaultGroupName)], widgets: [] }
  if (!isRecord(raw) || raw.version !== DASHBOARD_VERSION) return empty

  const groups: Group[] = []
  for (const entry of Array.isArray(raw.groups) ? raw.groups : []) {
    if (!isRecord(entry)) continue
    const id = text(entry.id)
    const name = text(entry.name)
    if (id === null || name === null || groups.some((group) => group.id === id)) continue
    groups.push({ id, name })
  }
  if (groups.length === 0) groups.push(firstGroup(defaultGroupName))
  const home = groups[0]?.id ?? 'g1'

  const widgets: StoredWidget[] = []
  for (const entry of Array.isArray(raw.widgets) ? raw.widgets : []) {
    if (!isRecord(entry) || widgets.length >= MAX_WIDGETS) continue
    const key = text(entry.key)
    const widget = widgetFrom(entry.widget)
    if (key === null || widget === null || widgets.some((stored) => stored.key === key)) continue
    widgets.push({
      key,
      widget,
      // A widget whose group was dropped goes to the first one rather than disappearing:
      // this is somebody's arrangement, and losing a panel silently is the worse failure.
      group: groups.some((group) => group.id === entry.group) ? String(entry.group) : home,
      x: count(entry.x, 0),
      y: count(entry.y, 0),
      w: count(entry.w, 4),
      h: count(entry.h, 2),
      libraryName: text(entry.libraryName),
    })
  }
  return { version: DASHBOARD_VERSION, groups, widgets }
}

export function writeLayout(layout: StoredLayout): void {
  try {
    window.localStorage.setItem(DASHBOARD_KEY, JSON.stringify(layout))
  } catch {
    // Quota, a private window, or site data turned off. The arrangement still applies for
    // this session — it simply will not be there next time, which is the whole of what is lost.
  }
}

/**
 * The next free widget key, and the next free group id.
 *
 * Counted rather than `crypto.randomUUID()`, which exists **only in a secure context**: an
 * air-gapped installation reached over plain `http://10.0.0.7:8080` — which is most of them —
 * has no `randomUUID`, and a dashboard that throws on Add on exactly the deployments this
 * product is for would be a poor trade for an identifier nobody sees.
 */
export function nextKey(taken: readonly string[], prefix: string): string {
  const highest = taken.reduce((top, key) => {
    const match = new RegExp(`^${prefix}(\\d+)$`).exec(key)
    const number = match === null ? 0 : Number.parseInt(match[1] ?? '0', 10)
    return Math.max(top, number)
  }, 0)
  return `${prefix}${highest + 1}`
}
