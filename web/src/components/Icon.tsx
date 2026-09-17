/**
 * Icons, drawn by Phosphor rather than by hand.
 *
 * Vendored from `@phosphor-icons/core` 2.1.1, regular weight (MIT, Copyright (c) 2023
 * Phosphor Icons), path data unchanged. Vendored and not installed: the React package
 * carries some 9,000 glyphs to use a handful, and an air-gapped build should not need a
 * registry for an `×`. Add a glyph by copying its `assets/regular/<name>.svg` path here.
 *
 * Decoration only. Every icon is `aria-hidden`, and the control that holds one carries the
 * label, so a screen reader hears "Close" and never "image".
 */
const GLYPHS = {
  close:
    'M205.66,194.34a8,8,0,0,1-11.32,11.32L128,139.31,61.66,205.66a8,8,0,0,1-11.32-11.32L116.69,128,50.34,61.66A8,8,0,0,1,61.66,50.34L128,116.69l66.34-66.35a8,8,0,0,1,11.32,11.32L139.31,128Z',
} as const

export type IconName = keyof typeof GLYPHS

/** 16px by default: at that size Phosphor's regular weight is a one-pixel line. */
export function Icon({ name, size = 16 }: { name: IconName; size?: number }) {
  return (
    <svg viewBox="0 0 256 256" width={size} height={size} fill="currentColor" aria-hidden="true">
      <path d={GLYPHS[name]} />
    </svg>
  )
}
