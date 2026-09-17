/**
 * Icons, drawn by Phosphor rather than by hand.
 *
 * Vendored from `@phosphor-icons/core` 2.1.1, regular weight (MIT, Copyright (c) 2023
 * Phosphor Icons), path data unchanged (`close` is `x`, `caretDown` is `caret-down`, `search` is `magnifying-glass`, `upload` is `upload-simple`). Vendored and not installed: the React package
 * carries some 9,000 glyphs to use a handful, and an air-gapped build should not need a
 * registry for an `×`. Add a glyph by copying its `assets/regular/<name>.svg` path here.
 *
 * Decoration only. Every icon is `aria-hidden`, and the control that holds one carries the
 * label, so a screen reader hears "Close" and never "image".
 */
const GLYPHS = {
  caretDown:
    'M213.66,101.66l-80,80a8,8,0,0,1-11.32,0l-80-80A8,8,0,0,1,53.66,90.34L128,164.69l74.34-74.35a8,8,0,0,1,11.32,11.32Z',
  close:
    'M205.66,194.34a8,8,0,0,1-11.32,11.32L128,139.31,61.66,205.66a8,8,0,0,1-11.32-11.32L116.69,128,50.34,61.66A8,8,0,0,1,61.66,50.34L128,116.69l66.34-66.35a8,8,0,0,1,11.32,11.32L139.31,128Z',
  list: 'M224,128a8,8,0,0,1-8,8H40a8,8,0,0,1,0-16H216A8,8,0,0,1,224,128ZM40,72H216a8,8,0,0,0,0-16H40a8,8,0,0,0,0,16ZM216,184H40a8,8,0,0,0,0,16H216a8,8,0,0,0,0-16Z',
  search:
    'M229.66,218.34l-50.07-50.06a88.11,88.11,0,1,0-11.31,11.31l50.06,50.07a8,8,0,0,0,11.32-11.32ZM40,112a72,72,0,1,1,72,72A72.08,72.08,0,0,1,40,112Z',
  upload:
    'M224,144v64a8,8,0,0,1-8,8H40a8,8,0,0,1-8-8V144a8,8,0,0,1,16,0v56H208V144a8,8,0,0,1,16,0ZM93.66,77.66,120,51.31V144a8,8,0,0,0,16,0V51.31l26.34,26.35a8,8,0,0,0,11.32-11.32l-40-40a8,8,0,0,0-11.32,0l-40,40A8,8,0,0,0,93.66,77.66Z',
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
