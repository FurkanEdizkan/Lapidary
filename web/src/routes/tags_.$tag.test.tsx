import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRouter } from '@tanstack/react-router'
import { afterEach, expect, test, vi } from 'vitest'
import { routeTree } from '../routeTree.gen'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { PartCard, RelatedTags, SavedFilter } from '../lib/types'

/**
 * `/tags/{tag}` — the grid pre-filtered, through the real route tree.
 *
 * The real tree twice over. A tag page is not a component, it is an address: the tag arrives as a
 * path segment and has to come back out as the tag somebody typed, and every control on the page has
 * to write its change back to *this* URL rather than to `/`. A synthetic tree would answer both of
 * those questions with routes this test wrote.
 *
 * jsdom draws no WebGL and the grid's cards do not ask it to, so nothing here is stubbed but `fetch`.
 */
const BASALT: PartCard = {
  id: '01931b6e-0000-7000-8000-0000000a0007',
  library: DEFAULT_LIBRARY_ID,
  revision: '01931b6e-0000-7000-8000-0000000b0007',
  name: 'Basalt cliff face, 180 mm span',
  partNumber: 'LP-7710-C',
  sourcePath: 'Terrain/Rocks/basalt-cliff-face.stl',
  thumbnail: null,
  triangleCount: 184_320,
  approximate: true,
  sourceHash: null,
  tessellationL0: null,
  sourceBytes: 9_216_044,
  storedBytes: 2_411_008,
  compressed: true,
  directory: 'libraries/default/Terrain/Rocks',
  storagePath: 'libraries/default/Terrain/Rocks/basalt-cliff-face.stl',
  createdAt: '2026-09-02T09:14:00Z',
  updatedAt: '2026-09-02T09:14:00Z',
  removedAt: null,
}

const NEAR: RelatedTags = {
  tag: 'dragon',
  parts: 12,
  floor: 2,
  related: [
    { value: '28 mm', count: 11 },
    { value: 'pre-supported', count: 7 },
  ],
}

/**
 * The three reads this page makes, and nothing else. Everything unrecognised hangs rather than
 * resolving, so no panel here ever asserts against a body it did not ask for — the rule
 * `index.test.tsx`'s own dispatcher is built on.
 */
function stub(
  over: {
    related?: RelatedTags | 'fails'
    cards?: PartCard[]
    /** The library's tags as the facet panel sees them. `[]` is a library nobody has tagged. */
    facetTags?: { value: string; count: number }[]
    filters?: SavedFilter[]
  } = {},
) {
  const asked: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      asked.push(url)
      const ok = (body: unknown) => ({ ok: true, status: 200, json: async () => body })
      // Before the bare `/tags` rule: a library route's own path, with the tag as a parameter.
      if (url.includes('/tags/related')) {
        return over.related === 'fails'
          ? { ok: false, status: 500, json: async () => ({ message: 'no' }) }
          : ok(over.related ?? NEAR)
      }
      if (url.includes('/facets')) {
        return ok({
          formats: [{ value: 'stl', count: 12 }],
          materials: [],
          tags:
            over.facetTags ??
            [
              { value: 'dragon', count: 12 },
              { value: 'terrain', count: 9 },
            ],
          fields: [],
        })
      }
      if (url.includes('/parts')) return ok({ parts: over.cards ?? [BASALT], next: null })
      if (url.endsWith('/tags')) return ok({ tags: [{ value: 'dragon', count: 12 }] })
      if (url.includes('/filters')) return ok(over.filters ?? [])
      if (url.endsWith('/folders') || url.endsWith('/fields')) return ok([])
      return new Promise(() => {})
    }),
  )
  return asked
}

function renderAt(path: string) {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
  })
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
  return router
}

afterEach(() => vi.unstubAllGlobals())

test('the page is titled by its tag, and the grid asks for that tag', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  const asked = stub()
  renderAt('/tags/dragon')

  await screen.findByRole('article', { name: BASALT.name })
  expect(document.title).toBe(strings.tagIndex.tagTitle('dragon'))
  // The heading above the grid is the tag, where the library's own page says the category.
  expect(screen.getByRole('heading', { name: 'dragon', level: 2 })).toBeTruthy()
  // **The same grid, the same parameter.** Not a second endpoint and not a second component.
  expect(asked.some((url) => url.includes('/parts?') && url.includes('tag=dragon'))).toBe(true)
  expect(asked.some((url) => url.includes('/tags/related?tag=dragon'))).toBe(true)
})

test('the document holds one title, not the grid’s as well as the tag’s', async () => {
  // Emptied rather than seeded with `index.html`'s fallback, which the tests above need and this one
  // would count: the head is one document across a file's tests.
  document.head.innerHTML = ''
  stub()
  renderAt('/tags/dragon')
  await screen.findByRole('article', { name: BASALT.name })
  // `titled={false}` is the whole rule: React hoists every `<title>` it is given, and `document.title`
  // taking the first in tree order is not a reason to render two.
  expect(document.head.querySelectorAll('title')).toHaveLength(1)
})

test('related tags are listed with the floor they earned, and each one is a place', async () => {
  stub()
  renderAt('/tags/dragon')
  // Waited for by its own sentence: the facet panel's lists render first, so any `role="list"` would
  // resolve before this panel's request had landed.
  expect(await screen.findByText(strings.tagIndex.counted(12))).toBeTruthy()
  // The floor is the server's number, printed rather than restated in the copy.
  expect(screen.getByText(strings.tagIndex.relatedNote(2))).toBeTruthy()
  const chip = screen.getByRole('link', { name: `28 mm ${strings.tagIndex.shared(11)}` })
  expect(chip.getAttribute('href')).toBe('/tags/28%20mm')
})

test('a tag on no live model says so, rather than leaving an empty grid to explain itself', async () => {
  stub({ related: { tag: 'greebles', parts: 0, floor: 2, related: [] }, cards: [] })
  renderAt('/tags/greebles')
  expect(await screen.findByText(strings.tagIndex.gone('greebles'))).toBeTruthy()
})

test('nothing shared enough models is its own sentence, not an empty panel', async () => {
  stub({ related: { tag: 'dragon', parts: 12, floor: 2, related: [] } })
  renderAt('/tags/dragon')
  expect(await screen.findByText(strings.tagIndex.relatedNone)).toBeTruthy()
})

test('the neighbours failing leaves the models alone', async () => {
  stub({ related: 'fails' })
  renderAt('/tags/dragon')
  await screen.findByRole('article', { name: BASALT.name })
  expect(screen.getByText(strings.tagIndex.relatedFailed)).toBeTruthy()
})

test('a tag with a space, punctuation and a slash survives the round trip through the URL', async () => {
  for (const [encoded, tag] of [
    ['/tags/28%20mm', '28 mm'],
    ['/tags/pre-supported%20%26%20primed', 'pre-supported & primed'],
    ['/tags/d%26d%205e', 'd&d 5e'],
    ['/tags/jig%2Ffixture', 'jig/fixture'],
    ['/tags/100%25%20infill', '100% infill'],
  ] as const) {
    const asked = stub({ related: { tag, parts: 2, floor: 2, related: [] } })
    renderAt(encoded)
    // The tag the page is about, read back from the encoded path. The heading is the assertion that
    // matters: `%2F` has to arrive as one segment holding a slash and not as two segments, and a
    // decoded pathname is what the history hands back either way.
    await screen.findByRole('heading', { name: tag, level: 2 })
    // `URLSearchParams`, not `encodeURIComponent`: a query string spells a space `+`, and both reads
    // are built with the former, so the expectation has to be too.
    const spelt = new URLSearchParams({ tag }).toString()
    expect(asked.some((url) => url.includes(spelt)), `${tag} as ${spelt} in ${asked}`).toBe(true)
    vi.unstubAllGlobals()
    document.body.innerHTML = ''
  }
})

test('choosing a format on a tag page keeps the tag, and lands back on the tag page', async () => {
  stub()
  const router = renderAt('/tags/dragon')
  fireEvent.click(await screen.findByRole('button', { name: strings.facets.option('stl', 12) }))
  await waitFor(() => expect(router.state.location.pathname).toBe('/tags/dragon'))
  expect(router.state.location.searchStr).toBe('?format=stl')
})

test('clearing the tag comes back to the grid, still carrying the rest of the search', async () => {
  stub()
  const router = renderAt('/tags/dragon?format=stl')
  // The chosen tag is the pressed row in the facet panel; pressing it again is "no tag".
  fireEvent.click(await screen.findByRole('button', { name: strings.facets.tagOption('dragon', 12) }))
  await waitFor(() => expect(router.state.location.pathname).toBe('/'))
  expect(router.state.location.searchStr).toBe('?format=stl')
})

test('choosing a tag on the grid goes to its page and takes the search with it', async () => {
  stub()
  const router = renderAt('/?format=stl')
  fireEvent.click(await screen.findByRole('button', { name: strings.facets.tagOption('terrain', 9) }))
  await waitFor(() => expect(router.state.location.pathname).toBe('/tags/terrain'))
  expect(router.state.location.searchStr).toBe('?format=stl')
})

test('the filter panel offers the whole library’s tags, once there is one', async () => {
  stub()
  renderAt('/tags/dragon')
  const all = await screen.findByRole('link', { name: strings.tagIndex.all })
  expect(all.getAttribute('href')).toBe('/tags')
})

test('a second library rides on the way into the index and out to a neighbour', async () => {
  const OTHER = '01952c40-0000-7000-8000-0000000000f2'
  stub()
  renderAt(`/tags/dragon?library=${OTHER}`)
  const all = await screen.findByRole('link', { name: strings.tagIndex.all })
  expect(all.getAttribute('href')).toBe(`/tags?library=${OTHER}`)
  const chip = screen.getByRole('link', { name: `28 mm ${strings.tagIndex.shared(11)}` })
  expect(chip.getAttribute('href')).toBe(`/tags/28%20mm?library=${OTHER}`)
  expect(within(screen.getByRole('main')).getAllByRole('link').length).toBeGreaterThan(0)
})

test('a library nobody has tagged is offered no way into an empty index', async () => {
  stub({ facetTags: [] })
  renderAt('/tags/dragon')
  await screen.findByRole('article', { name: BASALT.name })
  expect(screen.queryByRole('link', { name: strings.tagIndex.all })).toBeNull()
})

test('a saved filter carrying a tag lands on that tag’s page, with the tag in the path only', async () => {
  stub({
    filters: [
      {
        id: '01952c40-0000-7000-8000-0000000000a1',
        name: 'Dragons, pre-supported',
        search: { tag: 'pre-supported', format: 'stl' },
        folderGone: false,
      },
    ],
  })
  const router = renderAt('/tags/dragon')
  fireEvent.click(await screen.findByRole('button', { name: 'Dragons, pre-supported' }))
  await waitFor(() => expect(router.state.location.pathname).toBe('/tags/pre-supported'))
  // Not `?tag=`: the old tag in the path and the new one in the search would be two tags and one grid.
  expect(router.state.location.searchStr).toBe('?format=stl')
})
