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
 * How the grid draws itself.
 *
 * `gallery` is the wall of renders this application has always shown. `list` is one row per
 * part with the figures in columns — the layout that answers "which of these is tallest",
 * which no arrangement of square thumbnails can.
 *
 * The design's third option, "detail", is not here: it is the gallery with the caption
 * pinned open, which `namesAlways` already controls. See `strings.layout`.
 */
export const LAYOUTS = ['gallery', 'list'] as const
export type Layout = (typeof LAYOUTS)[number]

/**
 * The four card widths, smallest first — the design's 0–3 slider, as named values.
 *
 * Named and not numeric, for the reason `PAGE_SIZES` is a list of the four real sizes: the
 * index is meaningless in storage, so an entry written by one release and read by another
 * that reordered the scale would silently resize somebody's grid. The slider converts
 * between the two in the one component that draws it.
 */
export const CARD_SIZES = ['small', 'medium', 'large', 'huge'] as const
export type CardSize = (typeof CARD_SIZES)[number]

export const DEFAULT_PAGE_SIZE: PageSize = 50
export const DEFAULT_LAYOUT: Layout = 'gallery'
export const DEFAULT_CARD_SIZE: CardSize = 'medium'
/**
 * Whether a card's caption is painted at rest.
 *
 * `true`, which is the design file's own `namesAlways` prop default — and the first build
 * of this shipped `false`, on the argument that `v2` draws the render edge to edge and
 * reveals the words on hover, so a wall of parts reads as objects rather than as a table.
 *
 * **That was wrong, and looking at it is what settled it.** A screenshot of the grid with
 * the caption off is twelve identical squares: nothing on the page says which part is
 * which until a pointer is over one, and a person scanning a library is doing so precisely
 * *because* they do not yet know which one they want. The reveal is the right behaviour for
 * a grid somebody is browsing at leisure and the wrong default for the screen this
 * application exists to be. The toggle stays, because that browsing mode is real.
 *
 * The caption is never the only route to the name in either state: it is the card's
 * accessible name throughout, so a screen reader and the keyboard path are unaffected by
 * this setting.
 */
export const DEFAULT_NAMES_ALWAYS = true

/**
 * Namespaced and versioned by shape, not by release: `lapidary.grid.v1` says what these keys
 * mean, so a later change that alters the *meaning* of a value can pick `v2` and leave old
 * entries to be ignored rather than misread.
 */
const KEY = 'lapidary.grid.v1'

type Stored = {
  pageSize?: number
  layout?: string
  cardSize?: string
  namesAlways?: boolean
}

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

export function layoutFor(library: string): Layout {
  const stored = read(library).layout
  return LAYOUTS.find((layout) => layout === stored) ?? DEFAULT_LAYOUT
}

export function cardSizeFor(library: string): CardSize {
  const stored = read(library).cardSize
  return CARD_SIZES.find((size) => size === stored) ?? DEFAULT_CARD_SIZE
}

/**
 * Validated with `typeof` rather than taken as it comes: this is the browser's storage,
 * which a person can edit, and `JSON.parse` will hand back a string or an object for a key
 * typed `boolean`. Anything that is not a boolean is not a preference somebody set.
 */
export function namesAlwaysFor(library: string): boolean {
  const stored = read(library).namesAlways
  return typeof stored === 'boolean' ? stored : DEFAULT_NAMES_ALWAYS
}

export function setPageSize(library: string, pageSize: PageSize): void {
  write(library, { ...read(library), pageSize })
}

export function setLayout(library: string, layout: Layout): void {
  write(library, { ...read(library), layout })
}

export function setCardSize(library: string, cardSize: CardSize): void {
  write(library, { ...read(library), cardSize })
}

export function setNamesAlways(library: string, namesAlways: boolean): void {
  write(library, { ...read(library), namesAlways })
}
