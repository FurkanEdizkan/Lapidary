import { render, screen } from '@testing-library/react'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { act, useState } from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { breakable } from './Card'
import { Grid } from './Grid'
import type { PartCard, PartId } from '../lib/types'

const arrive = vi.hoisted(() => vi.fn((_elements: readonly HTMLElement[]) => () => {}))
vi.mock('../lib/motion', () => ({ arrive }))

afterEach(() => arrive.mockClear())

function card(id: string, name: string): PartCard {
  return {
    id: id as PartId,
    name,
    partNumber: null,
    thumbnail: null,
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
