import { afterEach, expect, test, vi } from 'vitest'
import {
  LETTER_JUMP_AT,
  OTHER_INITIAL,
  byCount,
  byInitial,
  byName,
  fetchRelatedTags,
  fetchTagIndex,
  initialOf,
  letterId,
} from './tags'
import { RefusedError } from './api'
import type { TagCount } from './types'

const LIBRARY = '01931b6e-0000-7000-8000-000000000001'

/** What a creator library of terrain and miniatures actually carries. */
const CORPUS: TagCount[] = [
  { value: '28 mm', count: 41 },
  { value: 'dragon', count: 12 },
  { value: 'pre-supported', count: 12 },
  { value: 'terrain', count: 9 },
]

function answers(body: unknown, ok = true, status = 200) {
  // The url is a declared parameter so the calls this returns are typed, which is what the
  // assertions below read: a `vi.fn(async () => …)` records calls of length zero.
  const fetchMock = vi.fn(async (_url: string) => ({ ok, status, json: async () => body }))
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => vi.unstubAllGlobals())

test('the index reads a library’s whole tag list', async () => {
  const fetchMock = answers({ tags: CORPUS })
  expect(await fetchTagIndex(LIBRARY)).toEqual(CORPUS)
  expect(fetchMock.mock.calls[0]?.[0]).toBe(`/api/libraries/${LIBRARY}/tags`)
})

test('the index refuses a 200 that is not a list', async () => {
  // The same guard `fetchFolds` and the dashboard's resolve make. `/tags` is the only route to a tag
  // that is not already in front of you, and `.map` over an object takes the whole page down.
  answers({ tags: { dragon: 12 } })
  await expect(fetchTagIndex(LIBRARY)).rejects.toBeInstanceOf(RefusedError)
})

test('the index carries a refusal’s reason', async () => {
  answers({ message: 'no', reason: 'noSuchLibrary' }, false, 404)
  await expect(fetchTagIndex(LIBRARY)).rejects.toMatchObject({ reason: 'noSuchLibrary' })
})

test('related tags ride on a query parameter, so a tag with a slash survives', async () => {
  const near = { tag: 'jig/fixture', parts: 3, floor: 2, related: [{ value: '28 mm', count: 3 }] }
  const fetchMock = answers(near)
  expect(await fetchRelatedTags(LIBRARY, 'jig/fixture')).toEqual(near)
  // A path segment would have had to carry an encoded slash past every proxy on the way here.
  expect(fetchMock.mock.calls[0]?.[0]).toBe(
    `/api/libraries/${LIBRARY}/tags/related?tag=jig%2Ffixture`,
  )
})

test('related tags encode a space, a plus and a hash', async () => {
  const fetchMock = answers({ tag: 'x', parts: 1, floor: 2, related: [] })
  await fetchRelatedTags(LIBRARY, 'pre-supported & 28 mm+#1')
  expect(fetchMock.mock.calls[0]?.[0]).toBe(
    `/api/libraries/${LIBRARY}/tags/related?tag=pre-supported+%26+28+mm%2B%231`,
  )
})

test('related tags refuse a 200 without a list', async () => {
  answers({ tag: 'dragon', parts: 2, floor: 2 })
  await expect(fetchRelatedTags(LIBRARY, 'dragon')).rejects.toBeInstanceOf(RefusedError)
})

test('by count, ties break alphabetically rather than by whatever the planner felt like', () => {
  const jumbled: TagCount[] = [
    { value: 'terrain', count: 9 },
    { value: 'pre-supported', count: 12 },
    { value: '28 mm', count: 41 },
    { value: 'dragon', count: 12 },
  ]
  expect(byCount(jumbled).map((tag) => tag.value)).toEqual([
    '28 mm',
    'dragon',
    'pre-supported',
    'terrain',
  ])
  // Sorting does not disturb the list it was given: two orders of one answer, on one page.
  expect(jumbled[0]?.value).toBe('terrain')
})

test('by name is a reader’s order, not a byte order', () => {
  const tags: TagCount[] = [
    { value: 'Éclair', count: 1 },
    { value: 'zulu', count: 1 },
    { value: 'elf', count: 1 },
  ]
  // A plain `<` puts `zulu` before `Éclair`, because `É` is past `z` in code-point order.
  expect(byName(tags).map((tag) => tag.value)).toEqual(['Éclair', 'elf', 'zulu'])
})

test('a tag that does not start with a letter is filed under one heading', () => {
  expect(initialOf('dragon')).toBe('D')
  expect(initialOf('  terrain')).toBe('T')
  expect(initialOf('Éclair')).toBe('É')
  expect(initialOf('28 mm')).toBe(OTHER_INITIAL)
  expect(initialOf('#wip')).toBe(OTHER_INITIAL)
  expect(initialOf('')).toBe(OTHER_INITIAL)
})

test('the letter groups come from the tags, so no heading is empty and no jump link is dead', () => {
  const groups = byInitial([
    { value: 'terrain', count: 9 },
    { value: '28 mm', count: 41 },
    { value: 'dragon', count: 12 },
    { value: 'dungeon', count: 4 },
  ])
  expect(groups.map((group) => group.initial)).toEqual([OTHER_INITIAL, 'D', 'T'])
  expect(groups[1]?.tags.map((tag) => tag.value)).toEqual(['dragon', 'dungeon'])
  expect(letterId('D')).toBe('tags-d')
  expect(letterId(OTHER_INITIAL)).toBe('tags-other')
})

test('the jump threshold is a number the page can reach', () => {
  // Not a rule a test can check by itself; what it can check is that it is a count and not a flag,
  // so `tags.length > LETTER_JUMP_AT` means something for a library of a few hundred tags.
  expect(LETTER_JUMP_AT).toBeGreaterThan(0)
  expect(Number.isInteger(LETTER_JUMP_AT)).toBe(true)
})
