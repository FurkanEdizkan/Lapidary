import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { act, useState } from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { breakable } from './Card'
import { Grid } from './Grid'
import { strings } from '../lib/strings'
import type { PartCard, PartId, PartImage, PartImageId } from '../lib/types'

const { arrive, reduced } = vi.hoisted(() => ({
  arrive: vi.fn((_elements: readonly HTMLElement[]) => () => {}),
  reduced: vi.fn(() => false),
}))
vi.mock('../lib/motion', () => ({ arrive, reduced }))

afterEach(() => {
  arrive.mockClear()
  reduced.mockReturnValue(false)
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

/** A tiny real WebP, as `part_image.image_webp` arrives on the row: under the 64 KB line. */
const WEBP = 'data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvAUAAAAdQ1LrVv/+BiOh/AAA='

/** A rendered thumbnail, deliberately different bytes: the fallback the picture has to beat. */
const RENDER = 'data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBEAAAAvA0AAAAdQvJoXpf+BiOh/AAA='

/** One picture, framed the way `0019_image_framing.sql` stores a framing. */
function picture(src: string, framing: Partial<PartImage> = {}): PartImage {
  return {
    id: '01931b6e-0000-7000-8000-0000000c0001' as PartImageId,
    src,
    origin: 'uploaded',
    sourceUrl: null,
    fit: 'cover',
    focusX: 0.5,
    focusY: 0.5,
    ...framing,
  }
}

function card(id: string, name: string, extra: Partial<PartCard> = {}): PartCard {
  return {
    id: id as PartId,
    name,
    partNumber: null,
    sourceFormat: 'stl',
    image: null,
    imageCount: 0,
    triangleCount: 784,
    approximate: true,
    directory: 'flanges',
    ...extra,
  } as unknown as PartCard
}

const FLANGE = card('01931b6e-0000-7000-8000-0000000b0001', 'flange-dn40-lp-3310-02')
const GEAR = card('01931b6e-0000-7000-8000-0000000b0002', 'spur-gear-m2-20t-lp-5140-00')
const VEE = card('01931b6e-0000-7000-8000-0000000b0003', 'vee-block-lp-3072-02')

let setParts: (parts: PartCard[]) => void = () => {}

function Harness({ initial = [FLANGE, GEAR] }: { initial?: PartCard[] }) {
  const [parts, set] = useState<PartCard[]>(initial)
  setParts = set
  return (
    <Grid
      parts={parts}
      density="comfortable"
      layout="detail"
      selecting={false}
      selected={new Set()}
      onToggle={() => {}}
      onSelectAll={() => {}}
      onOpen={() => {}}
    />
  )
}

async function renderGrid(initial?: PartCard[]) {
  const first = (initial ?? [FLANGE, GEAR])[0]!
  const router = createRouter({
    routeTree: createRootRoute({ component: () => <Harness initial={initial} /> }),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
  await screen.findByRole('article', { name: first.name })
}

/**
 * A scan refetches the grid every time the worker finishes a file, and each refetch hands back
 * the same parts as new objects. The arrival belongs to a card's first appearance only, or the
 * grid would flicker for the length of the scan.
 */
test('cards arrive once: a refetch of the same parts does not replay it, a new part does', async () => {
  await renderGrid()
  expect(arrive).toHaveBeenCalledTimes(1)
  expect(arrive.mock.calls[0]![0]).toHaveLength(2)

  await act(async () => setParts([{ ...FLANGE }, { ...GEAR }]))
  expect(arrive).toHaveBeenCalledTimes(1)

  await act(async () => setParts([{ ...FLANGE }, { ...GEAR }, VEE]))
  expect(arrive).toHaveBeenCalledTimes(2)
  const [only] = arrive.mock.calls[1]![0]
  expect(only?.textContent).toContain(VEE.name)
})

/** A slug has no spaces, so without break opportunities it wraps mid-word. */
test('a name breaks after its separators and reads exactly as before', () => {
  const { container } = render(<span>{breakable('flange-dn40_lp.3310-02')}</span>)
  expect(container.querySelectorAll('wbr')).toHaveLength(4)
  expect(container.textContent).toBe('flange-dn40_lp.3310-02')
  expect(container.innerHTML).toBe('<span>flange-<wbr>dn40_<wbr>lp.<wbr>3310-<wbr>02</span>')
})

/**
 * **The card's picture, in the three states a row can be in** (R1).
 *
 * `part_image` keeps bytes on the row under 64 KB and a blob hash above it
 * (`part_image_inline_or_blob`), and the card has to show both without knowing which is which —
 * so the two are asserted on the same page, beside a part that has neither. Three cards together
 * rather than three tests, because the failure worth catching is the wiring one: a card reading
 * the *first* part's picture passes every single-card test there is.
 */
test('a card shows its own picture, inline or addressed, and names the format when it has none', async () => {
  const BLOB = '/api/blob/9f2c4a8e1d6b05734cfe82a0d1945b6e73c8af20e51d9b4c6a3f0827de15bc49'
  const inline = card('01931b6e-0000-7000-8000-0000000b0011', 'flange-dn40-lp-3310-02', {
    image: picture(WEBP),
    imageCount: 1,
  })
  const addressed = card('01931b6e-0000-7000-8000-0000000b0012', 'spur-gear-m2-20t-lp-5140-00', {
    image: picture(BLOB),
    imageCount: 1,
  })
  const bare = card('01931b6e-0000-7000-8000-0000000b0013', 'vee-block-lp-3072-02', {
    sourceFormat: '3mf',
  })
  // The render that used to sit behind the picture is gone (R3), so there is no precedence left
  // to get wrong: a card shows its image or names its format. This one carries a picture and the
  // format a bare card would name, so a card that fell back to the format would show.
  const both = card('01931b6e-0000-7000-8000-0000000b0015', 'hex-spacer-m4x20-lp-2145-01', {
    image: picture(WEBP),
    imageCount: 1,
    sourceFormat: 'stl',
  })
  const fetchMock = vi.fn(async () => ({ ok: true, status: 200, json: async () => ({}) }))
  vi.stubGlobal('fetch', fetchMock)
  await renderGrid([inline, addressed, bare, both])

  // **No request per card.** The picture arrives with the page, inline or as an address, and the
  // grid's own code asks for nothing — which is the rule a gallery of a thousand parts rests on.
  // jsdom does not load an `<img src>`, so this is about `fetch` and not about the browser's own
  // image request: the blob URL below is one cached, immutable image load, never an API round trip.
  expect(fetchMock).not.toHaveBeenCalled()

  const shown = (part: PartCard) =>
    screen.getByRole('article', { name: part.name }).querySelector('img')

  expect(shown(inline)?.getAttribute('src')).toBe(WEBP)
  // Over the inline line, so the card addresses the bytes rather than carrying them. One `<img>`
  // request, cached immutably by `blob.rs` — never a second API call for the card.
  expect(shown(addressed)?.getAttribute('src')).toBe(BLOB)
  expect(shown(bare)).toBeNull()
  expect(
    shown(both)?.getAttribute('src') === WEBP,
    'a card with a picture shows it rather than falling back to its format',
  ).toBe(true)

  // Alt text as a literal, not read back from strings.ts: an alt of '' satisfies the constant form
  // and tells a screen-reader user nothing.
  expect(shown(inline)?.getAttribute('alt')).toBe('Picture of flange-dn40-lp-3310-02')

  // And the empty state names the format rather than apologising for a render nobody asked for.
  const well = screen.getByRole('article', { name: bare.name })
  expect(well.textContent).toContain(strings.parts.noPicture('3mf'))
  expect(well.textContent).not.toContain(strings.parts.noPicture(null))
  expect(fetchMock).not.toHaveBeenCalled()
  vi.unstubAllGlobals()
})

/**
 * **A set flips on the card, and costs nothing until it does** (M5). The page carries the first
 * picture and the count; the arrows fetch the gallery once, and a card with one picture has none.
 */
test('a card with several pictures flips through them, fetching the set only when asked', async () => {
  const SECOND = '/api/blob/2b7e1c0a9f3d4e5b6a7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c'
  const set = card('01931b6e-0000-7000-8000-0000000b0016', 'go-cruise-throttle-stop', {
    image: picture(WEBP),
    imageCount: 2,
  })
  const single = card('01931b6e-0000-7000-8000-0000000b0017', 'hex-spacer-m4x20-lp-2145-01', {
    image: picture(WEBP),
    imageCount: 1,
  })
  const fetchMock = vi.fn(async (_url: string) => ({
    ok: true,
    status: 200,
    json: async () => [picture(WEBP), picture(SECOND)],
  }))
  vi.stubGlobal('fetch', fetchMock)
  await renderGrid([set, single])
  const tile = screen.getByRole('article', { name: set.name })
  expect(tile.textContent).toContain('1 of 2')
  expect(fetchMock).not.toHaveBeenCalled()
  expect(
    screen.getByRole('article', { name: single.name }).querySelector('button'),
    'one picture, no arrows',
  ).toBeNull()

  fireEvent.click(within(tile).getByRole('button', { name: strings.parts.nextPicture }))
  await waitFor(() => expect(tile.querySelector('img')?.getAttribute('src')).toBe(SECOND))
  expect(tile.textContent).toContain('2 of 2')
  expect(fetchMock).toHaveBeenCalledTimes(1)
  expect(String(fetchMock.mock.calls[0]?.[0])).toContain(`/api/parts/${set.id}/images`)

  fireEvent.click(within(tile).getByRole('button', { name: strings.parts.nextPicture }))
  expect(tile.querySelector('img')?.getAttribute('src')).toBe(WEBP)
  expect(fetchMock).toHaveBeenCalledTimes(1)
})

/**
 * `fit` and `focus_x`/`focus_y` are three columns the browser applies at display time — nothing is
 * re-encoded — so the only thing that can be wrong is whether they reach the element. Asserted on
 * the element's own style, and with a framing nobody would have reached by accident: `cover` at
 * 0.5/0.5 is the stored default, so asserting that would pass against a card that read no columns.
 */
test('a picture is framed the way its row says', async () => {
  const framed = card('01931b6e-0000-7000-8000-0000000b0014', 'flange-dn40-lp-3310-02', {
    image: picture(WEBP, { fit: 'contain', focusX: 0.25, focusY: 0.8 }),
    imageCount: 1,
  })
  await renderGrid([framed])
  const img = screen.getByRole('article', { name: framed.name }).querySelector('img')
  expect(img?.style.objectFit).toBe('contain')
  expect(img?.style.objectPosition).toBe('25% 80%')
})

/**
 * **Nothing draws, loads or fetches because a pointer crossed a card** (R1).
 *
 * A hover used to rest for 150 ms and then put a turning model in the card's well, which fetched
 * that part's mesh. Fifty cards hovered on the way down a grid were fifty fetches, and with the
 * viewer reading **original** files they would be fifty whole models. So there is no hover
 * behaviour left, at any setting.
 *
 * The assertions are deliberately about the *mechanisms*, not about three.js: in jsdom there is no
 * WebGL, so `new WebGLRenderer()` throws before it appends anything and "no canvas" alone would
 * pass against a card that tried. A `fetch` count and a dynamic-import count are what actually
 * fail when something starts loading again — this test was written by putting a canvas and a
 * `fetch` back into `onMouseEnter` and watching both halves fail.
 */
test('resting on a card, and focusing its name, start nothing at all', async () => {
  const fetchMock = vi.fn(async () => ({ ok: true, status: 200, json: async () => ({}) }))
  vi.stubGlobal('fetch', fetchMock)
  // A screen that *can* hover, which is the setting the spin needed; and reduced motion off, so
  // nothing here passes because a reader asked for less of it.
  vi.stubGlobal(
    'matchMedia',
    vi.fn((query: string) => ({ matches: query === '(hover: hover)' })),
  )
  await renderGrid()
  const article = screen.getByRole('article', { name: GEAR.name })

  fireEvent.mouseEnter(article)
  fireEvent.focus(screen.getByRole('link', { name: GEAR.name }))
  // Well past the 150 ms of hover intent the spin waited for, and past any dynamic import it
  // would have started.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 300))
    await vi.dynamicImportSettled()
  })

  expect(fetchMock).not.toHaveBeenCalled()
  expect(document.querySelector('canvas')).toBeNull()
  fireEvent.mouseLeave(article)
  expect(fetchMock).not.toHaveBeenCalled()
})

/**
 * A35: scrolled past the grid's last row (End, a dragged scrollbar), the window kept no rows, found no card to measure,
 * and drew every card loaded for a frame: at 8,000 cards each page took 1.9 s to land. Layout stubbed, since jsdom lays
 * nothing out: four columns of 300px rows, the grid 100px down the page.
 */
test('scrolled past the end, the grid still draws a window and never every card', async () => {
  const many = Array.from({ length: 2000 }, (_, i) =>
    card(`01931b6e-0000-7000-8000-${i.toString(16).padStart(12, '0')}`, `bracket-lp-${1000 + i}-03`),
  )
  let scrollY = 0
  Object.defineProperty(window, 'scrollY', { configurable: true, get: () => scrollY })
  const real = window.getComputedStyle
  vi.spyOn(window, 'getComputedStyle').mockImplementation((node) =>
    node instanceof HTMLUListElement
      ? ({ gridTemplateColumns: '200px 200px 200px 200px', rowGap: '0' } as CSSStyleDeclaration)
      : real(node),
  )
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(300)
  vi.spyOn(HTMLUListElement.prototype, 'getBoundingClientRect').mockImplementation(
    () => ({ top: 100 - scrollY }) as DOMRect,
  )
  await renderGrid(many)
  const list = screen.getByRole('list')
  let most = 0
  const watch = new MutationObserver(() => {
    most = Math.max(most, list.children.length)
  })
  watch.observe(list, { childList: true })
  expect(list.children.length).toBeLessThan(100)

  scrollY = 10_000_000
  await act(async () => {
    window.dispatchEvent(new Event('scroll'))
    await new Promise((resolve) => setTimeout(resolve, 50))
  })
  watch.disconnect()
  expect(list.children.length, 'the last rows are drawn').toBeGreaterThan(0)
  expect(list.children.length).toBeLessThan(100)
  expect(most, 'never every card, not even for a frame').toBeLessThan(100)
  vi.restoreAllMocks()
})
