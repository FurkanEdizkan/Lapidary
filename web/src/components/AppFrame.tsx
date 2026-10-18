import { Link, useRouterState } from '@tanstack/react-router'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { strings } from '../lib/strings'
import type { LibraryId } from '../lib/types'
import { Icon } from './Icon'
import { JumpSearch } from './Search'
import { Menu } from './Menu'
import { Fade, Indicator, Presence, Rise, mechanical } from '../lib/motion-react'
import { LOGIN_PATH, currentUser, forgetSession, signOut } from '../lib/session'

/**
 * A square on its corner with a square inside it — the facet-and-table of a cut stone,
 * which is what a lapidary makes. Drawn rather than fetched: it is two spans and a rotation,
 * and an SVG file would be a second request and a second place for the brand to be defined.
 * 1.5px and not 1px — at 19px a hairline outline disappears against the bar.
 */
export function BrandMark({ filled = false }: { filled?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className="relative grid size-[19px] flex-none rotate-45 place-items-center rounded-[3px] border-[1.5px] border-[var(--color-accent)]"
    >
      <span className="size-[6px] bg-[var(--color-accent)]" />
      {/* Setup's done moment (A20): the diamond fills, by scale and opacity, while the server is followed. */}
      <span
        className={`ease-mechanical absolute inset-0 rounded-[1px] bg-[var(--color-accent)] duration-[var(--duration-slow)] ${filled ? 'opacity-100' : 'opacity-0 [transform:scale(0.3)]'}`}
      />
    </span>
  )
}

/**
 * Where a page is. The first six are the pages a place names; `part` and `tags` are pages inside Parts (a part's
 * own page, the tag index), which mark Parts without being it, so Parts stays a link there (A21).
 */
export type Place = 'parts' | 'dashboard' | 'sharedWithMe' | 'siteSettings' | 'profile' | 'people' | 'part' | 'tags'

/** Which mark the header shows for a page: one of the three places, or the account button for its pages. */
type Marked = 'parts' | 'dashboard' | 'sharedWithMe' | 'account'

function markedFor(current: Place | undefined): Marked | undefined {
  switch (current) {
    case 'parts':
    case 'part':
    case 'tags':
      return 'parts'
    case 'dashboard':
    case 'sharedWithMe':
      return current
    case 'siteSettings':
    case 'profile':
    case 'people':
      return 'account'
    case undefined:
      return undefined
  }
}

/**
 * Out of the JSX on purpose: `no-bare-strings` reads any attribute literal that contains a strings.ts
 * fragment as copy, and "settings" is one (`dashboard.settingsTitle`). A route path is not copy.
 */
const SETTINGS_PATH = '/settings'
const PROFILE_PATH = '/profile'
const PEOPLE_PATH = '/people'
const SHARED_PATH = '/shared'

// The places by name, out of the JSX for the reason the paths above are.
const PARTS = 'parts'
const DASHBOARD = 'dashboard'
const SHARED = 'sharedWithMe'
const ACCOUNT = 'account'

/** The one id the places marker travels by, between pages as well as within one (DESIGN.md § Header). */
const MARKER = 'place-marker'

const HOME =
  'ease-mechanical flex flex-none items-center gap-[9px] rounded-[var(--radius-ctl)] duration-[var(--duration-fast)] hover:opacity-80'

// Text weight and the marker carry the current place; the Bench Grey tray they replace is gone (A17).
const PLACE =
  'ease-mechanical flex h-full items-center px-3 text-xs duration-[var(--duration-fast)]'
const HERE = `${PLACE} font-semibold text-[var(--color-bright)]`
const THERE = `${PLACE} text-[var(--color-muted)] hover:text-[var(--color-text)]`
// Stacked in the drawer, which has no marker of its own: a second element holding the marker's id would fight the
// header's for it. A 2px Quicklime rule at the row's start says the same thing, still.
const ROW = 'ease-mechanical flex min-h-9 items-center border-l-2 px-3 text-sm duration-[var(--duration-fast)]'
const ROW_HERE = `${ROW} border-[var(--color-bright)] font-semibold text-[var(--color-bright)]`
const ROW_THERE = `${ROW} border-transparent text-[var(--color-muted)] hover:text-[var(--color-text)]`

/**
 * The 2px Quicklime line under the current place, at the header's foot. Among the three places it is one `Indicator`
 * that travels between them. Under the account button it is a plain line: under `md` the places are `display: none`, so
 * a shared marker would fly in from a 0×0 box at the window's corner, and DESIGN.md moves it among the places only.
 */
function Marker({ travels = true }: { travels?: boolean }) {
  return travels ? (
    <Indicator
      id={MARKER}
      move="base"
      aria-hidden="true"
      data-place-marker
      className="absolute inset-x-3 bottom-0 h-0.5 bg-[var(--color-bright)]"
    />
  ) : (
    <span aria-hidden="true" data-place-marker className="absolute inset-x-1 bottom-0 h-0.5 bg-[var(--color-bright)]" />
  )
}

/**
 * Where you are, among the application's places: the grid, the dashboard and what was shared with you. Profile,
 * Settings and People are the account menu's, at the header's right (M2).
 *
 * `current` is a prop rather than read from the router: the place is a fact the page knows,
 * and each page's tests mount it under a synthetic route tree where route matching would
 * answer nothing. The page you are on is marked and not linked — a link to `/` from `/` would
 * drop the category and the search you are in the middle of. A page inside Parts marks Parts and keeps it a link.
 */
function Places({ current, library, stacked = false }: { current?: Place; library?: LibraryId; stacked?: boolean }) {
  const search = library === undefined || library === DEFAULT_LIBRARY_ID ? undefined : { library }
  const marked = markedFor(current)
  const here = stacked ? ROW_HERE : HERE
  const there = stacked ? ROW_THERE : THERE
  const place = (
    name: 'parts' | 'dashboard' | 'sharedWithMe',
    label: string,
    link: (className: string) => ReactNode,
  ) => (
    <div className={stacked ? 'flex' : 'relative flex h-full'}>
      {current === name ? (
        <span aria-current="page" className={here}>
          {label}
        </span>
      ) : (
        link(marked === name ? here : there)
      )}
      {!stacked && marked === name ? <Marker /> : null}
    </div>
  )
  return (
    <nav
      aria-label={strings.frame.places}
      className={stacked ? 'flex flex-col gap-0.5' : 'flex h-full flex-none items-stretch'}
    >
      {place(PARTS, strings.frame.parts, (className) => (
        // Exact: the router marks a matching link `aria-current` itself, and `/` prefixes every path.
        <Link to="/" search={search} activeOptions={{ exact: true }} className={className}>
          {strings.frame.parts}
        </Link>
      ))}
      {/*
        Second, not first: `/` is the grid and the grid is what this application is for. The
        dashboard is the overview beside it, and it carries the library for the same reason
        Parts does — leaving it and coming back keeps the library you were in, which
        is the library the Add form offers first.
      */}
      {place(DASHBOARD, strings.dashboard.title, (className) => (
        <Link to="/dashboard" search={search} className={className}>
          {strings.dashboard.title}
        </Link>
      ))}
      {/* What other people shared with you (A4): theirs to change, yours to look at and download. */}
      {place(SHARED, strings.sharing.place, (className) => (
        <Link to={SHARED_PATH} className={className}>
          {strings.sharing.place}
        </Link>
      ))}
    </nav>
  )
}

const MENU_ITEM =
  'ease-mechanical rounded-[var(--radius-ctl)] px-3 py-1.5 text-left text-sm duration-[var(--duration-fast)] hover:bg-[var(--color-raised)]'

/**
 * The user menu, at the far right of every page's header (M2): the signed-in person's profile, the
 * settings, and signing out (A1). The page you are on is marked and not linked, as the places are.
 */
function UserMenu({ current }: { current?: Place }) {
  const inProfile = current === 'profile'
  const inSettings = current === 'siteSettings'
  const inPeople = current === 'people'
  // The admin's accounts page (A4), for an admin only.
  const admin = currentUser()?.isAdmin ?? false
  const marked = markedFor(current) === ACCOUNT
  return (
    // The account button is the place for its own pages: on Profile, Settings or People the marker sits under it.
    <div className="relative flex h-full items-center px-1">
    <Menu id="user-menu" label={strings.user.menu} icon="user">
      {inProfile ? (
        <span aria-current="page" className={`${MENU_ITEM} font-semibold`}>
          {strings.user.profile}
        </span>
      ) : (
        <Link to={PROFILE_PATH} className={MENU_ITEM}>
          {strings.user.profile}
        </Link>
      )}
      {inSettings ? (
        <span aria-current="page" className={`${MENU_ITEM} font-semibold`}>
          {strings.settings.title}
        </span>
      ) : (
        <Link to={SETTINGS_PATH} className={MENU_ITEM}>
          {strings.settings.title}
        </Link>
      )}
      {!admin ? null : inPeople ? (
        <span aria-current="page" className={`${MENU_ITEM} font-semibold`}>
          {strings.user.people}
        </span>
      ) : (
        <Link to={PEOPLE_PATH} className={MENU_ITEM}>
          {strings.user.people}
        </Link>
      )}
      {/* A full load to the sign-in page, so nothing cached for this person outlives the session (A1). */}
      <button
        type="button"
        onClick={() => {
          forgetSession()
          void signOut().finally(() => window.location.assign(LOGIN_PATH))
        }}
        className={MENU_ITEM}
      >
        {strings.user.signOut}
      </button>
    </Menu>
    {marked ? <Marker travels={false} /> : null}
    </div>
  )
}

/**
 * A page arriving: up 8px and in over 180 ms, enter only (DESIGN.md § Motion, "Routes").
 *
 * Here around the content, not in `__root.tsx` around the `Outlet` where REDESIGN put it, because every page renders
 * its own header: a `Rise` around the outlet would fade the whole header in on every navigation, hiding the places
 * marker's move, and for those 180 ms its transform would make the drawer, the scrim and the skip link fixed to the
 * wrapper instead of to the viewport. Keyed on the pathname the router has resolved, not the one it is loading, so the
 * leaving page is never re-keyed and replayed while the gate runs. No exit, so two pages never share the DOM.
 */
function PageEnter({ className, children }: { className?: string; children: ReactNode }) {
  const pathname = useRouterState({ select: (state) => (state.resolvedLocation ?? state.location).pathname })
  return (
    <Rise key={pathname} className={className}>
      {children}
    </Rise>
  )
}

/** The width at and over which the header is one row with everything in it: Tailwind's `md`. */
const WIDE = '(min-width: 48rem)'

/**
 * The page every route renders into: a skip link, one header, and the content.
 *
 * Rendered by each page rather than by the root route, because the grid's header controls
 * are the grid's state. Search is the `q` the route owns, Upload feeds the drop that page
 * watches, and the library menu acts on the library in its URL. A header in the root would
 * have to reach down for all of that; a header each page fills in is a slot.
 *
 * **One 56px row at every width** (A21). Mark, places, search, then the page's tools and actions and the account.
 * Under `md` the places and the page's `tools` (Library, Import) move into the drawer, search folds to an icon that
 * opens it across the row, and Upload keeps only its icon: three header rows were 140px of an 844px phone.
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
  tools,
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
  /** Quiet, now-and-then controls: in the header on a wide screen, in the drawer under `md`. */
  tools?: ReactNode
  /** The page's own standing control, which stays in the header at every width. */
  actions?: ReactNode
  rail?: ReactNode
  /** Off for a page that cannot trust the router, which is the crash page. */
  nav?: boolean
  children: ReactNode
}) {
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [scrolled, setScrolled] = useState(false)
  const drawerButton = useRef<HTMLButtonElement>(null)
  const closeDrawer = useRef<HTMLButtonElement>(null)
  const searchButton = useRef<HTMLButtonElement>(null)
  const searchRow = useRef<HTMLDivElement>(null)
  // Under `md` the places, the tools and the rail all live in the drawer, so a page with any of them has one.
  const drawer = nav || rail !== undefined || tools !== undefined

  useEffect(() => {
    if (!drawerOpen) return
    // Into the drawer, so the next Tab is in what just opened rather than behind the scrim.
    closeDrawer.current?.focus()
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setDrawerOpen(false)
      drawerButton.current?.focus()
    }
    // Widened past `md` with the drawer open: there is no drawer there, and the tools it holds belong in the header.
    const wide = typeof window.matchMedia === 'function' ? window.matchMedia(WIDE) : null
    const onWide = (event: MediaQueryListEvent) => {
      if (event.matches) setDrawerOpen(false)
    }
    document.addEventListener('keydown', onKeyDown)
    wide?.addEventListener?.('change', onWide)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      wide?.removeEventListener?.('change', onWide)
    }
  }, [drawerOpen])

  useEffect(() => {
    if (!searchOpen) return
    searchRow.current?.querySelector('input')?.focus()
  }, [searchOpen])

  // The header gains its hairline once something has scrolled under it, and sits flush on the ground at the top.
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 0)
    onScroll()
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  /*
    The rail beside the content on a wide screen; under `md`, a drawer over it holding the
    places and the tools as well. A page without a rail has no column, so its panel exists only as the drawer.
    The places and the tools are rendered into it only while it is open: a second copy of the nav in the
    document would be a second landmark with the same name, and a second Library menu a second `library-menu` id.
  */
  const panel = !drawer ? null : (
    <>
      {/* The scrim fades in and out by presence over 120 ms (A22's dialogs do the same); the drawer slides by CSS. */}
      <Presence>
        {drawerOpen ? (
          <Fade
            key="drawer-scrim"
            aria-hidden="true"
            transition={mechanical('fast')}
            onClick={() => setDrawerOpen(false)}
            className="fixed inset-0 z-[var(--z-drawer)] bg-black/60 md:hidden"
          />
        ) : null}
      </Presence>
      <div
        id="drawer"
        className={`${rail === undefined ? 'md:hidden' : 'w-56 shrink-0'} max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-[var(--z-drawer)] max-md:w-72 max-md:max-w-[85vw] max-md:overflow-y-auto max-md:overscroll-contain max-md:border-r max-md:border-[var(--color-border)] max-md:bg-[var(--color-bg)] max-md:p-4 ${
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
        {drawerOpen && tools !== undefined ? (
          <div className="mb-5 flex flex-col gap-2 border-t border-[var(--color-border)] pt-4 md:hidden">{tools}</div>
        ) : null}
        {rail}
      </div>
    </>
  )

  const searchShown = search !== null
  return (
    <div className="flex min-h-dvh flex-col">
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
        Machined Slate, a step above the page, and sticky, so search and Upload are one reach away from anywhere down
        a long grid. The Scribe Line arrives once the page scrolls under it; the border is always there, transparent
        at the top, so the row never moves by a pixel when it appears.

        **No blur behind it**, though DESIGN.md asks for one (decided in A21, for the owner): blur shows only through
        a translucent ground, and over a white picture Edge falls under 3:1 at any alpha below 100% (2.78:1 at 97%),
        and Graphite under 4.5:1 below 96%. Upload's boundary and the places' labels sit on this ground.
      */}
      <header
        className={`sticky top-0 z-[var(--z-header)] flex h-14 flex-none items-center gap-3 border-b bg-[var(--color-surface)] px-[13px] ${
          scrolled ? 'border-[var(--color-border)]' : 'border-transparent'
        }`}
      >
        {!drawer ? null : (
          <button
            ref={drawerButton}
            type="button"
            aria-expanded={drawerOpen}
            aria-controls="drawer"
            aria-label={strings.frame.openDrawer}
            onClick={() => setDrawerOpen(true)}
            className="ease-mechanical -ml-1 flex size-8 flex-none items-center justify-center rounded-[var(--radius-ctl)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] md:hidden"
          >
            <Icon name="list" />
          </button>
        )}
        {/*
          The mark goes home: the grid, as a logo does on every site (M1). A plain link on the crash
          page, which cannot trust the router — and where a full reload is the point.
        */}
        {nav ? (
          <Link to="/" className={HOME}>
            <BrandMark />
            {/*
              Letterspaced and uppercased in CSS, not in `strings.ts`: the application's name is
              "Lapidary", and a screen reader should say that rather than spell it.
            */}
            <h1 className="text-xs leading-none font-bold tracking-[0.16em] uppercase">{strings.appName}</h1>
          </Link>
        ) : (
          <a href="/" className={HOME}>
            <BrandMark />
            <h1 className="text-xs leading-none font-bold tracking-[0.16em] uppercase">{strings.appName}</h1>
          </a>
        )}
        {nav ? (
          <div className="h-full max-md:hidden">
            <Places current={current} library={library} />
          </div>
        ) : null}
        {!searchShown ? null : (
          /*
            Always in the document, folded by CSS under `md` rather than unmounted: the `/` shortcut and every query
            that finds the field by its name have to find it whatever the width. Opened, it covers the row.
          */
          <div
            ref={searchRow}
            onKeyDown={(event) => {
              if (event.key !== 'Escape' || !searchOpen) return
              setSearchOpen(false)
              searchButton.current?.focus()
            }}
            className={`flex min-w-0 flex-1 items-center gap-2 md:max-w-[36rem] ${
              searchOpen
                ? 'max-md:absolute max-md:inset-0 max-md:z-10 max-md:bg-[var(--color-surface)] max-md:px-[13px]'
                : 'max-md:hidden'
            }`}
          >
            {search ?? <JumpSearch />}
            <button
              type="button"
              aria-label={strings.frame.closeSearch}
              onClick={() => {
                setSearchOpen(false)
                searchButton.current?.focus()
              }}
              className="ease-mechanical flex size-8 flex-none items-center justify-center rounded-[var(--radius-ctl)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] md:hidden"
            >
              <Icon name="close" />
            </button>
          </div>
        )}
        {actions === undefined && tools === undefined && !nav && !searchShown ? null : (
          <div className="ml-auto flex h-full flex-none items-center gap-2">
            {!searchShown ? null : (
              <button
                ref={searchButton}
                type="button"
                aria-label={strings.frame.openSearch}
                aria-expanded={searchOpen}
                onClick={() => setSearchOpen(true)}
                className="ease-mechanical flex size-8 flex-none items-center justify-center rounded-[var(--radius-ctl)] text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] md:hidden"
              >
                <Icon name="search" />
              </button>
            )}
            {tools === undefined || drawerOpen ? null : <div className="flex items-center gap-2 max-md:hidden">{tools}</div>}
            {actions}
            {nav ? <UserMenu current={current} /> : null}
          </div>
        )}
      </header>
      <main className="min-h-0 flex-1 px-[18px] py-[13px]">
        {rail === undefined ? (
          <>
            {panel}
            {nav ? <PageEnter>{children}</PageEnter> : children}
          </>
        ) : (
          <div className="flex items-start gap-6">
            {panel}
            {nav ? <PageEnter className="flex min-w-0 flex-1 items-start gap-6">{children}</PageEnter> : children}
          </div>
        )}
      </main>
    </div>
  )
}
