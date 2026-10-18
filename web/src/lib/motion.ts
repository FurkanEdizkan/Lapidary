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
 *
 * `heads`, when given, are accent copies of `lines`, one for one: each rides the tip of its line as a short segment
 * and fades out before that line is finished, so the accent is only ever where something is being drawn. The caller
 * removes them on `onDone`; blue at rest is a defect. Under reduced motion they are never animated, so a caller
 * should not render them then.
 */
export function drawIn(
  lines: readonly SVGGeometryElement[],
  onDone: () => void = noop,
  total = 1800,
  heads: readonly SVGGeometryElement[] = [],
): () => void {
  const targets = lines.filter((line) => typeof line.getTotalLength === 'function')
  if (targets.length === 0 || reduced()) {
    onDone()
    return noop
  }
  const t = tokens()
  const spread = Math.max(0, Math.min(total, 3000) - t.scene)
  const step = targets.length > 1 ? spread / (targets.length - 1) : 0
  const drawables = createDrawable(targets as SVGGeometryElement[])
  const animation = animate(drawables, {
    draw: ['0 0', '0 1'],
    duration: t.scene,
    ease: curve(t),
    delay: stagger(step),
    onComplete: () => onDone(),
  })
  const tips = heads.filter((head) => typeof head.getTotalLength === 'function')
  if (tips.length === 0) {
    return () => {
      animation.revert()
    }
  }
  // The head is the last few percent of what is drawn so far, riding the line's tip, and it fades over the line's
  // last `--duration-fast`, so each line is grey the moment it is complete (DESIGN.md § Lattice).
  const head = animate(createDrawable(tips as SVGGeometryElement[]), {
    draw: ['0 0', '0.92 1'],
    duration: t.scene,
    ease: curve(t),
    delay: stagger(step),
  })
  const fade = waapi.animate(tips as unknown as HTMLElement[], {
    opacity: [1, 0],
    duration: t.fast,
    ease: t.ease,
    delay: stagger(step, { start: t.scene - t.fast }),
  })
  return () => {
    animation.revert()
    head.revert()
    fade.revert()
  }
}

/** A running cycle, and how to hold it: WCAG 2.2.2 wants anything that loops to be pausable. */
export type Cycle = { pause: () => void; play: () => void; cancel: () => void }

const STILL_CYCLE: Cycle = { pause: noop, play: noop, cancel: noop }

/**
 * A number running from 0 to 1 and round again, for ever, at an even pace: the sign-in drawing's turn (A26).
 *
 * The one thing in the product that loops, which is why it hands back a pause: its caller must show a control for it
 * and stop it while the page is hidden. Under reduced motion nothing runs and `onFrame` is never called, so the
 * drawing stays as it was drawn.
 */
export function cycle(periodMs: number, onFrame: (progress: number) => void): Cycle {
  if (reduced()) return STILL_CYCLE
  const clock = { progress: 0 }
  const animation = animate(clock, {
    progress: [0, 1],
    duration: periodMs,
    ease: 'linear',
    loop: true,
    onUpdate: () => onFrame(clock.progress),
  })
  return {
    pause: () => {
      animation.pause()
    },
    play: () => {
      animation.play()
    },
    cancel: () => {
      animation.cancel()
    },
  }
}
