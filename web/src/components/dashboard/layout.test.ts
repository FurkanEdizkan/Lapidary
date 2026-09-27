import { expect, test } from 'vitest'
import {
  COLUMNS,
  append,
  columnAt,
  inOrder,
  moveHorizontal,
  moveVertical,
  placeAt,
  remove,
  resizeBy,
  rowAt,
  rows,
  settle,
  type Limits,
  type Tile,
} from './layout'

/**
 * The layout maths, on its own.
 *
 * This is the half of a hand-rolled grid that can be wrong invisibly. Two widgets on one cell
 * render as one on top of the other and no render test notices; a column past the twelfth
 * silently becomes a thirteenth track and the board grows a lane nobody asked for. So every
 * rule is asserted here against numbers, and the one invariant that covers the lot —
 * no two tiles overlap, ever — is asserted after each operation rather than once.
 */

function tile(key: string, x: number, y: number, w: number, h: number): Tile {
  return { key, x, y, w, h }
}

/** The invariant. Every function in the module returns a board this is true of. */
function overlapping(tiles: readonly Tile[]): string[] {
  const clashes: string[] = []
  for (const [index, a] of tiles.entries()) {
    for (const b of tiles.slice(index + 1)) {
      if (a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h) {
        clashes.push(`${a.key}/${b.key}`)
      }
    }
  }
  return clashes
}

function at(tiles: readonly Tile[], key: string): Tile {
  const found = tiles.find((t) => t.key === key)
  if (found === undefined) throw new Error(`no tile ${key}`)
  return found
}

const LIMITS: Limits = { min: { w: 2, h: 1 }, max: { w: 8, h: 4 } }

test('the board is twelve columns and nothing else', () => {
  expect(COLUMNS).toBe(12)
})

test('a lone widget rises to the top however deep it was stored', () => {
  const settled = settle([tile('storage', 0, 9, 4, 2)])
  expect(at(settled, 'storage').y).toBe(0)
  expect(rows(settled)).toBe(2)
})

test('an empty board needs no rows', () => {
  expect(rows([])).toBe(0)
})

/**
 * Compaction is what keeps the board gapless, and removing the widget above is the case that
 * proves it: everything under it moves up, and nothing is left hovering over a blank row.
 */
test('removing a widget closes the gap it leaves', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 0, 2, 6, 2), tile('c', 0, 4, 6, 2)])
  expect([at(board, 'a').y, at(board, 'b').y, at(board, 'c').y]).toEqual([0, 2, 4])
  const without = remove(board, 'a')
  expect([at(without, 'b').y, at(without, 'c').y]).toEqual([0, 2])
  expect(overlapping(without)).toEqual([])
})

test('two widgets asking for the same cell are stacked, not drawn on each other', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 0, 0, 6, 2)])
  expect(overlapping(board)).toEqual([])
  expect(at(board, 'b').y).toBe(2)
})

test('widgets side by side both stay on the top row', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 6, 0, 6, 2)])
  expect([at(board, 'a').y, at(board, 'b').y]).toEqual([0, 0])
  expect(rows(board)).toBe(2)
})

/** A stored column that no longer fits — a narrower kind, an edited localStorage — is pulled in. */
test('a widget that would hang off the right edge is moved onto the board', () => {
  const board = settle([tile('wide', 10, 0, 4, 2)])
  expect(at(board, 'wide').x).toBe(8)
  expect(at(board, 'wide').w).toBe(4)
})

test('a width larger than the board is cut to it, and a height is never zero', () => {
  const board = settle([tile('huge', 3, 0, 40, 0)])
  expect(at(board, 'huge')).toMatchObject({ x: 0, w: COLUMNS, h: 1 })
})

test('arrow left and right move by one column and stop at the edges', () => {
  let board = settle([tile('a', 4, 0, 4, 2)])
  board = moveHorizontal(board, 'a', -1)
  expect(at(board, 'a').x).toBe(3)
  board = moveHorizontal(board, 'a', 1)
  expect(at(board, 'a').x).toBe(4)
  for (let step = 0; step < 10; step += 1) board = moveHorizontal(board, 'a', -1)
  expect(at(board, 'a').x).toBe(0)
  for (let step = 0; step < 20; step += 1) board = moveHorizontal(board, 'a', 1)
  expect(at(board, 'a').x).toBe(COLUMNS - 4)
})

/**
 * Down is a reorder, and this is why: on a compacted board `y + 1` is undone by the next
 * settle, because the tile simply rises again. So "down" has to mean *past the widget below
 * me*, and after the move the two have exchanged rows rather than drifted apart.
 */
test('arrow down puts a widget below the one that was under it, and up brings it back', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 0, 2, 6, 3)])
  const down = moveVertical(board, 'a', 1)
  expect(at(down, 'b').y).toBe(0)
  expect(at(down, 'a').y).toBe(3)
  expect(overlapping(down)).toEqual([])
  const up = moveVertical(down, 'a', -1)
  expect([at(up, 'a').y, at(up, 'b').y]).toEqual([0, 2])
})

test('a widget with nothing in its columns cannot move down, and says so by not moving', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 6, 0, 6, 2)])
  const down = moveVertical(board, 'a', 1)
  expect([at(down, 'a').y, at(down, 'b').y]).toEqual([0, 0])
  const up = moveVertical(board, 'b', -1)
  expect([at(up, 'a').y, at(up, 'b').y]).toEqual([0, 0])
})

/** Only widgets that share a column are below each other; one beside it is not passed over. */
test('arrow down skips a widget in other columns and finds the next one that shares a column', () => {
  const board = settle([tile('a', 0, 0, 4, 2), tile('beside', 8, 0, 4, 2), tile('under', 0, 2, 4, 2)])
  const down = moveVertical(board, 'a', 1)
  expect(at(down, 'under').y).toBe(0)
  expect(at(down, 'a').y).toBe(2)
  expect(at(down, 'beside').y).toBe(0)
})

test('shift+arrow resizes inside the kind limits and never off the right edge', () => {
  let board = settle([tile('a', 0, 0, 4, 2)])
  board = resizeBy(board, 'a', 1, 0, LIMITS)
  expect(at(board, 'a').w).toBe(5)
  for (let step = 0; step < 10; step += 1) board = resizeBy(board, 'a', 1, 0, LIMITS)
  expect(at(board, 'a').w).toBe(LIMITS.max.w)
  for (let step = 0; step < 10; step += 1) board = resizeBy(board, 'a', -1, 0, LIMITS)
  expect(at(board, 'a').w).toBe(LIMITS.min.w)
  for (let step = 0; step < 10; step += 1) board = resizeBy(board, 'a', 0, 1, LIMITS)
  expect(at(board, 'a').h).toBe(LIMITS.max.h)
  for (let step = 0; step < 10; step += 1) board = resizeBy(board, 'a', 0, -1, LIMITS)
  expect(at(board, 'a').h).toBe(LIMITS.min.h)
})

test('a widget in the last columns grows no further than the board', () => {
  const board = resizeBy(settle([tile('a', 9, 0, 3, 2)]), 'a', 1, 0, { min: { w: 2, h: 1 }, max: { w: 12, h: 4 } })
  expect(at(board, 'a')).toMatchObject({ x: 9, w: 3 })
})

test('growing a widget pushes what was under it down instead of drawing over it', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 0, 2, 6, 2)])
  const taller = resizeBy(board, 'a', 0, 2, LIMITS)
  expect(at(taller, 'a').h).toBe(4)
  expect(at(taller, 'b').y).toBe(4)
  expect(overlapping(taller)).toEqual([])
})

test('a drag lands a widget in the column and row it was dropped on', () => {
  const board = settle([tile('a', 0, 0, 4, 2), tile('b', 0, 2, 4, 2)])
  const dropped = placeAt(board, 'b', 4, 0)
  expect(at(dropped, 'b')).toMatchObject({ x: 4, y: 0 })
  expect(at(dropped, 'a').y).toBe(0)
  expect(overlapping(dropped)).toEqual([])
})

test('dropping onto an occupied row lands above it and moves it down', () => {
  const board = settle([tile('a', 0, 0, 12, 2), tile('b', 0, 2, 12, 2)])
  const dropped = placeAt(board, 'b', 0, 0)
  expect(at(dropped, 'b').y).toBe(0)
  expect(at(dropped, 'a').y).toBe(2)
})

test('a new widget goes under what is already there', () => {
  const board = append(settle([tile('a', 0, 0, 12, 2)]), tile('b', 0, 0, 6, 3))
  expect(at(board, 'b').y).toBe(2)
  expect(rows(board)).toBe(5)
})

test('a key nothing knows leaves the board alone', () => {
  const board = settle([tile('a', 0, 0, 4, 2)])
  expect(moveHorizontal(board, 'gone', 1)).toEqual(board)
  expect(moveVertical(board, 'gone', 1)).toEqual(board)
  expect(resizeBy(board, 'gone', 1, 1, LIMITS)).toEqual(board)
  expect(placeAt(board, 'gone', 4, 4)).toEqual(board)
  expect(remove(board, 'gone')).toEqual(board)
})

/**
 * A reload has only rows and columns to go on — the order is not stored, because it is the
 * rows that a person saw. Reading order is what turns one back into the other.
 */
test('reading order recovers the order a board was left in', () => {
  const stored = [tile('c', 6, 2, 6, 2), tile('a', 0, 0, 6, 2), tile('b', 6, 0, 6, 2), tile('d', 0, 2, 6, 2)]
  expect(inOrder(stored).map((t) => t.key)).toEqual(['a', 'b', 'd', 'c'])
  const board = settle(inOrder(stored))
  expect(board.map((t) => `${t.key}@${t.x},${t.y}`)).toEqual(['a@0,0', 'b@6,0', 'd@0,2', 'c@6,2'])
})

/**
 * jsdom measures every element as zero, and a browser measures a board that has not been laid
 * out yet the same way. A division by that would put every widget in column NaN.
 */
test('a board with no width yet answers column zero rather than NaN', () => {
  expect(columnAt(240, 0)).toBe(0)
  expect(rowAt(240, 0)).toBe(0)
  expect(Number.isNaN(columnAt(240, 0))).toBe(false)
})

test('an offset inside the board names its column, and one past the end names the last', () => {
  expect(columnAt(0, 1200)).toBe(0)
  expect(columnAt(350, 1200)).toBe(3)
  expect(columnAt(1199, 1200)).toBe(11)
  expect(columnAt(4000, 1200)).toBe(11)
  expect(columnAt(-40, 1200)).toBe(0)
  expect(rowAt(0, 90)).toBe(0)
  expect(rowAt(271, 90)).toBe(3)
  expect(rowAt(-10, 90)).toBe(0)
})

/**
 * Twelve widgets of assorted sizes, every operation in turn, and the invariant after each
 * one. A single hand-written arrangement passing is not evidence the packer holds.
 */
test('no sequence of moves and resizes ever leaves two widgets on one cell', () => {
  let board = settle(
    Array.from({ length: 12 }, (_, index) =>
      tile(`w${index}`, (index * 5) % COLUMNS, index, 2 + (index % 5), 1 + (index % 3)),
    ),
  )
  expect(overlapping(board)).toEqual([])
  for (const [index, key] of board.map((t) => t.key).entries()) {
    board = moveHorizontal(board, key, index % 2 === 0 ? 1 : -1)
    expect(overlapping(board)).toEqual([])
    board = moveVertical(board, key, index % 3 === 0 ? 1 : -1)
    expect(overlapping(board)).toEqual([])
    board = resizeBy(board, key, index % 2 === 0 ? 1 : -1, index % 3 === 0 ? 1 : -1, LIMITS)
    expect(overlapping(board)).toEqual([])
    board = placeAt(board, key, index % COLUMNS, index % 4)
    expect(overlapping(board)).toEqual([])
  }
  expect(board).toHaveLength(12)
  // And still on the board: every column inside the twelve, every row at or above the deepest.
  for (const t of board) {
    expect(t.x).toBeGreaterThanOrEqual(0)
    expect(t.x + t.w).toBeLessThanOrEqual(COLUMNS)
    expect(t.y).toBeGreaterThanOrEqual(0)
  }
})

/**
 * `Number.MAX_SAFE_INTEGER` is how every caller says "last": a new widget, one crossing between
 * groups, one whose group was removed. Before the clamp, the pull-up loop counted down from it a
 * row at a time — nine quadrillion iterations, which reads as a hung page and not as a wrong
 * position, and is exactly how it was found.
 */
test('a row past the bottom of the board is the bottom, and settles at once', () => {
  const started = Date.now()
  const board = settle([
    tile('a', 0, 0, 12, 2),
    tile('b', 0, 2, 12, 2),
    tile('last', 0, Number.MAX_SAFE_INTEGER, 6, 3),
  ])
  expect(at(board, 'last').y).toBe(4)
  expect(rows(board)).toBe(7)
  expect(overlapping(board)).toEqual([])
  // A guard on the shape of the bug rather than on a duration: the wrong version does not finish.
  expect(Date.now() - started).toBeLessThan(1000)
})

test('an infinite or missing row does not become a row at all', () => {
  const board = settle([tile('a', 0, 0, 6, 2), tile('b', 0, Number.POSITIVE_INFINITY, 6, 2)])
  expect(at(board, 'b').y).toBe(2)
  expect(Number.isFinite(at(board, 'b').y)).toBe(true)
})
