/**
 * The motion tokens as numbers, and whether the reader asked for less motion.
 *
 * No engine here: both engines read these (anime.js through `motion.ts`, Motion through `motion-react.tsx`), and the
 * root's `MotionRoot` must not pull anime into the entry chunk (A18). `styles.css` is the one definition; this reads
 * it.
 */

/** The motion tokens, as numbers a script can use. */
export type Tokens = { fast: number; base: number; slow: number; scene: number; intro: number; ease: string }

const FALLBACK: Tokens = {
  fast: 120,
  base: 180,
  slow: 280,
  scene: 480,
  intro: 900,
  ease: 'cubic-bezier(0.2, 0, 0, 1)',
}

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
    // The front door's authored moments only (A17): the intro into setup and the lattice's scenes.
    scene: durationMs(root.getPropertyValue('--duration-scene'), FALLBACK.scene),
    intro: durationMs(root.getPropertyValue('--duration-intro'), FALLBACK.intro),
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

/** The one curve's four control points, from `--ease-mechanical`, for an engine that takes them as numbers. */
export function bezier(t: Tokens = tokens()): [number, number, number, number] {
  const [x1, y1, x2, y2] = (t.ease.match(/-?[\d.]+/g) ?? []).map(Number)
  return [x1 ?? 0.2, y1 ?? 0, x2 ?? 0, y2 ?? 1]
}
