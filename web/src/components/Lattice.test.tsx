import { fireEvent, render, screen } from '@testing-library/react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, expect, test, vi } from 'vitest'
import Lattice from './Lattice'
import { lattice } from '../lib/lattice'
import { strings } from '../lib/strings'

afterEach(() => {
  vi.unstubAllGlobals()
})

/** Anything stroked in the accent: the draw head, which must never be there at rest (DESIGN.md § Lattice). */
function accent(container: HTMLElement): Element[] {
  return Array.from(container.querySelectorAll('[stroke]')).filter((el) => el.getAttribute('stroke')?.includes('accent'))
}

test('under reduced motion the gear is simply drawn, every edge grey, and decorative', () => {
  // The test renderer has no matchMedia, which `reduced()` reads as reduced motion.
  const { container } = render(<Lattice />)
  const svg = container.querySelector('svg')!
  expect(svg.getAttribute('aria-hidden')).toBe('true')
  expect(svg.querySelectorAll('path')).toHaveLength(lattice().edges.length)
  expect(accent(container)).toEqual([])
})

/**
 * The first frame, before any effect: what a reader sees before the drawing starts. Rendered to markup because in
 * jsdom a path has no length, so `drawIn` finishes at once and would hide a head that should never have been there.
 */
function firstFrame(element: React.ReactElement, motion: boolean): string {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: !motion })),
  )
  return renderToStaticMarkup(element)
}

test('the draw head exists only on a lattice that is about to draw', () => {
  expect(firstFrame(<Lattice />, true)).toContain('var(--color-accent)')
  expect(firstFrame(<Lattice still />, true)).not.toContain('var(--color-accent)')
  expect(firstFrame(<Lattice />, false)).not.toContain('var(--color-accent)')
})

/** A26: sign-in's drawing keeps turning once drawn, so it carries a Pause (WCAG 2.2.2); nothing else does. */
test('a live drawing can be paused and played, and only a live one offers to', async () => {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: false })),
  )
  render(<Lattice live />)
  // In jsdom a path has no length, so the drawing is done at once and the turn begins.
  const pause = await screen.findByRole('button', { name: strings.doorway.pause })
  expect(pause.getAttribute('aria-pressed')).toBe('false')
  fireEvent.click(pause)
  const play = screen.getByRole('button', { name: strings.doorway.play })
  expect(play.getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(play)
  expect(screen.getByRole('button', { name: strings.doorway.pause })).toBeTruthy()
})

test('nothing offers a pause where nothing loops', () => {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: false })),
  )
  render(<Lattice />)
  render(<Lattice still />)
  expect(screen.queryByRole('button')).toBeNull()
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: true })),
  )
  render(<Lattice live />)
  expect(screen.queryByRole('button')).toBeNull()
})
