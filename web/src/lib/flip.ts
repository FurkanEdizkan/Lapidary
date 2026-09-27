import { reduced, tokens } from './motion'

/**
 * Move an element from where another one was, once.
 *
 * FLIP: the element is already laid out where it belongs, so the animation runs *backwards*
 * from the old position to none. Nothing here touches layout — the whole thing is one
 * `transform`, which is the only property besides `opacity` this design system animates.
 *
 * It exists for one job. A part's render is the product: a person scans a wall of them and
 * decides from a picture. When they click one, the panel that opens is *about that picture*,
 * and a panel that simply appears makes them find the render again to confirm they opened
 * what they meant to. Carrying the render across says it without a word.
 */

/**
 * Animate `el` as though it had started at `from`.
 *
 * Returns `null` when it declined to animate, which a caller may ignore — the element is
 * already in its final position, so a skipped animation is a correct, static result rather
 * than a missing one.
 *
 * Declines when the reader asked for reduced motion, and when either rectangle has no area:
 * a zero-width source produces a `scale(0)` that reads as the panel bursting out of nothing,
 * which is a worse answer than no movement at all.
 */
export function flipFrom(
  el: HTMLElement,
  from: DOMRect,
  /**
   * Which token times the move. `slow` — 280ms — is the panel's, and stays the default so every
   * caller written before this argument existed is unchanged.
   *
   * `base` is the dashboard's. A widget changing places is a state change, which `DESIGN.md`
   * caps at 180ms, and `phase-6.md` names 180ms for it explicitly. Passed rather than hard-coded
   * because this is still the only FLIP in the application and a second copy of it under
   * `components/dashboard/` would be a second place for the reduced-motion and zero-area
   * declines to be forgotten.
   */
  duration: 'fast' | 'base' | 'slow' = 'slow',
): Animation | null {
  // Capability first, and not only for the test renderer: the Web Animations API is the one
  // thing here a runtime can lack, and a decorative flight is never worth throwing inside a
  // layout effect that a panel's render depends on.
  if (typeof el.animate !== 'function') return null
  if (reduced()) return null

  const to = el.getBoundingClientRect()
  if (from.width === 0 || from.height === 0 || to.width === 0 || to.height === 0) return null

  const dx = from.left - to.left
  const dy = from.top - to.top
  const sx = from.width / to.width
  const sy = from.height / to.height

  // Already where it started, to within a pixel: animating would be a 280ms pause.
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) {
    return null
  }

  // `--duration-slow` is 280ms and had no caller until this one. A view opening is not a
  // state change, which is what DESIGN.md caps at 180ms; it is the longest move the system
  // makes, and this is the move it was declared for. A caller that *is* making a state change
  // asks for `base` instead — the dashboard's widgets do.
  const t = tokens()
  return el.animate(
    [
      { transformOrigin: 'top left', transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
      { transformOrigin: 'top left', transform: 'none' },
    ],
    { duration: t[duration], easing: t.ease },
  )
}
