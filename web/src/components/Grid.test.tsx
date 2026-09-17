import { fireEvent, render, screen } from '@testing-library/react'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { act, useState } from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { breakable } from './Card'
import { Grid } from './Grid'
import type { PartCard, PartId } from '../lib/types'

const { arrive, reduced, spin, stop } = vi.hoisted(() => {
  const stop = vi.fn()
  return {
    arrive: vi.fn((_elements: readonly HTMLElement[]) => () => {}),
    reduced: vi.fn(() => false),
    spin: vi.fn((_well: HTMLElement, _hash: string) => stop),
    stop,
  }
})
vi.mock('../lib/motion', () => ({ arrive, reduced }))
vi.mock('./turntable', () => ({ spin }))
vi.mock('../lib/viewer-math', async (original) => ({
  ...(await original<typeof import('../lib/viewer-math')>()),
  hasWebGL: () => true,
}))

afterEach(() => {
  arrive.mockClear()
  spin.mockClear()
  stop.mockClear()
  reduced.mockReturnValue(false)
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

function card(id: string, name: string): PartCard {
  return {
    id: id as PartId,
    name,
    partNumber: null,
    thumbnail: null,
    tessellationL0: `b3${id.slice(-4)}`,
    triangleCount: 784,
    approximate: true,
    directory: 'flanges',
  } as unknown as PartCard
}

const FLANGE = card('01931b6e-0000-7000-8000-0000000b0001', 'flange-dn40-lp-3310-02')
const GEAR = card('01931b6e-0000-7000-8000-0000000b0002', 'spur-gear-m2-20t-lp-5140-00')
const VEE = card('01931b6e-0000-7000-8000-0000000b0003', 'vee-block-lp-3072-02')

let setParts: (parts: PartCard[]) => void = () => {}

function Harness() {
  const [parts, set] = useState<PartCard[]>([FLANGE, GEAR])
  setParts = set
  return (
    <Grid
      parts={parts}
      onRender={() => {}}
      hostRoot={null}
      density="comfortable"
      layout="detail"
      selecting={false}
      selected={new Set()}
      onToggle={() => {}}
      onSelectAll={() => {}}
      onOpen={() => {}}
      onHover={() => {}}
      spins
    />
  )
}

async function renderGrid() {
  const router = createRouter({
    routeTree: createRootRoute({ component: Harness }),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  render(<RouterProvider router={router as never} />)
  await screen.findByRole('article', { name: FLANGE.name })
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

/** A screen that can hover, for the pointer's half of these tests. */
function hoverable() {
  vi.stubGlobal(
    'matchMedia',
    vi.fn((query: string) => ({ matches: query === '(hover: hover)' })),
  )
}

/** Past the intent delay, and past the dynamic import of the turntable's chunk. */
async function wait(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms)
  })
  await act(async () => {
    await vi.dynamicImportSettled()
  })
}

test('a card turns once the pointer has rested on it, and stops when it leaves', async () => {
  hoverable()
  await renderGrid()
  vi.useFakeTimers()
  const article = screen.getByRole('article', { name: GEAR.name })

  fireEvent.mouseEnter(article)
  await wait(149)
  expect(spin).not.toHaveBeenCalled()

  await wait(1)
  expect(spin).toHaveBeenCalledTimes(1)
  expect(spin.mock.calls[0]![1]).toBe(GEAR.tessellationL0)
  expect(article.contains(spin.mock.calls[0]![0])).toBe(true)

  fireEvent.mouseLeave(article)
  expect(stop).toHaveBeenCalledTimes(1)
})

test('a pointer sweeping across a card does not start it', async () => {
  hoverable()
  await renderGrid()
  vi.useFakeTimers()
  const article = screen.getByRole('article', { name: FLANGE.name })
  fireEvent.mouseEnter(article)
  await wait(80)
  fireEvent.mouseLeave(article)
  await wait(200)
  expect(spin).not.toHaveBeenCalled()
})

/** The keyboard path to a turning part: focus on the card's name, which is its one tab stop. */
test('focus on the name turns the part, and moving on stops it', async () => {
  hoverable()
  await renderGrid()
  vi.useFakeTimers()
  const link = screen.getByRole('link', { name: FLANGE.name })
  fireEvent.focus(link)
  await wait(150)
  expect(spin).toHaveBeenCalledTimes(1)
  fireEvent.blur(link)
  expect(stop).toHaveBeenCalledTimes(1)
})

test('a reader who asked for less motion gets the still picture', async () => {
  hoverable()
  reduced.mockReturnValue(true)
  await renderGrid()
  vi.useFakeTimers()
  fireEvent.mouseEnter(screen.getByRole('article', { name: GEAR.name }))
  await wait(300)
  expect(spin).not.toHaveBeenCalled()
})

/** A tap on a touch screen fires `mouseenter` on its way to opening the part. */
test('a screen that cannot hover does not turn a tapped card', async () => {
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })))
  await renderGrid()
  vi.useFakeTimers()
  fireEvent.mouseEnter(screen.getByRole('article', { name: GEAR.name }))
  await wait(300)
  expect(spin).not.toHaveBeenCalled()
})
