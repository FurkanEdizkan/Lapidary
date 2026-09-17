import { afterEach, expect, test, vi } from 'vitest'
import { STAGGERED, arrive, durationMs, staggerStep, tween } from './motion'

function reducedMotion(matches: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

/** An element whose `animate` records what it was asked and hands back enough of an Animation. */
function recording() {
  const el = document.createElement('div')
  const calls: { keyframes: Record<string, unknown>; options: KeyframeAnimationOptions }[] = []
  Object.defineProperty(el, 'animate', {
    value: (keyframes: Record<string, unknown>, options: KeyframeAnimationOptions) => {
      calls.push({ keyframes, options })
      return { pause() {}, cancel() {}, commitStyles() {}, play() {}, playbackRate: 1 } as unknown as Animation
    },
  })
  return { el, calls }
}

test('a token reads the same written in milliseconds or seconds', () => {
  expect(durationMs('120ms', 0)).toBe(120)
  expect(durationMs(' 0.28s', 0)).toBe(280)
  expect(durationMs('', 180)).toBe(180)
})

/**
 * The stagger's promise is that the whole arrival, however many cards, is over inside
 * `--duration-slow`. Checked for every count a page can hold rather than for the one case
 * the formula was worked out on.
 */
test('an arrival of any size finishes inside the slow duration', () => {
  const t = { fast: 120, base: 180, slow: 280, ease: '' }
  expect(staggerStep(1, t)).toBe(0)
  expect(staggerStep(5, t)).toBe(24)
  for (let count = 1; count <= 200; count++) {
    const lastStart = (Math.min(count, STAGGERED) - 1) * staggerStep(count, t)
    expect(lastStart + t.fast).toBeLessThanOrEqual(t.slow + 1e-9)
  }
})

test('cards arrive in turn, and the thirteenth lands with the twelfth', () => {
  reducedMotion(false)
  const cards = Array.from({ length: 14 }, recording)
  arrive(cards.map((c) => c.el))

  const delays = cards.map((c) => c.calls[0]!.options.delay)
  const step = staggerStep(14)
  expect(delays[0]).toBe(0)
  expect(delays[1]).toBeCloseTo(step)
  expect(delays[11]).toBeCloseTo(11 * step)
  expect(delays[13]).toBe(delays[11])
  // Opacity and the independent `translate`, never `transform`: the card's hover lift is a
  // `translate` class and the flight in the quick look owns `transform`.
  expect(cards[0]!.calls.map((c) => Object.keys(c.keyframes)[0]).sort()).toEqual(['opacity', 'translate'])
})

test('a reader who asked for less motion sees the cards arrive without moving', () => {
  reducedMotion(true)
  const card = recording()
  arrive([card.el])
  expect(card.calls).toHaveLength(0)
})

test('a tween under reduced motion lands on its end values at once', () => {
  reducedMotion(true)
  const callout = { reveal: 0, other: 3 }
  const onUpdate = vi.fn()
  tween(callout, { reveal: 1 }, onUpdate)
  expect(callout).toEqual({ reveal: 1, other: 3 })
  expect(onUpdate).toHaveBeenCalledTimes(1)
})

/**
 * CLAUDE.md: only `lib/motion.ts` imports anime.js. A second importer would bring its own
 * durations and its own idea of reduced motion, which is what this module exists to prevent.
 */
test('motion.ts is the only importer of anime.js', () => {
  const sources = import.meta.glob('../**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<
    string,
    string
  >
  const importers = Object.entries(sources)
    .filter(([path, text]) => !path.endsWith('.test.ts') && /from\s+['"]animejs/.test(text))
    .map(([path]) => path)
  // The glob has to reach the components and routes, or an empty scan would pass.
  expect(Object.keys(sources)).toContain('../routes/index.tsx')
  expect(importers).toEqual(['./motion.ts'])
})
