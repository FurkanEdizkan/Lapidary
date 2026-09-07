/**
 * Grid preferences, remembered per library.
 *
 * **Per viewer, per library — and `FEATURES.md` says so now.** The row used to read
 * "persisted per library", which sounds like a property of the library and would mean a
 * column. There is no user table, no session table and no auth in Phase 1, so a column would
 * make one operator's choice everybody's, and be wrong the moment auth arrives. This is the
 * browser's own storage keyed by library id: each library remembers its own setting, which
 * is the intent, and the memory is this browser's, which is the honest limit.
 *
 * Everything here is wrapped in `try`/`catch` because `localStorage` is not merely empty in
 * a private window or with site data blocked — the accessor itself throws. A grid that fails
 * to render because somebody has cookies turned off would be a poor trade for remembering a
 * page size.
 */

/** How many cards a page asks for. The four `FEATURES.md` names, and the route's ceiling. */
export const PAGE_SIZES = [50, 100, 250, 500] as const
export type PageSize = (typeof PAGE_SIZES)[number]

/**
 * How tightly the grid packs.
 *
 * Two, not three. `comfortable` is what the grid has always been and stays the default;
 * `compact` is for somebody scanning a large library, where fitting more on screen is the
 * whole point. A third would be a value nobody asked for.
 */
export const DENSITIES = ['comfortable', 'compact'] as const
export type Density = (typeof DENSITIES)[number]

export const DEFAULT_PAGE_SIZE: PageSize = 50
export const DEFAULT_DENSITY: Density = 'comfortable'

/**
 * Namespaced and versioned by shape, not by release: `lapidary.grid.v1` says what these keys
 * mean, so a later change that alters the *meaning* of a value can pick `v2` and leave old
 * entries to be ignored rather than misread.
 */
const KEY = 'lapidary.grid.v1'

type Stored = { pageSize?: number; density?: string }

function read(library: string): Stored {
  try {
    const raw = window.localStorage.getItem(`${KEY}.${library}`)
    if (raw === null) return {}
    const parsed: unknown = JSON.parse(raw)
    return parsed !== null && typeof parsed === 'object' ? (parsed as Stored) : {}
  } catch {
    // Blocked, unavailable, or holding something that is not ours. Defaults are correct in
    // every one of those cases, and none of them is worth a message.
    return {}
  }
}

function write(library: string, value: Stored): void {
  try {
    window.localStorage.setItem(`${KEY}.${library}`, JSON.stringify(value))
  } catch {
    // Quota, a private window, or site data turned off. The setting still applies for this
    // session — it simply will not be there next time, which is the whole of what is lost.
  }
}

/**
 * The page size for `library`, or the default.
 *
 * Validated against `PAGE_SIZES` rather than trusted: this is the browser's storage, which a
 * person can edit, and a value the route would clamp anyway should not reach the route as a
 * number nobody offered.
 */
export function pageSizeFor(library: string): PageSize {
  const stored = read(library).pageSize
  return PAGE_SIZES.find((size) => size === stored) ?? DEFAULT_PAGE_SIZE
}

export function densityFor(library: string): Density {
  const stored = read(library).density
  return DENSITIES.find((density) => density === stored) ?? DEFAULT_DENSITY
}

export function setPageSize(library: string, pageSize: PageSize): void {
  write(library, { ...read(library), pageSize })
}

export function setDensity(library: string, density: Density): void {
  write(library, { ...read(library), density })
}
