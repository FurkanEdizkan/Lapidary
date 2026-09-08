import { afterEach, expect, test, vi } from 'vitest'
import { flipFrom } from './flip'

/**
 * `flipFrom` is the only motion in the application that is written in JavaScript rather than
 * in CSS, so it is the only motion that can throw. Every branch here is a decline, and a
 * decline has to be silent and correct: the element is already where it belongs, so refusing
 * to animate leaves a static, right answer rather than a missing one.
 */

/** An element with a controllable rectangle and a recording `animate`. */
function elementAt(rect: Partial<DOMRect>) {
  const el = document.createElement('div')
  const calls: { keyframes: unknown; options: unknown }[] = []
  Object.defineProperty(el, 'getBoundingClientRect', {
    value: () => ({ left: 0, top: 0, width: 100, height: 100, ...rect }) as DOMRect,
  })
  Object.defineProperty(el, 'animate', {
    value: (keyframes: unknown, options: unknown) => {
      calls.push({ keyframes, options })
      return {} as Animation
    },
    configurable: true,
  })
  return { el, calls }
}

function rect(r: Partial<DOMRect>): DOMRect {
  return { left: 0, top: 0, width: 100, height: 100, ...r } as DOMRect
}

function reducedMotion(matches: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

test('a render that moved flies from where it was', () => {
  reducedMotion(false)
  const { el, calls } = elementAt({ left: 400, top: 300, width: 160, height: 160 })

  expect(flipFrom(el, rect({ left: 100, top: 200, width: 80, height: 80 }))).not.toBeNull()
  expect(calls).toHaveLength(1)

  // The first keyframe carries the whole animation: the element is already laid out at its
  // destination, so the flight runs backwards from the tile's rectangle to none.
  const [first, last] = calls[0]!.keyframes as { transform: string }[]
  // 100 − 400 and 200 − 300; 80 ÷ 160 both ways.
  expect(first!.transform).toBe('translate(-300px, -100px) scale(0.5, 0.5)')
  expect(last!.transform).toBe('none')
})

/**
 * The reduced-motion path removes the movement and keeps the outcome. There is no gentler
 * version of a 300px flight worth keeping — the panel is already open and already correct.
 */
test('a reader who asked for less motion gets none of this', () => {
  reducedMotion(true)
  const { el, calls } = elementAt({ left: 400, top: 300 })

  expect(flipFrom(el, rect({ left: 0, top: 0 }))).toBeNull()
  expect(calls).toHaveLength(0)
})

/**
 * A part the worker has not rasterized yet has no `<img>` to measure, so the caller passes a
 * zero-area rectangle. Scaling from zero reads as the panel bursting out of a point, which is
 * a worse answer than opening plainly.
 */
test('a tile with no render yet opens without a flight', () => {
  reducedMotion(false)
  const { el, calls } = elementAt({ left: 400, top: 300 })

  expect(flipFrom(el, rect({ width: 0, height: 0 }))).toBeNull()
  expect(calls).toHaveLength(0)
})

/** Animating a zero-distance move is a 280ms pause with nothing to look at. */
test('a render that did not move is not animated', () => {
  reducedMotion(false)
  const { el, calls } = elementAt({ left: 400, top: 300, width: 160, height: 160 })

  expect(flipFrom(el, rect({ left: 400, top: 300, width: 160, height: 160 }))).toBeNull()
  expect(calls).toHaveLength(0)
})

/**
 * The Web Animations API is the one thing here a runtime can lack. This runs inside a layout
 * effect the panel's render depends on, so a missing `animate` must decline rather than throw.
 */
test('a runtime without the animations API degrades instead of throwing', () => {
  reducedMotion(false)
  const el = document.createElement('div')
  Object.defineProperty(el, 'getBoundingClientRect', {
    value: () => rect({ left: 400, top: 300 }),
  })
  // jsdom is exactly this runtime, which is why every other test here defines `animate`.
  expect((el as { animate?: unknown }).animate).toBeUndefined()

  expect(() => flipFrom(el, rect({ left: 0, top: 0 }))).not.toThrow()
  expect(flipFrom(el, rect({ left: 0, top: 0 }))).toBeNull()
})
