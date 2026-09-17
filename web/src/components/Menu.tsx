import { type CSSProperties, type ReactNode } from 'react'

/** A button and the popover it opens. See `styles.css` for why it is native. */
export function Menu({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  // Each menu names its own anchor, so two menus on one page never position against the
  // same button.
  const anchor = { '--anchor': `--${id}` } as CSSProperties
  return (
    <>
      <button
        type="button"
        popoverTarget={id}
        style={anchor}
        className="menu-anchor ease-mechanical flex min-h-6 flex-none items-center gap-1.5 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-xs duration-[var(--duration-fast)] hover:-translate-y-px"
      >
        {label}
        <span aria-hidden="true" className="text-[9px] opacity-75">
          ▾
        </span>
      </button>
      <div
        id={id}
        popover="auto"
        data-menu
        role="group"
        aria-label={label}
        style={anchor}
        /*
          `open:flex`, never `flex`. A `display` utility on the popover itself outranks the
          browser's own `[popover]:not(:popover-open) { display: none }` — author styles beat
          user-agent ones — so a plain `flex` here painted both menus permanently open over
          the grid while every test stayed green: jsdom applies none of these classes, so the
          suite could not see it. The first browser check did.
        */
        className="menu panel-in w-64 flex-col gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[var(--color-text)] shadow-overlay open:flex"
      >
        {children}
      </div>
    </>
  )
}
