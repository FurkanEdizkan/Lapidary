import { render, screen, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRouter } from '@tanstack/react-router'
import { afterEach, expect, test, vi } from 'vitest'
import { routeTree } from '../routeTree.gen'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import { LETTER_JUMP_AT } from '../lib/tags'
import type { TagCount } from '../lib/types'

/**
 * `/tags` through the real route tree, because half of what this page is about is addresses: the
 * order is a link, every tag is a link, and a tag with a space or a slash has to come back out of the
 * URL as the tag that went in. A synthetic tree would resolve those hrefs against routes this test
 * wrote, which proves nothing about the ones the application ships.
 *
 * `routeTree.gen.ts` imports its routes plainly and `vitest.config.ts` loads no router plugin, so the
 * generated tree renders here as it ships.
 */
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

/** The tag index answers; nothing else on this page asks for anything. */
function stub(tags: TagCount[] | 'fails') {
  const asked: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      asked.push(url)
      if (url.includes('/tags')) {
        return tags === 'fails'
          ? { ok: false, status: 500, json: async () => ({ message: 'no' }) }
          : { ok: true, status: 200, json: async () => ({ tags }) }
      }
      // Anything else hangs rather than resolving, so nothing here asserts against a body it did
      // not ask for. This page asks for one thing.
      return new Promise(() => {})
    }),
  )
  return asked
}

const CORPUS: TagCount[] = [
  { value: '28 mm', count: 41 },
  { value: 'dragon', count: 12 },
  { value: 'pre-supported', count: 12 },
  { value: 'terrain', count: 9 },
]

afterEach(() => vi.unstubAllGlobals())

test('the page is titled, and lists every tag with its count, most used first', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  const asked = stub(CORPUS)
  renderAt('/tags')

  expect(await screen.findByRole('heading', { name: strings.tagIndex.heading })).toBeTruthy()
  expect(document.title).toBe(strings.tagIndex.title)
  expect(asked).toEqual([`/api/libraries/${DEFAULT_LIBRARY_ID}/tags`])

  const rows = within(await screen.findByRole('list')).getAllByRole('link')
  expect(rows.map((row) => row.textContent)).toEqual([
    `28 mm ${strings.tagIndex.models(41)}`,
    `dragon ${strings.tagIndex.models(12)}`,
    `pre-supported ${strings.tagIndex.models(12)}`,
    `terrain ${strings.tagIndex.models(9)}`,
  ])
})

test('a tag with a space goes into the URL and comes back out as itself', async () => {
  stub(CORPUS)
  renderAt('/tags')
  // The one assertion P3 names about punctuation, made where the router builds the href rather than
  // where a test builds it: `28 mm` is a real tag, and `/tags/28 mm` is not a URL.
  const row = await screen.findByRole('link', { name: `28 mm ${strings.tagIndex.models(41)}` })
  expect(row.getAttribute('href')).toBe('/tags/28%20mm')
})

test('the two orders are addresses, and A to Z files the tags under letters', async () => {
  stub(CORPUS)
  renderAt('/tags')
  await screen.findByRole('list')
  const byName = screen.getByRole('link', { name: strings.tagIndex.byName })
  expect(byName.getAttribute('href')).toBe('/tags?order=name')
  expect(screen.getByRole('link', { name: strings.tagIndex.byCount }).getAttribute('href')).toBe(
    '/tags',
  )
  // Most used is the default, so it is the one marked, and it is marked without a parameter.
  // The router marks the current one; `aria-current="page"` is its word for it.
  expect(screen.getByRole('link', { name: strings.tagIndex.byCount }).getAttribute('aria-current')).toBe(
    'page',
  )
  expect(byName.getAttribute('aria-current')).toBeNull()
})

test('on the A-to-Z page it is A to Z that is marked, and only it', async () => {
  stub(CORPUS)
  renderAt('/tags?order=name')
  // **This is where the router's default goes wrong.** "Most used" carries no `order`, and an empty
  // search is a subset of `?order=name` under a partial match, so without `exact` both links read as
  // the current one — which is one more current link than a page has.
  const byName = await screen.findByRole('link', { name: strings.tagIndex.byName })
  expect(byName.getAttribute('aria-current')).toBe('page')
  expect(
    screen.getByRole('link', { name: strings.tagIndex.byCount }).getAttribute('aria-current'),
  ).toBeNull()
})

test('ordered by name, the groups are headed by letter and numbers share one heading', async () => {
  stub(CORPUS)
  renderAt('/tags?order=name')
  expect(await screen.findByRole('heading', { name: strings.tagIndex.otherInitial })).toBeTruthy()
  expect(screen.getByRole('heading', { name: 'D' })).toBeTruthy()
  expect(screen.getByRole('heading', { name: 'P' })).toBeTruthy()
  expect(screen.getByRole('heading', { name: 'T' })).toBeTruthy()
  // Four tags is not a long list, so there is no jump bar to skim past.
  expect(screen.queryByRole('navigation', { name: strings.tagIndex.jump })).toBeNull()
})

test('past the threshold the letter jump appears, with a link per group and no dead ones', async () => {
  // One tag per letter of the alphabet, twice over, which is what a creator library looks like.
  const many: TagCount[] = Array.from({ length: LETTER_JUMP_AT + 1 }, (_, n) => ({
    value: `${String.fromCharCode(97 + (n % 26))}-tag-${n}`,
    count: n + 1,
  }))
  stub(many)
  renderAt('/tags?order=name')
  const jump = await screen.findByRole('navigation', { name: strings.tagIndex.jump })
  const letters = within(jump).getAllByRole('link')
  expect(letters).toHaveLength(26)
  // Every jump link points at a heading that is on the page.
  for (const letter of letters) {
    const id = letter.getAttribute('href')?.slice(1) ?? ''
    expect(document.getElementById(id), id).toBeTruthy()
  }
})

test('a library nobody has tagged says so, and offers no order to put nothing in', async () => {
  stub([])
  renderAt('/tags')
  expect(await screen.findByText(strings.tagIndex.none)).toBeTruthy()
  expect(screen.queryByRole('link', { name: strings.tagIndex.byName })).toBeNull()
})

test('a read that fails says so where a reader is looking', async () => {
  stub('fails')
  renderAt('/tags')
  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toBe(strings.tagIndex.failed)
})

test('a second library is carried by every link on the page', async () => {
  const OTHER = '01952c40-0000-7000-8000-0000000000f2'
  const asked = stub(CORPUS)
  renderAt(`/tags?library=${OTHER}`)

  await screen.findByRole('list')
  expect(asked).toEqual([`/api/libraries/${OTHER}/tags`])
  expect(
    screen.getByRole('link', { name: `dragon ${strings.tagIndex.models(12)}` }).getAttribute('href'),
  ).toBe(`/tags/dragon?library=${OTHER}`)
  expect(screen.getByRole('link', { name: strings.tagIndex.byName }).getAttribute('href')).toBe(
    `/tags?library=${OTHER}&order=name`,
  )
})
