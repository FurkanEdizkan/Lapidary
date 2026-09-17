import { type CSSProperties, type ReactNode } from 'react'
import { Icon, type IconName } from './Icon'

/** A button and the popover it opens. See `styles.css` for why it is native. */
export function Menu({
  id,
  label,
  icon,
  children,
}: {
  id: string
  label: string
  /** Shown instead of the label, which then names the button to a screen reader. */
  icon?: IconName
  children: ReactNode
}) {
  // Each menu names its own anchor, so two menus on one page never position against the
  // same button.
  const anchor = { '--anchor': `--${id}` } as CSSProperties
  return (
    <>
      <button
        type="button"
        popoverTarget={id}
        style={anchor}
        aria-label={icon === undefined ? undefined : label}
        className={
          icon === undefined
            ? 'menu-anchor ease-mechanical flex min-h-6 flex-none items-center gap-1.5 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-1.5 text-xs duration-[var(--duration-fast)] hover:-translate-y-px'
            : 'menu-anchor ease-mechanical flex min-h-8 flex-none items-center justify-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-surface)] px-2.5 py-2 duration-[var(--duration-fast)] hover:-translate-y-px'
        }
      >
        {icon === undefined ? (
          <>
            {label}
            <span className="opacity-75">
              <Icon name="caretDown" size={12} />
            </span>
          </>
        ) : (
          <Icon name={icon} />
        )}
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
