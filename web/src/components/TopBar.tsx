import { Link } from '@tanstack/react-router'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { strings } from '../lib/strings'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import type { LibraryId } from '../lib/types'

/**
 * The bar across the top of every screen — `v2`'s chrome, in one row.
 *
 * # Why this is a component and not the root route
 *
 * The design draws the wordmark, the navigation, the search field and the upload button in
 * one 38px bar. Only the first two are true without context: search acts on a library, and
 * the library is a search param the *route* owns. `__root` cannot read it without reaching
 * past the route for state the route is responsible for, and — the part that decided it —
 * `index.test.tsx` renders `Index` directly, without a router, so anything hoisted into the
 * root leaves the search box and the upload button with no test covering them at all.
 *
 * So the bar is a component with a slot, rendered by each route that has the context to
 * fill it. Three call sites and one definition, rather than one call site and a router
 * mechanism for passing state upward.
 *
 * # What is not here
 *
 * The account control at the right end of the design's bar. There is no auth in Phase 1 —
 * `ROADMAP.md` puts it in Phase 8 — and a menu showing an avatar, a name and a role chip
 * would be the interface inventing a user who does not exist. The space it occupied goes to
 * the search field, which is the control that actually earns the width.
 */
export function TopBar({
  library,
  sidebar,
  onToggleSidebar,
  children,
}: {
  /** Carried so the tabs can keep the selected library across a navigation. */
  library: LibraryId
  /**
   * Whether the sidebar is showing, or `null` on a screen that has no sidebar to toggle.
   * `null` and not `false`: a screen without one must not render a control that claims to
   * hide something.
   */
  sidebar: boolean | null
  onToggleSidebar?: () => void
  /** The route's own controls — search, upload, the view menu. */
  children?: ReactNode
}) {
  /*
    The library rides on every tab as a search param, because it is what "which library" is
    spelled as everywhere else in this application. Omitted when it is the seeded one, so
    the common URL stays `/` rather than `/?library=<uuid>` — the same rule `ActionBar`'s
    Removed link follows.
  */
  const search = library === DEFAULT_LIBRARY_ID ? undefined : { library }
  return (
    <header className="flex flex-none flex-wrap items-center gap-[9px] border-b border-[var(--color-border)] bg-[var(--color-surface)] px-[13px] py-[9px]">
      {sidebar === null ? null : (
        <button
          type="button"
          onClick={onToggleSidebar}
          /*
            The label is the action and flips with the state. A button permanently reading
            "Sidebar" announces nothing about what pressing it does, and `aria-expanded`
            alone does not fix that for a control whose name never changes.
          */
          aria-label={sidebar ? strings.shell.hideSidebar : strings.shell.showSidebar}
          aria-expanded={sidebar}
          className="ease-mechanical grid size-[26px] flex-none place-items-center rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]"
        >
          {/*
            Two bars and a gap, drawn rather than lettered: the design uses a glyph here and
            every candidate character (⊞, ▤, ☰) is announced by a screen reader as its own
            name. `aria-hidden` on the drawing and the real label on the button is what makes
            the control say one thing rather than two.
          */}
          <span aria-hidden="true" className="flex h-[11px] w-[11px] items-stretch gap-[2px]">
            <span className="w-[3px] rounded-[1px] bg-current" />
            <span className="flex-1 rounded-[1px] border border-current opacity-60" />
          </span>
        </button>
      )}

      <BrandMark />
      {/*
        The application's name, letterspaced and uppercased in CSS rather than in
        `strings.ts`: the name is "Lapidary", and a screen reader should say that rather
        than spell it. The accessible name is the string; the treatment is the design.

        An `h1` on every screen, which is what it was in `__root` — the page's own heading
        is the `h2` above the grid. That split is deliberate: the level-1 heading names the
        application, and a screen with two `h1`s has no outline at all.
      */}
      <h1 className="flex-none text-xs leading-none font-bold tracking-[0.16em] uppercase">
        {strings.appName}
      </h1>

      {/*
        The tab group. A segmented control drawn as one inset well with the active tab
        raised out of it — the design's shape, and the reason it is a `nav` with links
        rather than buttons: these are places, so they get URLs, a middle click and a back
        button.
      */}
      <nav
        aria-label={strings.shell.navLabel}
        className="flex flex-none gap-[2px] rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] p-[3px]"
      >
        <Tab to="/" search={search}>
          {strings.shell.grid}
        </Tab>
        <Tab to="/organize" search={search}>
          {strings.shell.groups}
        </Tab>
      </nav>

      {children}
    </header>
  )
}

/**
 * One tab. `activeProps` rather than a hand-rolled comparison against the current path:
 * the router already knows which route is active, and a second answer computed here is a
 * second answer that can be wrong.
 *
 * `activeOptions={{ exact: true }}` because `/` is a prefix of every path in this
 * application — without it the Grid tab renders active while Groups is open.
 */
function Tab({
  to,
  search,
  children,
}: {
  to: '/' | '/organize'
  search: { library: LibraryId } | undefined
  children: ReactNode
}) {
  return (
    <Link
      to={to}
      search={search}
      activeOptions={{ exact: true }}
      className="ease-mechanical rounded-[var(--radius-sm)] px-[11px] py-[5px] text-xs leading-none font-semibold text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
      activeProps={{
        className:
          'bg-[var(--color-surface)] text-[var(--color-bright)] shadow-[0_1px_2px_rgba(0,0,0,0.4)]',
        'aria-current': 'page',
      }}
    >
      {children}
    </Link>
  )
}

/**
 * A square on its corner with a square inside it — the facet and table of a cut stone,
 * which is what a lapidary makes.
 *
 * Drawn rather than fetched: it is two divs and a rotation, and an SVG file would be a
 * second request and a second place for the brand to be defined. `rotate-45` on the outer
 * box carries the inner one with it, so both read as diamonds. 1.5px and not 1px — at 19px
 * a hairline outline disappears against the bar.
 */
function BrandMark() {
  return (
    <span
      aria-hidden="true"
      className="grid size-[19px] flex-none rotate-45 place-items-center rounded-[3px] border-[1.5px] border-[var(--color-accent)]"
    >
      <span className="size-[6px] bg-[var(--color-accent)]" />
    </span>
  )
}

/**
 * A popover anchored under the bar, dismissed by Escape and by a click outside it.
 *
 * **Not a `Dialog`.** That component is modal — it traps focus, covers the page with a
 * scrim and is the right shape for a decision. This is a set of view preferences a person
 * changes *while looking at the grid the changes apply to*, so covering the grid would hide
 * the only feedback the controls have.
 *
 * Both dismissals, because they are not interchangeable: Escape is the keyboard's way out
 * and SC 2.1.2 wants one, and an outside click is what a pointer user does without thinking
 * about it. Neither is a substitute for the other.
 *
 * `pointerdown` and not `click` for the outside dismissal: a `click` listener on the
 * document fires after the press that opened the panel has finished travelling, so a
 * trigger that toggles state closes the panel it just opened. `pointerdown` on the trigger
 * is already over by then, and the `contains` check below is what keeps a press *inside*
 * the panel from closing it.
 */
export function Popover({
  label,
  open,
  onOpenChange,
  children,
}: {
  /** The trigger's text, which is also the popover's accessible name. */
  label: string
  open: boolean
  onOpenChange: (open: boolean) => void
  children: ReactNode
}) {
  const anchor = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const outside = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Node && anchor.current?.contains(target) === false) {
        onOpenChange(false)
      }
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onOpenChange(false)
    }
    document.addEventListener('pointerdown', outside)
    document.addEventListener('keydown', escape)
    return () => {
      document.removeEventListener('pointerdown', outside)
      document.removeEventListener('keydown', escape)
    }
  }, [open, onOpenChange])

  return (
    <div ref={anchor} className="relative flex-none">
      <button
        type="button"
        onClick={() => onOpenChange(!open)}
        aria-expanded={open}
        className="ease-mechanical flex items-center gap-[5px] rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] px-[11px] py-[6px] text-xs leading-none font-semibold text-[var(--color-muted)] duration-[var(--duration-fast)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]"
      >
        {label}
        <span aria-hidden="true" className="text-[9px] opacity-75">
          ▾
        </span>
      </button>
      {!open ? null : (
        /*
          `role="group"` and not `dialog`: a dialog is modal by contract and this one
          deliberately is not, so announcing it as one would promise a focus trap that does
          not exist. The label is the trigger's own text, so the group is announced by the
          name of the thing that opened it.

          `right-0` rather than `left-0`: the trigger sits at the right end of the bar, and
          a panel 260px wide hanging left from it would run off a narrow window.
        */
        <div
          role="group"
          aria-label={label}
          className="panel-in absolute top-[calc(100%+7px)] right-0 z-50 flex w-[264px] flex-col gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 shadow-[0_18px_44px_rgba(0,0,0,0.55)]"
        >
          {children}
        </div>
      )}
    </div>
  )
}

/**
 * The state a `Popover` needs, so a caller does not spell out a `useState` and the two
 * handlers every time. One line at each of the two call sites, and the closing behaviour
 * cannot be forgotten at one of them.
 */
export function usePopover(): {
  open: boolean
  onOpenChange: (open: boolean) => void
  close: () => void
} {
  const [open, setOpen] = useState(false)
  return { open, onOpenChange: setOpen, close: () => setOpen(false) }
}
