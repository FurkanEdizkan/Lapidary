import { Link } from '@tanstack/react-router'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { LibraryId } from '../lib/types'
import { Icon } from './Icon'
import { JumpSearch } from './Search'

/**
 * A square on its corner with a square inside it — the facet-and-table of a cut stone,
 * which is what a lapidary makes. Drawn rather than fetched: it is two spans and a rotation,
 * and an SVG file would be a second request and a second place for the brand to be defined.
 * 1.5px and not 1px — at 19px a hairline outline disappears against the bar.
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

export type Place = 'parts' | 'removed' | 'sharing'

const PLACE =
  'ease-mechanical flex min-h-7 items-center rounded-[5px] px-3 text-xs duration-[var(--duration-fast)]'
// The border grey as a fill: the header is `surface` and the nav's well is `raised`, so a
// `surface` pill, which marked the place on the old page-ground toolbar, vanished into the bar.
const HERE = `${PLACE} bg-[var(--color-border)] font-semibold text-[var(--color-bright)]`
const THERE = `${PLACE} text-[var(--color-muted)] hover:text-[var(--color-text)]`

/**
 * Where you are, among the three places the application has.
 *
 * `current` is a prop rather than read from the router: the place is a fact the page knows,
 * and each page's tests mount it under a synthetic route tree where route matching would
 * answer nothing. The current place is marked and not linked — a link to `/` from `/` would
 * drop the category and the search you are in the middle of.
 */
function Places({ current, library, stacked = false }: { current?: Place; library?: LibraryId; stacked?: boolean }) {
  const search = library === undefined || library === DEFAULT_LIBRARY_ID ? undefined : { library }
  const inParts = current === 'parts'
  const inRemoved = current === 'removed'
  const inSharing = current === 'sharing'
  return (
    <nav
      aria-label={strings.frame.places}
      className={`flex flex-none gap-0.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-raised)] p-[3px] ${
        stacked ? 'flex-col' : 'items-center'
      }`}
    >
      {inParts ? (
        <span aria-current="page" className={HERE}>
          {strings.frame.parts}
        </span>
      ) : (
        // Exact: the router marks a matching link `aria-current` itself, and `/` prefixes every path.
        <Link to="/" search={search} activeOptions={{ exact: true }} className={THERE}>
          {strings.frame.parts}
        </Link>
      )}
      {inRemoved ? (
        <span aria-current="page" className={HERE}>
          {strings.removal.removedTitle}
        </span>
      ) : (
        <Link to="/removed" search={search} className={THERE}>
          {strings.removal.removedTitle}
        </Link>
      )}
      {/* The whole installation's, not one library's, so it carries no library. */}
      {inSharing ? (
        <span aria-current="page" className={HERE}>
          {strings.sharing.title}
        </span>
      ) : (
        <Link to="/sharing" className={THERE}>
          {strings.sharing.title}
        </Link>
      )}
    </nav>
  )
}

/**
 * The page every route renders into: a skip link, one header, and the content.
 *
 * Rendered by each page rather than by the root route, because the grid's header controls
 * are the grid's state. Search is the `q` the route owns, Upload feeds the drop that page
 * watches, and the library menu acts on the library in its URL. A header in the root would
 * have to reach down for all of that; a header each page fills in is a slot.
 *
 * **One row on a wide screen, two on a narrow one.** Mark, places, search, then the page's
 * actions. Under `md` the search takes a row of its own, because a search field squeezed to
 * the width left over by four controls is a field nobody can read back.
 *
 * `rail` is the categories-and-filters column. On a wide screen it sits beside the content;
 * under `md` it becomes a drawer. The drawer is CSS over always-rendered content, never a
 * `matchMedia` decision: whether the rail exists must not depend on a query the test
 * renderer answers however it is stubbed.
 */
export function AppFrame({
  current,
  library,
  skipTo,
  search,
  actions,
  rail,
  nav = true,
  children,
}: {
  current?: Place
  /** Carried by the places that are one library's, so leaving and coming back keeps it. */
  library?: LibraryId
  /** The content's anchor, for the skip link. No skip link without one. */
  skipTo?: { href: string; label: string }
  /** The page's own search. Absent is a search that takes you to the grid; `null` is none. */
  search?: ReactNode
  actions?: ReactNode
  rail?: ReactNode
  /** Off for a page that cannot trust the router, which is the crash page. */
  nav?: boolean
  children: ReactNode
}) {
  const [drawerOpen, setDrawerOpen] = useState(false)
  const drawerButton = useRef<HTMLButtonElement>(null)
  const closeDrawer = useRef<HTMLButtonElement>(null)
  // Under `md` the places and the rail both live in the drawer, so a page with either has one.
  const drawer = nav || rail !== undefined

  useEffect(() => {
    if (!drawerOpen) return
    // Into the drawer, so the next Tab is in what just opened rather than behind the scrim.
    closeDrawer.current?.focus()
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setDrawerOpen(false)
      drawerButton.current?.focus()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [drawerOpen])

  /*
    The rail beside the content on a wide screen; under `md`, a drawer over it holding the
    places as well. A page without a rail has no column, so its panel exists only as the drawer.
    The places are rendered into it only while it is open: a second copy of the nav in the
    document would be a second landmark with the same name.
  */
  const panel = !drawer ? null : (
    <>
      {drawerOpen ? (
        <div
          aria-hidden="true"
          onClick={() => setDrawerOpen(false)}
          className="scrim-in fixed inset-0 z-[var(--z-drawer)] bg-black/60 md:hidden"
        />
      ) : null}
      <div
        id="drawer"
        className={`${rail === undefined ? 'md:hidden' : 'w-56 shrink-0'} max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-[var(--z-drawer)] max-md:w-72 max-md:max-w-[85vw] max-md:overflow-y-auto max-md:border-r max-md:border-[var(--color-border)] max-md:bg-[var(--color-bg)] max-md:p-4 ${
          drawerOpen ? '' : 'max-md:invisible max-md:-translate-x-full'
        }`}
      >
        <button
          ref={closeDrawer}
          type="button"
          onClick={() => {
            setDrawerOpen(false)
            drawerButton.current?.focus()
          }}
          aria-label={strings.frame.closeDrawer}
          className="ease-mechanical mb-3 ml-auto flex size-8 items-center justify-center rounded-[var(--radius-ctl)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] md:hidden"
        >
          <Icon name="close" />
        </button>
        {drawerOpen && nav ? (
          <div className="mb-5 md:hidden">
            <Places current={current} library={library} stacked />
          </div>
        ) : null}
        {rail}
      </div>
    </>
  )

  return (
    <div className="flex min-h-screen flex-col">
      {skipTo === undefined ? null : (
        /*
          The first tab stop on the page, and off-screen until it is one. SC 2.4.1, Level A.
          Moved by `translate` rather than hidden: `display: none` and `visibility: hidden`
          both remove it from the tab order, which is the one thing it must stay in.
        */
        <a
          href={skipTo.href}
          className="ease-mechanical fixed top-4 left-4 z-[var(--z-skip)] -translate-y-20 rounded-sm border border-[var(--color-edge)] bg-[var(--color-surface)] px-3 py-2 text-sm duration-[var(--duration-fast)] focus:translate-y-0"
        >
          {skipTo.label}
        </a>
      )}
      {/*
        On `--color-surface`, a step above the page, which is what separates it without the
        rule underneath doing the work alone. Sticky, so search and Upload are one reach away
        from anywhere down a long grid.
      */}
      <header className="sticky top-0 z-[var(--z-header)] flex flex-none flex-wrap items-center gap-x-3 gap-y-2 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-[13px] py-[9px]">
        {!drawer ? null : (
          <button
            ref={drawerButton}
            type="button"
            aria-expanded={drawerOpen}
            aria-controls="drawer"
            aria-label={strings.frame.openDrawer}
            onClick={() => setDrawerOpen(true)}
            className="ease-mechanical -ml-1 flex size-8 items-center justify-center rounded-[var(--radius-ctl)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] md:hidden"
          >
            <Icon name="list" />
          </button>
        )}
        <div className="flex flex-none items-center gap-[9px]">
          <BrandMark />
          {/*
            Letterspaced and uppercased in CSS, not in `strings.ts`: the application's name is
            "Lapidary", and a screen reader should say that rather than spell it.
          */}
          <h1 className="text-xs leading-none font-bold tracking-[0.16em] uppercase">{strings.appName}</h1>
        </div>
        {nav ? (
          <div className="max-md:hidden">
            <Places current={current} library={library} />
          </div>
        ) : null}
        {search === null ? null : (
          <div className="flex min-w-0 flex-1 max-md:order-last max-md:basis-full md:max-w-[36rem]">
            {search ?? <JumpSearch />}
          </div>
        )}
        {actions === undefined ? null : (
          <div className="ml-auto flex flex-none items-center gap-2">{actions}</div>
        )}
      </header>
      <main className="min-h-0 flex-1 px-[18px] py-[13px]">
        {rail === undefined ? (
          <>
            {panel}
            {children}
          </>
        ) : (
          <div className="flex items-start gap-6">
            {panel}
            {children}
          </div>
        )}
      </main>
    </div>
  )
}
