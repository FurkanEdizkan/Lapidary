import { animate } from 'animejs/animation'
import { cubicBezier } from 'animejs/easings'
import { createDrawable } from 'animejs/svg'
import { stagger } from 'animejs/utils'
import { waapi } from 'animejs/waapi'
import { STAGGERED, bezier, reduced, staggerStep, tokens, type Tokens } from './motion-tokens'

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

export { STAGGERED, bezier, durationMs, reduced, staggerStep, tokens, type Tokens } from './motion-tokens'

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
 * The system's one curve as a function of progress, for a script that draws its own frames (the
 * turntable's step back). Same parsing as `tween`, so the curve still has one definition.
 */
export function curve(t: Tokens = tokens()): (progress: number) => number {
  return cubicBezier(...bezier(t))
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
  const animation = animate(target, {
    ...to,
    duration: t[duration],
    ease: curve(t),
    onUpdate,
  })
  return () => {
    animation.cancel()
  }
}

/**
 * Lines drawing themselves, in turn: the lattice's arrival (A19, A20).
 *
 * An authored moment, so it may run past `--duration-slow`: each line takes `--duration-scene` and the run is spread
 * over `total` ms (at most 3 s, so it is still within WCAG 2.2.2's five seconds once it settles). It draws once and
 * stays drawn; nothing here loops. Under reduced motion the lines are simply there. `onDone` runs either way, once.
 */
export function drawIn(
  lines: readonly SVGGeometryElement[],
  onDone: () => void = noop,
  total = 1800,
): () => void {
  const targets = lines.filter((line) => typeof line.getTotalLength === 'function')
  if (targets.length === 0 || reduced()) {
    onDone()
    return noop
  }
  const t = tokens()
  const spread = Math.max(0, Math.min(total, 3000) - t.scene)
  const drawables = createDrawable(targets as SVGGeometryElement[])
  const animation = animate(drawables, {
    draw: ['0 0', '0 1'],
    duration: t.scene,
    ease: curve(t),
    delay: stagger(targets.length > 1 ? spread / (targets.length - 1) : 0),
    onComplete: () => onDone(),
  })
  return () => {
    animation.revert()
  }
}
