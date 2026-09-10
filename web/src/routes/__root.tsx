import { Outlet, createRootRoute } from '@tanstack/react-router'

/**
 * The application's ground, and nothing else.
 *
 * # Why the bar is not here any more
 *
 * It was: a header with the wordmark in it, on the argument that "which application is
 * this" is the one fact true on every screen. `v2` puts four more things in that row — the
 * section tabs, the search field, the upload button and the view menu — and three of those
 * four act on a library, which is a search param the *route* owns. Reading it here would
 * mean the root reaching past the route for state the route is responsible for, and it
 * would put the search box outside what `index.test.tsx` renders, since that suite renders
 * `Index` directly without a router.
 *
 * So the bar became `components/TopBar.tsx`, with a slot the route fills, and each route
 * renders it. Three call sites and one definition. What is left here is the page's ground
 * and its text colour — the two things that are genuinely true of every screen and that
 * nothing below would otherwise set.
 *
 * There is no `<main>` wrapper here either. Each route lays itself out differently now: the
 * grid is a full-height three-column shell that scrolls in the middle, and the part page is
 * a padded column. A shared padded `main` would fight the first and duplicate the second.
 */
export const Route = createRootRoute({
  component: () => (
    <div className="flex min-h-screen flex-col bg-[var(--color-bg)] text-[var(--color-text)]">
      <Outlet />
    </div>
  ),
})
