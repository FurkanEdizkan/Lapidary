/**
 * The anatomy every page past the grid shares, as classes: one headline, a lead no wider than a
 * reading measure, and sections that open on a small label over a scribe line rather than living in
 * bordered boxes. Boxes are for things that float or that you pick up (cards, dialogs); a section of
 * a page is neither, and a page of boxes reads as a form.
 */
export const HEADLINE = 'text-2xl leading-tight font-semibold tracking-tight text-[var(--color-bright)]'

export const LEAD = 'mt-2 max-w-[70ch] text-sm text-[var(--color-dim)]'

export const SECTION = 'mt-10 border-t border-[var(--color-border)] pt-4'

export const SECTION_TITLE = 'text-xs font-medium tracking-wider text-[var(--color-muted)] uppercase'

/** The page's one standing control: brighter and heavier than the quiet ones, never blue. */
export const STANDING =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px'
