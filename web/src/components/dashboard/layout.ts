/**
 * Where the dashboard's widgets sit, as arithmetic and nothing else.
 *
 * Pure on purpose, and the reason is `phase-6.md`'s: drag and resize are hand-rolled, so the
 * part that can be wrong in a way nobody sees — two widgets on the same cell, a column past
 * the twelfth, a gap that appears after something is removed — is the part that has to be
 * testable without a pointer, a layout engine or a browser. Nothing here imports React, the
 * registry or `strings.ts`; the size limits arrive as arguments, because a module that knew
 * the registry could not be unit-tested without mounting it.
 *
 * **`y` is derived, never chosen.** A tile carries a row, but it is an output: {@link settle}
 * computes every row from the tiles' *order*, so the board cannot hold a vertical gap and no
 * caller has to remember to compact. That is what makes the keyboard moves up and down
 * reorderings rather than row arithmetic — see {@link moveVertical}.
 */

/** Twelve, as `phase-6.md` fixes it. Every width and column below is in these units. */
export const COLUMNS = 12

/** One widget's rectangle. `key` is what the stored layout knows it by. */
export type Tile = { key: string; x: number; y: number; w: number; h: number }

export type Size = { w: number; h: number }

/** A kind's size limits, from the registry — passed in so this module does not read it. */
export type Limits = { min: Size; max: Size }

/** Whether two rectangles share a cell. Half-open on both axes: touching edges do not. */
function collides(a: Tile, b: Tile): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/** Whether two tiles share a column, whatever their rows. What "the one below" means. */
function sameColumns(a: Tile, b: Tile): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value))
}

/** A width that fits, and a column that leaves room for it. */
function fit(tile: Tile): { x: number; w: number } {
  const w = clamp(Math.round(tile.w), 1, COLUMNS)
  return { x: clamp(Math.round(tile.x), 0, COLUMNS - w), w }
}

/**
 * Every tile's row, from the order they are in.
 *
 * Each tile in turn drops to the first free row at or below the one it came in with, then
 * rises as far as it can without hitting something already placed. Push-down first and
 * pull-up second, in that order: a tile whose row is already taken has to find a free one
 * before "as high as it will go" means anything.
 *
 * The order of the array is the whole input. Two tiles in the same columns keep their
 * relative order, which is why a reorder is how they change places.
 *
 * **A row past the bottom of what is already placed is the bottom.** Not a nicety: a tile
 * cannot come to rest below the content, since it would only rise back to it, so starting
 * lower is the same answer reached one row at a time. Without the clamp, the `Number
 * .MAX_SAFE_INTEGER` that callers use to mean "last" made the pull-up loop count down nine
 * quadrillion rows, which is a hang and not a wrong answer — it is what made the first three
 * board tests time out with nothing to show. The clamp also makes "last" a legitimate thing
 * for a caller to ask for, which is why every one of them does.
 */
export function settle<T extends Tile>(tiles: readonly T[]): T[] {
  const placed: T[] = []
  for (const tile of tiles) {
    const { x, w } = fit(tile)
    const h = Math.max(1, Math.round(tile.h))
    let y = Math.min(Math.max(0, Math.round(tile.y)), rows(placed))
    const at = (row: number): Tile => ({ key: tile.key, x, y: row, w, h })
    while (placed.some((other) => collides(at(y), other))) y += 1
    while (y > 0 && !placed.some((other) => collides(at(y - 1), other))) y -= 1
    placed.push({ ...tile, x, y, w, h })
  }
  return placed
}

/**
 * The tiles in reading order: the order {@link settle} should be given after a reload, where
 * all that survived is each tile's last row and column.
 */
export function inOrder<T extends Tile>(tiles: readonly T[]): T[] {
  return [...tiles].sort((a, b) => a.y - b.y || a.x - b.x || (a.key < b.key ? -1 : 1))
}

/** How many rows the board needs. Zero for an empty one, so the grid has no stray row. */
export function rows(tiles: readonly Tile[]): number {
  return tiles.reduce((deep, tile) => Math.max(deep, tile.y + tile.h), 0)
}

/** The tile `key` is, or undefined. */
function indexOf(tiles: readonly Tile[], key: string): number {
  return tiles.findIndex((tile) => tile.key === key)
}

/**
 * Left or right by one column, then settled.
 *
 * Clamped rather than refused at the edges: an arrow key that does nothing at column 0 is
 * the honest answer, and `aria-live` reports the position it ended at either way.
 */
export function moveHorizontal<T extends Tile>(tiles: readonly T[], key: string, dx: number): T[] {
  const index = indexOf(tiles, key)
  if (index === -1) return [...tiles]
  const moved = tiles.map((tile, at) => (at === index ? { ...tile, x: tile.x + dx } : tile))
  return settle(moved)
}

/**
 * Up or down past the next widget that shares a column.
 *
 * A reorder and not a row change, because the rows are computed. Adding 1 to `y` on a
 * compacted board is undone by the next {@link settle} — the tile simply rises again — so
 * "down" has to mean *after the thing below me*, which is a swap in the order. A tile with
 * nothing above or below it in its columns cannot move that way and the array comes back
 * unchanged, which is also the truth: it is already as high as the board goes.
 */
export function moveVertical<T extends Tile>(tiles: readonly T[], key: string, dy: number): T[] {
  const index = indexOf(tiles, key)
  const tile = tiles[index]
  if (tile === undefined) return [...tiles]
  const step = dy < 0 ? -1 : 1
  for (let at = index + step; at >= 0 && at < tiles.length; at += step) {
    const other = tiles[at]
    if (other === undefined || !sameColumns(tile, other)) continue
    const reordered = [...tiles]
    reordered.splice(index, 1)
    reordered.splice(at, 0, tile)
    return settle(reordered)
  }
  return settle([...tiles])
}

/**
 * Wider, narrower, taller or shorter by one, inside the kind's limits.
 *
 * The limits are the registry's and arrive as an argument. A width is also capped by what is
 * left to the right of the tile's own column, so resizing never silently moves it.
 */
export function resizeBy<T extends Tile>(
  tiles: readonly T[],
  key: string,
  dw: number,
  dh: number,
  limits: Limits,
): T[] {
  const index = indexOf(tiles, key)
  if (index === -1) return [...tiles]
  const resized = tiles.map((tile, at) => {
    if (at !== index) return tile
    const w = clamp(tile.w + dw, limits.min.w, Math.min(limits.max.w, COLUMNS - tile.x))
    const h = clamp(tile.h + dh, limits.min.h, limits.max.h)
    return { ...tile, w, h }
  })
  return settle(resized)
}

/**
 * Drop `key` at a column and a row — what a pointer drag ends with.
 *
 * The row is a *position among the others*, not a stored value: the tile is given that row,
 * the whole board is sorted by row, and {@link settle} turns the sort back into rows. A drag
 * onto an occupied row therefore lands above what was there, and what was there moves down,
 * which is what a person doing it expects to see.
 */
export function placeAt<T extends Tile>(tiles: readonly T[], key: string, x: number, row: number): T[] {
  const index = indexOf(tiles, key)
  if (index === -1) return [...tiles]
  const moved = tiles.map((tile, at) => (at === index ? { ...tile, x, y: Math.max(0, row) } : tile))
  const ordered = [...moved].sort((a, b) => {
    if (a.y !== b.y) return a.y - b.y
    if (a.key === key) return -1
    if (b.key === key) return 1
    return a.x - b.x
  })
  return settle(ordered)
}

/** A new widget goes last and lands wherever {@link settle} puts it. */
export function append<T extends Tile>(tiles: readonly T[], tile: T): T[] {
  return settle([...tiles, { ...tile, y: rows(tiles) }])
}

export function remove<T extends Tile>(tiles: readonly T[], key: string): T[] {
  return settle(tiles.filter((tile) => tile.key !== key))
}

/**
 * Which column an offset inside the board falls in.
 *
 * Takes the board's width rather than reading it, for the reason the whole module is pure:
 * `getBoundingClientRect` answers zeros under jsdom, and a division by zero here would put
 * every widget in column `NaN` in a test that could not see it. A board with no width yet
 * answers column 0.
 */
export function columnAt(offsetX: number, boardWidth: number): number {
  if (!(boardWidth > 0)) return 0
  return clamp(Math.floor((offsetX / boardWidth) * COLUMNS), 0, COLUMNS - 1)
}

/** Which row an offset falls in, given the row pitch the board is drawn with. */
export function rowAt(offsetY: number, rowHeight: number): number {
  if (!(rowHeight > 0)) return 0
  return Math.max(0, Math.floor(offsetY / rowHeight))
}
