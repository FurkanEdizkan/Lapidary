import { beforeEach, expect, test, vi } from 'vitest'
import {
  fetchDuplicates,
  fetchFolds,
  fetchLikeness,
  foldPart,
  justAdded,
  newestFirst,
  setPartLink,
} from './likeness'
import type { DuplicateCluster, PartCard, PartId } from './types'

/**
 * The likeness routes and the two pure functions around them.
 *
 * The fold direction is the one thing here that cannot be checked by looking at the screen: a
 * fold in the wrong direction succeeds, says nothing, and removes the part whose page the
 * person was reading. So both halves — which part is in the URL, and which is in the body —
 * are asserted rather than one.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001'
const MOUNTING = '01931b6e-0000-7000-8000-0000000a0001' as PartId
const SPARES = '01931b6e-0000-7000-8000-0000000a0002' as PartId

type Call = { url: string; method: string; body: unknown }

function stub(status = 200, body: unknown = {}) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({
        url,
        method: init?.method ?? 'GET',
        body: init?.body === undefined ? undefined : JSON.parse(String(init.body)),
      })
      return { ok: status < 300, status, json: async () => body }
    }),
  )
  return calls
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

test('a part’s likeness is read from its own route', async () => {
  const calls = stub(200, { profiled: true, identical: [], nearDuplicates: [], similar: [], variants: [] })
  const likeness = await fetchLikeness(MOUNTING)
  expect(likeness.profiled).toBe(true)
  expect(calls).toEqual([{ url: `/api/parts/${MOUNTING}/likeness`, method: 'GET', body: undefined }])
})

test('the queue is read per library, and a batch narrows it by when it started', async () => {
  const calls = stub(200, { clusters: [], unprofiled: 0 })
  await fetchDuplicates(LIBRARY)
  await fetchDuplicates(LIBRARY, '2026-09-26T09:15:00Z')
  expect(calls.map((call) => call.url)).toEqual([
    `/api/libraries/${LIBRARY}/duplicates`,
    // Encoded: a timestamp carries a `+03:00` offset on a server that does not send `Z`,
    // and a bare `+` in a query string is a space.
    `/api/libraries/${LIBRARY}/duplicates?since=2026-09-26T09%3A15%3A00Z`,
  ])
})

test('a fold names the part that goes in the URL and the part kept in the body', async () => {
  const calls = stub(204)
  // Standing on MOUNTING's page, folding the near-duplicate SPARES into it.
  await foldPart(SPARES, MOUNTING)
  expect(calls).toEqual([
    { url: `/api/parts/${SPARES}/fold`, method: 'POST', body: { into: MOUNTING } },
  ])
})

test('a decision about a pair is a PUT of its kind, and folding is not one of them', async () => {
  const calls = stub(204)
  await setPartLink(MOUNTING, SPARES, 'variant')
  await setPartLink(MOUNTING, SPARES, 'distinct')
  expect(calls).toEqual([
    { url: `/api/parts/${MOUNTING}/links/${SPARES}`, method: 'PUT', body: { kind: 'variant' } },
    { url: `/api/parts/${MOUNTING}/links/${SPARES}`, method: 'PUT', body: { kind: 'distinct' } },
  ])
  // @ts-expect-error `foldedInto` is refused at the route and excluded here, so it cannot compile.
  await setPartLink(MOUNTING, SPARES, 'foldedInto')
})

test('a refused read throws rather than answering with a shape the page would render', async () => {
  stub(500)
  await expect(fetchLikeness(MOUNTING)).rejects.toThrow()
  stub(404)
  await expect(fetchFolds(LIBRARY)).rejects.toThrow()
})

function card(id: string, createdAt: string): PartCard {
  return {
    id: id as PartId,
    library: LIBRARY as PartCard['library'],
    revision: `${id}-rev` as PartCard['revision'],
    name: 'Bracket, LP-1042-03',
    partNumber: 'LP-1042-03',
    sourcePath: `mounting/${id}.stl`,
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
    createdAt,
    updatedAt: createdAt,
    removedAt: null,
  }
}

function cluster(parts: PartCard[], identical = false): DuplicateCluster {
  return { parts, identical }
}

test('the queue puts the group holding the newest part first', () => {
  const old = cluster([card('a', '2026-09-01T10:00:00Z'), card('b', '2026-09-02T10:00:00Z')])
  const fresh = cluster([card('c', '2026-09-01T10:00:00Z'), card('d', '2026-09-25T10:00:00Z')])
  expect(newestFirst([old, fresh]).map((group) => group.parts[1]?.id)).toEqual(['d', 'b'])
})

test('an offset and a Z timestamp naming one instant sort as one instant', () => {
  // `2026-09-25T13:00:00+03:00` is 10:00Z, so it is older than 11:00Z — which string
  // comparison gets backwards, since '1' sorts after '0'.
  const offset = cluster([card('a', '2026-09-25T13:00:00+03:00')])
  const zulu = cluster([card('b', '2026-09-25T11:00:00Z')])
  expect(newestFirst([offset, zulu]).map((group) => group.parts[0]?.id)).toEqual(['b', 'a'])
})

test('the finished line counts the parts just added, not the groups they landed in', () => {
  const since = '2026-09-26T09:00:00Z'
  const older = card('old', '2026-09-01T10:00:00Z')
  const groups = [
    // Two just-added copies of one older part: one group, two parts to look at.
    cluster([older, card('new-1', '2026-09-26T09:05:00Z'), card('new-2', '2026-09-26T09:06:00Z')]),
    cluster([card('old-2', '2026-08-01T10:00:00Z'), card('new-3', '2026-09-26T09:07:00Z')]),
  ]
  expect(justAdded(groups, since)).toBe(3)
  expect(justAdded([cluster([older, card('old-3', '2026-09-01T10:00:00Z')])], since)).toBe(0)
})

test('a timestamp that cannot be read counts nothing rather than everything', () => {
  const groups = [cluster([card('a', '2026-09-26T09:05:00Z'), card('b', '2026-09-26T09:06:00Z')])]
  expect(justAdded(groups, 'not a timestamp')).toBe(0)
})
