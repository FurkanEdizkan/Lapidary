import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import {
  byKey,
  libraryOf,
  readLayout,
  requestOf,
  resolve,
  writeLayout,
  type StoredLayout,
} from '../lib/dashboard'
import { useAppEvents } from '../lib/events'
import { strings } from '../lib/strings'
import type { LibraryId, WidgetRequest, WidgetResult } from '../lib/types'
import { AppFrame } from '../components/AppFrame'
import { HEADLINE, LEAD } from '../components/Page'
import { AddGroup, AddWidget } from '../components/dashboard/AddWidget'
import { Board, settleAll } from '../components/dashboard/Board'

/**
 * The dashboard: what every library holds, in one request.
 *
 * **One request, and the whole design turns on it.** Twelve widgets on a page, one
 * `POST /api/dashboard/resolve`, and the answer carries a result per key — `ok`, `timedOut` or
 * `failed` — so a slow key says so in its own panel while the other eleven render. There is no
 * per-widget endpoint and there is no polling: `FEATURES.md` §8 calls per-widget polling a
 * self-inflicted DoS, and at twelve panels it would be one. A test under
 * `components/dashboard/` fails if `refetchInterval` appears anywhere in this feature.
 *
 * What keeps the page current instead is `GET /api/events` (`lib/events.ts`): the server says
 * which library changed, and this route asks again **for that library's keys only, in one
 * call, at most once a second**. A resync — a reconnect, or a subscriber the hub could not keep
 * up with — asks for everything, once.
 *
 * **The arrangement is this browser's**, in `localStorage` under `lapidary.dashboard`, version
 * 1, exactly as `phase-6.md` decided and `preferences.ts` already argues for the grid: there is
 * no user table until Phase 8, so a column would make one operator's dashboard everybody's.
 */
export const Route = createFileRoute('/dashboard')({
  component: RouteComponent,
  /*
    The library travels in the URL as it does on the grid, the removed list and the duplicates
    queue — but only as the *first offer* to the Add form. Every widget stores the library it is
    about, so a dashboard is about as many libraries as somebody put on it, and this parameter
    changes nothing already on the page.
  */
  validateSearch: (search: Record<string, unknown>): { library?: string } =>
    typeof search.library === 'string' ? { library: search.library } : {},
})

function RouteComponent() {
  const { library } = Route.useSearch()
  return <DashboardPage library={(library as LibraryId | undefined) ?? DEFAULT_LIBRARY_ID} />
}

export function DashboardPage({ library }: { library: LibraryId }) {
  /**
   * Read once, from this browser. Not a query: there is no server to ask, and re-reading
   * `localStorage` on every render would let a second tab's arrangement replace one somebody is
   * in the middle of dragging.
   */
  const [layout, setLayout] = useState<StoredLayout>(() => readLayout(strings.dashboard.defaultGroup))

  /**
   * The page's one request.
   *
   * The key is constant — `['dashboard', 'resolve']` — and that is the point: a key carrying
   * positions or groups would re-resolve twelve widgets every time somebody nudged one. Adding
   * a widget or changing its settings asks for that key alone, below.
   *
   * `staleTime: Infinity` and no refetch on focus, because the event stream is what keeps this
   * current. Without them, every return to the tab would cost a second resolve of everything —
   * polling by another name, on a schedule set by how often somebody looks at the window.
   *
   * `refetchOnMount: 'always'` is the one exception, and it is not a timer: the `QueryClient` is
   * shared across the whole application and holds this key for five minutes after the page is
   * left. Coming back inside that window would otherwise show the *previous* visit's answer with
   * no request at all — and a widget added during that visit would sit on "Loading" for ever,
   * because its result only ever lived in `later`, which is component state. So each arrival asks
   * once, for the layout as it is now, and the values already cached stay on screen meanwhile.
   */
  const asked = useQuery({
    queryKey: ['dashboard', 'resolve'],
    queryFn: () => resolve(layout.widgets.map(requestOf)),
    // 0 keys is a 422, so an empty dashboard asks nothing at all.
    enabled: layout.widgets.length > 0,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnMount: 'always',
    retry: false,
  })

  /**
   * Later answers, over the first ones.
   *
   * A retry, a new widget and an event all resolve *some* keys, so their results are merged on
   * top of the first load's rather than replacing it — which is what a second `useQuery` per
   * widget would amount to.
   */
  const [later, setLater] = useState<Map<string, WidgetResult>>(new Map())
  const results = useMemo(
    () => new Map<string, WidgetResult>([...byKey(asked.data), ...later]),
    [asked.data, later],
  )

  /** Ask again for some of the keys, in one call. Never on a timer — see the module comment. */
  const askFor = (requests: readonly WidgetRequest[]) => {
    if (requests.length === 0) return
    void resolve(requests)
      .then((answer) => {
        setLater((before) => new Map([...before, ...byKey(answer)]))
      })
      .catch(() => {
        /*
          The widgets keep what they were showing. This is a re-ask of keys that already
          answered — an api that has just gone away is a reason to leave the last true figures
          on screen, not to blank eleven panels that were right a second ago. The first load's
          failure is the one that gets a message, below.
        */
      })
  }

  /**
   * A change to the arrangement: settled, written to this browser, and any widget that is new or
   * newly configured asked for.
   *
   * Every writer goes through here, which is why none of them works out a row: `settleAll` is
   * the one normaliser, so an added widget can say "last, wherever that is" and mean it.
   */
  const save = (next: StoredLayout) => {
    const settled = settleAll(next)
    writeLayout(settled)
    setLayout(settled)
    /*
      Nothing to ask for when the board was empty. The query was disabled with no widgets and is
      about to become enabled, which resolves the whole layout by itself — so asking here as well
      would make the very first widget somebody adds cost two requests, which is the one number
      this page is about.
    */
    if (layout.widgets.length === 0) return
    askFor(
      settled.widgets
        .filter((widget) => {
          const before = layout.widgets.find((entry) => entry.key === widget.key)
          return before === undefined || JSON.stringify(before.widget) !== JSON.stringify(widget.widget)
        })
        .map(requestOf),
    )
  }

  /*
    The server said something changed. Only that library's keys are asked for — plus every
    widget about the whole store, because a part added to any library moves those figures and
    there is no notification that says "the installation changed". One call for the lot.
  */
  useAppEvents(
    (libraries) => {
      askFor(
        layout.widgets
          .filter((widget) => {
            const of = libraryOf(widget.widget)
            return of === null || libraries.has(of)
          })
          .map(requestOf),
      )
    },
    () => askFor(layout.widgets.map(requestOf)),
  )

  return (
    <AppFrame
      current="dashboard"
      library={library}
      skipTo={{ href: '#dashboard', label: strings.dashboard.title }}
      actions={<AddWidget layout={layout} onLayout={save} library={library} />}
    >
      {/* Rendered, not assigned: React 19 hoists it, so the route that owns the page owns its title. */}
      <title>{strings.titles.dashboard}</title>
      {/*
        No reading measure on the board. The prose above it keeps one — `LEAD` is capped at 70ch
        — but a twelve-column grid inside a `max-w-3xl` would use the left 800px of a 1440
        window and wrap every heading inside it, which is exactly what the rig found on the
        duplicates queue.
      */}
      <div id="dashboard">
        <h2 className={HEADLINE}>{strings.dashboard.title}</h2>
        <p className={LEAD}>{strings.dashboard.lead}</p>
        <p className="mt-1 max-w-[70ch] text-xs text-[var(--color-muted)]">{strings.dashboard.perBrowser}</p>
        {layout.widgets.length === 0 ? (
          <section className="mt-10 border-t border-[var(--color-border)] pt-6">
            <h3 className="text-sm font-medium text-[var(--color-bright)]">{strings.dashboard.empty}</h3>
            <p className="mt-2 max-w-[70ch] text-sm text-[var(--color-dim)]">{strings.dashboard.emptyLead}</p>
          </section>
        ) : asked.isError ? (
          <section className="mt-10 border-t border-[var(--color-border)] pt-6">
            <p className="max-w-[70ch] text-sm text-[var(--color-dim)]">{strings.dashboard.loadFailed}</p>
            <button
              type="button"
              className="ease-mechanical mt-3 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-4 py-2 text-sm font-semibold text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px"
              onClick={() => void asked.refetch()}
            >
              {strings.dashboard.retry}
            </button>
          </section>
        ) : (
          <Board layout={layout} onLayout={save} results={results} onRetry={(key) => {
            const widget = layout.widgets.find((entry) => entry.key === key)
            if (widget !== undefined) askFor([requestOf(widget)])
          }} />
        )}
        {layout.widgets.length === 0 ? null : <AddGroup layout={layout} onLayout={save} />}
      </div>
    </AppFrame>
  )
}
