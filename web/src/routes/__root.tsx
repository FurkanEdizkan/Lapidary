import { Outlet, createRootRoute } from '@tanstack/react-router'
import { strings } from '../lib/strings'

/**
 * The application's frame: a bar with the mark in it, and everything else below.
 *
 * `v2` puts the wordmark, the navigation, the search field and the account control in one
 * 38px bar. Only the first of those belongs *here* — the rest need a library to act on, and
 * the library is a search param the route owns, so they live in the route's own toolbar
 * where that context exists. What the root contributes is the thing that is true on every
 * screen: which application this is.
 */
function BrandMark() {
  return (
    <span
      aria-hidden="true"
      /*
        A square on its corner with a square inside it — the facet-and-table of a cut stone,
        which is what a lapidary makes. Drawn rather than fetched: it is two divs and a
        rotation, and an SVG file would be a second request and a second place for the brand
        to be defined.

        `rotate-45` on the outer box carries the inner one with it, so both read as
        diamonds. 1.5px and not 1px — at 19px a hairline outline disappears against the bar.
      */
      className="grid size-[19px] flex-none rotate-45 place-items-center rounded-[3px] border-[1.5px] border-[var(--color-accent)]"
    >
      <span className="size-[6px] bg-[var(--color-accent)]" />
    </span>
  )
}

export const Route = createRootRoute({
  component: () => (
    <div className="flex min-h-screen flex-col bg-[var(--color-bg)] text-[var(--color-text)]">
      {/*
        The bar sits on `--color-surface`, a step *above* the page rather than on it, which
        is what separates it without needing the rule underneath to do the work alone.

        The wordmark is letterspaced and uppercased in CSS, not in `strings.ts`: the
        application's name is "Lapidary", and a screen reader should say that rather than
        spell it. The accessible name is the string; the treatment is the design.
      */}
      <header className="flex flex-none items-center gap-[9px] border-b border-[var(--color-border)] bg-[var(--color-surface)] px-[13px] py-[9px]">
        <BrandMark />
        <h1 className="text-xs leading-none font-bold tracking-[0.16em] uppercase">
          {strings.appName}
        </h1>
      </header>
      <main className="min-h-0 flex-1 px-[18px] py-[13px]">
        <Outlet />
      </main>
    </div>
  ),
})
