/*
  The context a Lapidary screen expects, for claude.ai/design — and for nothing in the app.

  `AppFrame`, `Card` and `Crash` render TanStack Router `Link`s, and `JumpSearch` inside the frame
  reads the router, so they throw outside a router. The data components read TanStack Query. The
  app provides both from `main.tsx`; a design has neither, so this does it once: a memory router
  whose one route renders whatever is inside, and a query client that never goes to the network.

  Queries are off rather than pointed at a fake server. A data component with nothing seeded shows
  its own loading state — the honest thing for it to show. To draw one with data, pass `seed`: each
  entry is a query key the component reads and the answer it should find there, e.g.
  `[['folders', library], [...categories]]` for `FolderTree`. The key is the component's own; read
  it from the component's source rather than guessing.

  Outside `src/`, as `.ds-entry.tsx` is: tsconfig's `include` never sees it, so the app build pays
  nothing, and `src/no-bare-strings.test.ts` has no string here to hold to `strings.ts`.
*/
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { createContext, useContext, useState, type ReactNode } from 'react'

/** What the router's one route renders: the children of the nearest `DesignProviders`. */
const Slot = createContext<ReactNode>(null)

const root = createRootRoute({ component: () => <>{useContext(Slot)}</> })

/**
 * Wrap anything that uses `AppFrame`, `Card`, `Crash` or a data component. Everything else in
 * the system renders without it.
 */
export function DesignProviders({
  children,
  seed = [],
}: {
  children: ReactNode
  /** Answers already in hand, by query key, so a data component draws with them. Read once. */
  seed?: ReadonlyArray<readonly [readonly unknown[], unknown]>
}) {
  const [client] = useState(() => {
    const client = new QueryClient({
      defaultOptions: { queries: { enabled: false, retry: false }, mutations: { retry: false } },
    })
    for (const [key, answer] of seed) client.setQueryData(key, answer)
    return client
  })
  const [router] = useState(() =>
    createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: ['/'] }) }),
  )
  return (
    <QueryClientProvider client={client}>
      <Slot.Provider value={children}>
        <RouterProvider router={router} />
      </Slot.Provider>
    </QueryClientProvider>
  )
}
