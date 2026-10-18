import { AnimatePresence, LazyMotion, MotionConfig, m, useIsPresent, type HTMLMotionProps, type Transition } from 'motion/react'
import type { ReactNode, Ref } from 'react'
import { bezier, reduced, tokens } from './motion-tokens'

/**
 * The one place Motion (motion.dev's `motion` package) is imported, beside `motion-features.ts` (A18).
 *
 * Motion does what anime.js does not do well in React: things leaving (`Presence`), layout moving (`layout`), and one
 * element travelling between two places (`layoutId`). anime.js keeps timelines and staggers (`motion.ts`). Both
 * read the same tokens, so a duration or the curve has one definition, in `styles.css`.
 *
 * Feature code uses the primitives below, never `m.*` directly, for one reason that is easy to forget:
 * `styles.css` gives every element a base `transition` on transform and opacity, and Motion writes those every
 * frame, so each write would be smeared over 180 ms. Every primitive carries `transition-none`.
 */

const SMEAR_FREE = 'transition-none'

/**
 * The one curve, at a token's length, as Motion takes it. Zero under reduced motion: a primitive's own `transition`
 * outranks `MotionRoot`'s, so an exit at `fast` would otherwise still fade for 120 ms (A22).
 */
export function mechanical(duration: 'fast' | 'base' | 'slow' | 'scene' | 'intro' = 'base'): Transition {
  const t = tokens()
  return { duration: reduced() ? 0 : t[duration] / 1000, ease: bezier(t) }
}

/** Layout and shared-element moves: critically damped, so nothing overshoots (A17's doctrine). */
export const SETTLE: Transition = { type: 'spring', visualDuration: 0.28, bounce: 0 }

/**
 * Around the whole app. Reduced motion is the end state at once: `reducedMotion="user"` alone keeps opacity fades,
 * so every transition is given zero duration as well.
 */
export function MotionRoot({ children }: { children: ReactNode }) {
  return (
    <LazyMotion features={() => import('./motion-features').then((module) => module.default)} strict>
      <MotionConfig reducedMotion="user" transition={mechanical('base')}>
        {children}
      </MotionConfig>
    </LazyMotion>
  )
}

/** Children that may leave: each needs a stable `key`. */
export const Presence = AnimatePresence

/** False while this component is leaving inside a `Presence`, true otherwise (and always outside one). */
export const usePresent = useIsPresent

type Primitive = Omit<HTMLMotionProps<'div'>, 'className'> & { className?: string }

function classes(className: string | undefined): string {
  return className === undefined ? SMEAR_FREE : `${className} ${SMEAR_FREE}`
}

/** In and out by opacity alone. */
export function Fade({ className, ...props }: Primitive) {
  return (
    <m.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: mechanical('fast') }}
      className={classes(className)}
      {...props}
    />
  )
}

/**
 * One picture giving way to the next on a stage (A23): both out and in over `base`, 180 ms, so the two layers cross
 * rather than the old one vanishing first. Each layer must be absolutely placed over the other.
 */
export function Crossfade({ className, ...props }: Primitive) {
  return (
    <m.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: mechanical('base') }}
      exit={{ opacity: 0, transition: mechanical('base') }}
      className={classes(className)}
      {...props}
    />
  )
}

/** Arriving: up 8px and in. What a page or a panel does when it first appears. */
export function Rise({ className, ...props }: Primitive) {
  return (
    <m.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, transition: mechanical('fast') }}
      className={classes(className)}
      {...props}
    />
  )
}

/**
 * One step of a sequence: in from the side it is going to, out to the side it came from. `direction` is 1 going
 * forward and -1 going back, and must be passed to `Presence`'s `custom` as well so the leaving step knows it.
 */
export function Step({ className, direction, ...props }: Primitive & { direction: 1 | -1 }) {
  return (
    <m.div
      custom={direction}
      variants={{
        enter: (to: number) => ({ opacity: 0, x: 16 * to }),
        here: { opacity: 1, x: 0, transition: mechanical('base') },
        leave: (to: number) => ({ opacity: 0, x: -16 * to, transition: mechanical('fast') }),
      }}
      initial="enter"
      animate="here"
      exit="leave"
      className={classes(className)}
      {...props}
    />
  )
}

/**
 * A marker that travels between places sharing its `id`: the current place, the current step.
 *
 * `move` is how it travels: the layout spring by default (the stepper's rail), or `base`, 180 ms on the curve, for
 * the header's places marker (DESIGN.md § Header). Under reduced motion it jumps, whichever it is: a transition given
 * here would otherwise outrank `MotionRoot`'s zero.
 */
export function Indicator({
  id,
  className,
  move = 'settle',
  ...props
}: Primitive & { id: string; move?: 'settle' | 'base' }) {
  const transition = reduced() ? { duration: 0 } : move === 'base' ? mechanical('base') : SETTLE
  return <m.div layoutId={id} transition={transition} className={classes(className)} {...props} />
}

/**
 * Something floating over the page — a dialog's box (A22): up 8px from 0.98 in over `base`, back out over `fast`.
 * Scaled from its centre, so it grows where it will sit rather than from a corner.
 */
export function Panel({ className, ...props }: Primitive & { ref?: Ref<HTMLDivElement> }) {
  return (
    <m.div
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1, transition: mechanical('base') }}
      exit={{ opacity: 0, y: 8, scale: 0.98, transition: mechanical('fast') }}
      className={classes(className)}
      {...props}
    />
  )
}
