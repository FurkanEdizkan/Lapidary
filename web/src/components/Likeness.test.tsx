import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { Likeness as LikenessSection, ReviewOffer } from './Likeness'
import { RemovedPage } from '../routes/removed'
import { strings } from '../lib/strings'
import type {
  BatchStatus,
  DuplicateCluster,
  Fold,
  LibraryId,
  Likeness,
  PartCard,
  PartId,
} from '../lib/types'

/**
 * What a person sees when two parts look alike, and what they can decide about it.
 *
 * Three product rules are asserted here because nothing else can assert them. **No number
 * reaches the screen** — a person reads "identical", "near-duplicate" or "similar", never 0.038,
 * and the only figure on a row is the part's own triangle count, labelled approximate as every
 * mesh-derived figure in this interface is. **"Fold into", never "merge"** — `CLAUDE.md` reserves
 * merge for versioning. And **a fold takes nothing away**: it is the soft delete, the folded part
 * keeps everything, the Removed list says where it went, and Restore brings it back. The dialog
 * has to say all three, so the dialog's words are asserted and not merely its presence.
 *
 * The fold *direction* is the other thing only a test sees. On a part's page, folding a
 * near-duplicate acts on the near-duplicate; getting it backwards removes the part the person is
 * reading and looks like success.
 */

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId

/**
 * One bracket, in two folders, which is the case this whole feature is for: a scan found
 * `LP-1042-03.stl` under `mounting/` and again under `spares/`, and since slice 6a the path is
 * the only thing that tells them apart. `removed.test.tsx` uses the same pair.
 */
const MOUNTING: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0001' as PartId,
  library: LIBRARY,
  revision: '01931b6e-0000-7000-8000-0000000b0001' as PartCard['revision'],
  name: 'Bracket, LP-1042-03',
  partNumber: 'LP-1042-03',
  sourcePath: 'mounting/LP-1042-03.stl',
  thumbnail: null,
  triangleCount: 48112,
  approximate: true,
  tessellationL0: null,
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222' as PartCard['sourceHash'],
  sourceBytes: 204800,
  storedBytes: 91204,
  compressed: true,
  directory: 'libraries/default/mounting/lp-1042-03',
  storagePath: 'libraries/default/mounting/lp-1042-03/lp-1042-03.stl',
  createdAt: '2026-09-06T10:00:00Z',
  updatedAt: '2026-09-06T10:00:00Z',
  removedAt: null,
}

const SPARES: PartCard = {
  ...MOUNTING,
  id: '01931b6e-0000-7000-8000-0000000a0002' as PartId,
  revision: '01931b6e-0000-7000-8000-0000000b0002' as PartCard['revision'],
  sourcePath: 'spares/LP-1042-03.stl',
  createdAt: '2026-09-26T09:05:00Z',
  updatedAt: '2026-09-26T09:05:00Z',
}

/** A different part, close in shape and not in size: the "more like this" case. */
const GUSSET: PartCard = {
  ...MOUNTING,
  id: '01931b6e-0000-7000-8000-0000000a0003' as PartId,
  revision: '01931b6e-0000-7000-8000-0000000b0003' as PartCard['revision'],
  name: 'Gusset, LP-1090-01',
  partNumber: 'LP-1090-01',
  sourcePath: 'mounting/LP-1090-01.stl',
  triangleCount: 12904,
}

/** A variant somebody already linked: the left hand of a mirrored pair. */
const MIRROR: PartCard = {
  ...MOUNTING,
  id: '01931b6e-0000-7000-8000-0000000a0004' as PartId,
  revision: '01931b6e-0000-7000-8000-0000000b0004' as PartCard['revision'],
  name: 'Bracket, LP-1042-04 (left)',
  partNumber: 'LP-1042-04',
  sourcePath: 'mounting/LP-1042-04.stl',
}

const FULL: Likeness = {
  profiled: true,
  identical: [SPARES],
  nearDuplicates: [GUSSET],
  similar: [MIRROR],
  variants: [MIRROR],
}

type Call = { url: string; method: string; body: unknown }

/**
 * Answers the reads and records the writes. `state` is mutable on purpose: the fold test needs
 * the POST to change what the removed list and the folds list say afterwards, because two
 * independent snapshots would assert two facts and not one flow.
 */
function stub(state: {
  likeness?: Likeness
  status?: number
  clusters?: DuplicateCluster[]
  unprofiled?: number
  removed?: PartCard[]
  folds?: Fold[]
}) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET'
      calls.push({ url, method, body: init?.body === undefined ? undefined : JSON.parse(String(init.body)) })
      const answer = (status: number, body: unknown) => ({ ok: status < 300, status, json: async () => body })
      if (url.endsWith('/likeness')) {
        return answer(state.status ?? 200, state.likeness ?? FULL)
      }
      if (url.includes('/duplicates')) {
        return answer(200, { clusters: state.clusters ?? [], unprofiled: state.unprofiled ?? 0 })
      }
      if (url.includes('/folds')) return answer(200, state.folds ?? [])
      if (url.endsWith('/fold') || url.includes('/links/')) return answer(204, {})
      // The removed list, which the fold flow reads after the fold went through.
      return answer(200, { parts: state.removed ?? [], next: null })
    }),
  )
  return calls
}

function mount(element: React.ReactNode) {
  const rootRoute = createRootRoute({ component: () => <>{element}</> })
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

/** The list under a heading, so an assertion names the kind of alike it means. */
function listUnder(title: string): HTMLElement {
  const heading = screen.getByRole('heading', { name: title })
  const list = heading.parentElement?.querySelector('ul')
  expect(list).not.toBeNull()
  expect(list?.getAttribute('role')).toBe('list')
  return list as HTMLElement
}

test('a part page names each kind of alike, and lists the parts in it', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  within(listUnder(strings.likeness.identical)).getByText(SPARES.sourcePath)
  within(listUnder(strings.likeness.nearDuplicates)).getByText(GUSSET.sourcePath)
  within(listUnder(strings.likeness.variants)).getByText(MIRROR.sourcePath)
  within(listUnder(strings.likeness.similar)).getByText(MIRROR.sourcePath)
  // One section, one h3, so the part page's index links it once.
  const section = document.getElementById('part-likeness')
  expect(section?.querySelectorAll('h3')).toHaveLength(1)
  expect(section?.querySelector('h3')?.textContent).toBe(strings.likeness.title)
})

test('a kind with nothing in it draws no heading and no empty list', async () => {
  stub({ likeness: { ...FULL, variants: [], similar: [] } })
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  expect(screen.queryByRole('heading', { name: strings.likeness.variants })).toBeNull()
  expect(screen.queryByRole('heading', { name: strings.likeness.similar })).toBeNull()
})

/**
 * The open questions get the decisions; the settled and the browsed do not. A variant was already
 * decided about, and "Similar" is top-k by shape whatever the size — writing "not the same" from
 * it would record a decision about a pair nothing ever proposed.
 */
test('the three decisions are offered on identical and near-duplicates, and nowhere else', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  // Found by their accessible names, which are the per-part labels — the visible words are
  // "Fold into this", and five of those in a list say nothing about which row they are on.
  for (const [title, card] of [
    [strings.likeness.identical, SPARES],
    [strings.likeness.nearDuplicates, GUSSET],
  ] as const) {
    const list = within(listUnder(title))
    expect(list.getByRole('button', { name: strings.likeness.foldLabel(card.sourcePath) }).textContent).toBe(
      strings.likeness.fold,
    )
    list.getByRole('button', { name: strings.likeness.variantLabel(card.sourcePath) })
    list.getByRole('button', { name: strings.likeness.distinctLabel(card.sourcePath) })
    expect(list.getAllByRole('button')).toHaveLength(3)
  }
  for (const title of [strings.likeness.variants, strings.likeness.similar]) {
    expect(within(listUnder(title)).queryByRole('button')).toBeNull()
  }
})

test('each decision button names its own part, since two of them share a name', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  screen.getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) })
  screen.getByRole('button', { name: strings.likeness.variantLabel(SPARES.sourcePath) })
  screen.getByRole('button', { name: strings.likeness.distinctLabel(SPARES.sourcePath) })
})

/**
 * WCAG 2.5.3, Label in Name (Level A): every one of these buttons is named for its own part, and
 * a name that drops the visible words is a button somebody driving this by voice cannot press —
 * they say "click Fold into this" and nothing matches. `failure.retryOne` is the same pattern.
 */
test('every decision button’s accessible name begins with the words on it', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  const buttons = screen.getAllByRole('button')
  expect(buttons.length).toBeGreaterThan(0)
  for (const button of buttons) {
    const name = button.getAttribute('aria-label') ?? button.textContent ?? ''
    // Reported as a pair, so a failure names the button and not merely `false`.
    expect({ visible: button.textContent, name, startsWith: name.startsWith(button.textContent ?? '') })
      .toEqual({ visible: button.textContent, name, startsWith: true })
  }
})

test('a part with no shape profile says so instead of showing empty lists', async () => {
  stub({ likeness: { profiled: false, identical: [], nearDuplicates: [], similar: [], variants: [] } })
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByText(strings.likeness.unprofiled)
  expect(screen.queryByRole('heading', { name: strings.likeness.nearDuplicates })).toBeNull()
})

test('a profiled part with nothing alike says that, which is a different sentence', async () => {
  stub({ likeness: { profiled: true, identical: [], nearDuplicates: [], similar: [], variants: [] } })
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByText(strings.likeness.nothing)
  expect(screen.queryByText(strings.likeness.unprofiled)).toBeNull()
})

test('a read that fails says so rather than loading for ever', async () => {
  stub({ status: 503 })
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByText(strings.likeness.failed)
})

/**
 * `CLAUDE.md`'s measurement rule, on the one figure a row shows. The judgement behind the row has
 * no figure at all — see the test below — so this is the only thing here to label.
 */
test('a row’s triangle count is labelled approximate, because it is measured from a mesh', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  const row = within(listUnder(strings.likeness.identical)).getByText(SPARES.sourcePath).closest('li')
  expect(row?.textContent).toContain(strings.parts.triangles(48112))
  expect(row?.textContent).toContain(strings.parts.approximate)
})

/**
 * The rule with no field behind it: `Likeness` carries no score, so there is nothing to leak —
 * and this fails the day somebody adds one and renders it. A distance would read as `0.038`, a
 * percentage as `96%`; neither may appear beside a part.
 */
test('no similarity figure reaches a row — a person reads one of three words', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.nearDuplicates })
  const list = listUnder(strings.likeness.nearDuplicates)
  expect(list.textContent).not.toMatch(/\d+\.\d+/)
  expect(list.textContent).not.toMatch(/\d\s?%/)
  // And the vocabulary is the three words, not a fourth of somebody's invention.
  expect(
    [strings.likeness.identical, strings.likeness.nearDuplicates, strings.likeness.similar],
  ).toEqual(['Identical', 'Near-duplicates', 'Similar'])
})

test('nothing in this feature is ever called a merge', () => {
  // Every entry in the block, builders included: the word must not be in a sentence either.
  // Cast because `Object.values` of the block is a union of signatures, whose parameters
  // intersect to `never`; each builder here takes strings or numbers and tolerates both.
  const said = JSON.stringify(
    Object.values(strings.likeness).map((entry) =>
      typeof entry === 'function'
        ? (entry as (a: string, b: string) => string)('a path', 'another path')
        : entry,
    ),
  )
  expect(said.toLowerCase()).not.toContain('merge')
})

/**
 * The dialog carries three facts, and none of them is optional: which part goes and which is kept
 * — by path, because both are called `Bracket, LP-1042-03` — that nothing is deleted and nothing
 * moves, and that Restore undoes it.
 */
test('the fold dialog says which part goes, that nothing is lost, and that Restore undoes it', async () => {
  stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) }))

  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByRole('heading', { name: strings.likeness.foldTitle })).toBeTruthy()
  const words = strings.likeness.foldConfirm(SPARES.sourcePath, MOUNTING.sourcePath)
  within(dialog).getByText(words)
  // Both parts, by path, and the right way round: the near-duplicate goes into the page's part.
  expect(words).toContain(SPARES.sourcePath)
  expect(words).toContain(MOUNTING.sourcePath)
  expect(words).toContain('Nothing is deleted and nothing moves')
  expect(words).toContain('Restore brings it back')
  expect(words).toContain('the Removed list says where it went')
  // Cancel has focus, so a reflexive Enter keeps both parts.
  expect(document.activeElement?.textContent).toBe(strings.folders.cancel)
})

test('cancelling the dialog folds nothing', async () => {
  const calls = stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) }))
  const dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: strings.folders.cancel }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(calls.some((call) => call.method === 'POST')).toBe(false)
})

/**
 * The flow, in one stub: fold the second copy into the first, then open the Removed list and read
 * where it went. Two separate renders of two fixtures would assert that the page can draw a line;
 * this asserts that folding is what puts it there.
 */
test('a folded part is removed, and the Removed list says which part it went into', async () => {
  const state: Parameters<typeof stub>[0] = { removed: [], folds: [] }
  const calls = stub(state)
  const first = mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.foldLabel(SPARES.sourcePath) }))
  const dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: strings.likeness.foldConfirmAction }))

  // The part in the URL is the one that goes; the body names the part kept. Backwards, this
  // removes the part whose page the person is reading.
  await waitFor(() =>
    expect(calls).toContainEqual({
      url: `/api/parts/${SPARES.id}/fold`,
      method: 'POST',
      body: { into: MOUNTING.id },
    }),
  )
  state.removed = [{ ...SPARES, removedAt: '2026-09-26T09:20:00Z' }]
  state.folds = [{ part: SPARES.id, into: MOUNTING }]
  first.unmount()

  mount(<RemovedPage library={LIBRARY} />)
  const where = await screen.findByRole('link', { name: strings.likeness.foldedInto(MOUNTING.name) })
  expect(where.getAttribute('href')).toBe(`/parts/${MOUNTING.id}`)
  // And the way back is the ordinary one, because a fold is the ordinary removal.
  screen.getByRole('button', { name: strings.removal.restore })
})

/**
 * `/folds` answers with the kept part whether it is live or itself removed, so that "folded into X"
 * stays true whichever it is (G3's handler says so in as many words). A removed part has no page —
 * every read path filters `deleted_at` — so the sentence stays and the press goes: linking would
 * offer a click that lands on "could not open this part".
 */
test('a part folded into one that was removed since still says so, without a link', async () => {
  stub({
    removed: [
      { ...SPARES, removedAt: '2026-09-26T09:20:00Z' },
      { ...MOUNTING, removedAt: '2026-09-26T11:00:00Z' },
    ],
    folds: [{ part: SPARES.id, into: { ...MOUNTING, removedAt: '2026-09-26T11:00:00Z' } }],
  })
  mount(<RemovedPage library={LIBRARY} />)
  const said = await screen.findByText(strings.likeness.foldedInto(MOUNTING.name))
  expect(said.tagName).toBe('SPAN')
  expect(screen.queryByRole('link', { name: strings.likeness.foldedInto(MOUNTING.name) })).toBeNull()
  // And the way back is still the ordinary one, on both rows.
  expect(screen.getAllByRole('button', { name: strings.removal.restore })).toHaveLength(2)
})

test('an ordinary removal says nothing about folding', async () => {
  stub({ removed: [{ ...SPARES, removedAt: '2026-09-26T09:20:00Z' }], folds: [] })
  mount(<RemovedPage library={LIBRARY} />)
  await screen.findByText(SPARES.sourcePath)
  expect(screen.queryByRole('link', { name: strings.likeness.foldedInto(MOUNTING.name) })).toBeNull()
})

test('"they belong together" and "not the same" each write their own kind', async () => {
  const calls = stub({})
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.variantLabel(SPARES.sourcePath) }))
  await waitFor(() =>
    expect(calls).toContainEqual({
      url: `/api/parts/${MOUNTING.id}/links/${SPARES.id}`,
      method: 'PUT',
      body: { kind: 'variant' },
    }),
  )
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.distinctLabel(GUSSET.sourcePath) }))
  await waitFor(() =>
    expect(calls).toContainEqual({
      url: `/api/parts/${MOUNTING.id}/links/${GUSSET.id}`,
      method: 'PUT',
      body: { kind: 'distinct' },
    }),
  )
})

test('a refused decision says so and claims nothing changed', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') return { ok: false, status: 409, json: async () => ({}) }
      return { ok: true, status: 200, json: async () => FULL }
    }),
  )
  mount(<LikenessSection part={MOUNTING} />)
  await screen.findByRole('heading', { name: strings.likeness.identical })
  fireEvent.click(screen.getByRole('button', { name: strings.likeness.variantLabel(SPARES.sourcePath) }))
  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toBe(strings.likeness.actionFailed)
})

const FINISHED: BatchStatus = {
  batchId: '01931b6e-0000-7000-8000-0000000c0001' as BatchStatus['batchId'],
  libraryId: LIBRARY,
  total: 4,
  pending: 0,
  running: 0,
  ingested: 3,
  skipped: 1,
  rendered: 3,
  revised: 0,
  unkept: 0,
  scanned: 0,
  migrated: 0,
  migrating: 0,
  failedTotal: 0,
  failed: [],
  startedAt: '2026-09-26T09:00:00Z',
  finishedAt: '2026-09-26T09:10:00Z',
}

/**
 * One group, holding the part that was already here and *two* copies the upload just added.
 *
 * Two, not one, and that is the fixture doing work: the line counts parts, so this group is two
 * things to look at and a count of groups would report it as one. A batch of eleven copies of one
 * bracket is the ordinary way this happens.
 */
const PAIR: DuplicateCluster = {
  parts: [
    MOUNTING,
    SPARES,
    { ...SPARES, id: '01931b6e-0000-7000-8000-0000000a0005' as PartId, sourcePath: 'spares/LP-1042-03 (1).stl' },
  ],
  identical: true,
}

test('a finished upload offers a review of what looks like parts already here', async () => {
  stub({ clusters: [PAIR] })
  mount(<ReviewOffer status={FINISHED} kind="upload" library={LIBRARY} />)
  // Announced, not only drawn: the line arrives on its own, after the batch finished and the page
  // stopped changing, so a reader not looking at the grid is told nothing by it otherwise.
  expect((await screen.findByText(strings.likeness.reviewOffer(2))).closest('[role=status]')).not.toBeNull()
  // Two parts just added, in one group. Counting groups would say 1 and send somebody to review
  // half of what they uploaded.
  await screen.findByText(strings.likeness.reviewOffer(2))
  const review = screen.getByRole('link', { name: strings.likeness.review })
  // The batch's own queue, not the whole library's: `since` is where the batch started.
  expect(review.getAttribute('href')).toContain(`since=${encodeURIComponent(FINISHED.startedAt)}`)
  expect(review.getAttribute('href')).toContain('/duplicates')
})

test('nothing is offered, and nothing is asked, while a batch is still running', async () => {
  const calls = stub({ clusters: [PAIR] })
  mount(<ReviewOffer status={{ ...FINISHED, finishedAt: null, pending: 2 }} kind="upload" library={LIBRARY} />)
  await waitFor(() => expect(screen.queryByRole('link')).toBeNull())
  expect(calls.some((call) => call.url.includes('/duplicates'))).toBe(false)
})

/** A thumbnail sweep and a storage migration add no parts, so neither has duplicates to offer. */
test('a render sweep is not offered a duplicate review', async () => {
  const calls = stub({ clusters: [PAIR] })
  mount(<ReviewOffer status={FINISHED} kind="render" library={LIBRARY} />)
  await waitFor(() => expect(screen.queryByRole('link')).toBeNull())
  expect(calls.some((call) => call.url.includes('/duplicates'))).toBe(false)
})

test('a finished upload where nothing looks alike says nothing at all', async () => {
  stub({ clusters: [] })
  mount(<ReviewOffer status={FINISHED} kind="upload" library={LIBRARY} />)
  await waitFor(() => expect(screen.queryByRole('link')).toBeNull())
  expect(screen.queryByText(strings.likeness.review)).toBeNull()
})
