import { animate } from 'animejs/animation'
import { cubicBezier } from 'animejs/easings'
import { waapi } from 'animejs/waapi'

/**
 * The one place motion is sequenced in script.
 *
 * CSS already moves everything that changes state: hovers, menus, the panel's arrival. What
 * it cannot say is "these twelve cards, one after another, and the whole run done inside
 * 280 ms", or "this number on a three.js object, over 180 ms". anime.js does both, and this
 * module is its only importer, so the durations, the one curve and reduced motion are
 * enforced in one file instead of at every call site.
 *
 * DOM elements go through anime's Web Animations adapter, never its JS engine. The engine
 * writes inline styles every frame, and the global `@layer base` transition in `styles.css`
 * would smear each write over 180 ms. A Web Animation is not a style change, so no transition
 * ever sees it.
 */

/** The motion tokens, as numbers a script can use. */
export type Tokens = { fast: number; base: number; slow: number; ease: string }

const FALLBACK: Tokens = { fast: 120, base: 180, slow: 280, ease: 'cubic-bezier(0.2, 0, 0, 1)' }

/** `120ms` → 120, `0.12s` → 120, anything unreadable → `fallback`. */
export function durationMs(value: string, fallback: number): number {
  const trimmed = value.trim()
  const number = Number.parseFloat(trimmed)
  if (!Number.isFinite(number)) return fallback
  return trimmed.endsWith('ms') ? number : trimmed.endsWith('s') ? number * 1000 : number
}

/** Read from `styles.css` rather than restated here, so a token edit reaches script too. */
export function tokens(): Tokens {
  if (typeof getComputedStyle !== 'function') return FALLBACK
  const root = getComputedStyle(document.documentElement)
  return {
    fast: durationMs(root.getPropertyValue('--duration-fast'), FALLBACK.fast),
    base: durationMs(root.getPropertyValue('--duration-base'), FALLBACK.base),
    slow: durationMs(root.getPropertyValue('--duration-slow'), FALLBACK.slow),
    ease: root.getPropertyValue('--ease-mechanical').trim() || FALLBACK.ease,
  }
}

/**
 * The reader asked for less motion, or the runtime cannot say whether they did.
 *
 * `styles.css` cuts CSS transitions under the media query, but that rule never reaches a
 * script, so each script-driven move asks here. No `matchMedia` counts as asking: that is
 * the test renderer, and a jump to the end state is the answer every test wants anyway.
 */
export function reduced(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return true
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/** How many arrivals take a turn. Anything after the twelfth lands with the twelfth. */
export const STAGGERED = 12

/**
 * The gap between two arrivals: 24 ms at most, and less when there are many, so the last
 * one still finishes inside `--duration-slow`. With twelve cards and 120 ms moves that is
 * (280 − 120) / 11 ≈ 14.5 ms.
 */
export function staggerStep(count: number, t: Tokens = FALLBACK): number {
  const turns = Math.min(count, STAGGERED) - 1
  if (turns <= 0) return 0
  return Math.min(24, (t.slow - t.fast) / turns)
}

const noop = () => {}

/**
 * New things coming into view: fade up four pixels, in turn.
 *
 * For content that was just added (a first page, the next page, a new filter's first page),
 * never for a refetch of what is already on screen. Returns a cancel function.
 *
 * On completion the animation is reverted rather than committed. anime commits a finished
 * Web Animation's end values inline, and an inline `translate: 0 0` would outrank every
 * `hover:-translate-y-*` class on the card it arrived.
 */
export function arrive(elements: readonly HTMLElement[]): () => void {
  const targets = elements.filter((el) => typeof el.animate === 'function')
  if (targets.length === 0 || reduced()) return noop
  const t = tokens()
  const step = staggerStep(targets.length, t)
  const animation = waapi.animate(targets, {
    opacity: [0, 1],
    translate: ['0 4px', '0 0'],
    duration: t.fast,
    ease: t.ease,
    delay: (_target, index) => Math.min(index, STAGGERED - 1) * step,
    onComplete: (self) => {
      self.revert()
    },
  })
  return () => {
    animation.revert()
  }
}

/**
 * Numbers on an object that is not an element: a callout's draw-in, a light's intensity.
 *
 * Runs on anime's own engine, which is right here because nothing it writes is a style.
 * Under reduced motion the object takes its end values at once and `onUpdate` runs once, so
 * the state change still happens and only the movement is gone.
 */
export function tween<T extends object>(
  target: T,
  to: Partial<Record<keyof T, number>>,
  onUpdate: () => void,
  duration: keyof Omit<Tokens, 'ease'> = 'base',
): () => void {
  if (reduced()) {
    Object.assign(target, to)
    onUpdate()
    return noop
  }
  const t = tokens()
  const [x1, y1, x2, y2] = (t.ease.match(/-?[\d.]+/g) ?? []).map(Number)
  const animation = animate(target, {
    ...to,
    duration: t[duration],
    ease: cubicBezier(x1 ?? 0.2, y1 ?? 0, x2 ?? 0, y2 ?? 1),
    onUpdate,
  })
  return () => {
    animation.cancel()
  }
}
