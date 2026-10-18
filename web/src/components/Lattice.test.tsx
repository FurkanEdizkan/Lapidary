import { render } from '@testing-library/react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, expect, test, vi } from 'vitest'
import Lattice from './Lattice'
import { lattice } from '../lib/lattice'

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
