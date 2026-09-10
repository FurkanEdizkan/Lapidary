import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { DEFAULT_LIBRARY_ID } from '../lib/api'
import { Organize } from './organize'
import { strings } from '../lib/strings'
import type { FolderNode, PartCard } from '../lib/types'

/**
 * Filing in bulk — the one piece of genuinely new behaviour on this screen.
 *
 * Everything else here is components the grid already ships and `index.test.tsx` already
 * covers: the tree, its drag targets, its create and rename dialogs. What is new is the
 * loop that moves a selection, and specifically what it does with a model the server
 * refuses — which is the case that cannot be seen from the API tests, because
 * `PATCH /api/parts/{id}` behaves identically whether one caller or fifty made the call.
 */

const FIXTURE = {
  library: DEFAULT_LIBRARY_ID,
  thumbnail: null,
  approximate: true,
  tessellationL0: null,
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceBytes: 204_800,
  storedBytes: 91_204,
  compressed: true,
  createdAt: '2026-09-06T10:00:00Z',
  updatedAt: '2026-09-06T10:00:00Z',
} as const

const SHAFT_COLLAR: PartCard = {
  ...FIXTURE,
  id: '01931b6e-0000-7000-8000-0000000a0001',
  revision: '01931b6e-0000-7000-8000-0000000b0001',
  name: 'Shaft collar, 8 mm bore',
  partNumber: 'LP-5120-A',
  sourcePath: 'drivetrain/LP-5120-A-collar.stl',
  triangleCount: 4_820,
  // A 16 mm collar on an 8 mm shaft, 9 mm wide, less the bore and the clamp slot.
  bboxMm: [16.0, 16.0, 9.0],
  volumeMm3: 1_318.0,
  directory: 'libraries/default/drivetrain/shaft_collar_8_mm_bore',
  storagePath: 'libraries/default/drivetrain/shaft_collar_8_mm_bore/shaft_collar_8_mm_bore.stl',
}

const IDLER_PULLEY: PartCard = {
  ...FIXTURE,
  id: '01931b6e-0000-7000-8000-0000000a0002',
  revision: '01931b6e-0000-7000-8000-0000000b0002',
  name: 'Idler pulley, GT2 20T',
  partNumber: 'LP-5133-B',
  sourcePath: 'drivetrain/LP-5133-B-idler.stl',
  triangleCount: 9_640,
  bboxMm: [12.2, 12.2, 13.0],
  volumeMm3: 942.5,
  directory: 'libraries/default/drivetrain/idler_pulley_gt2_20t',
  storagePath: 'libraries/default/drivetrain/idler_pulley_gt2_20t/idler_pulley_gt2_20t.stl',
}

const DRIVETRAIN: FolderNode = {
  id: '01931b6e-0000-7000-8000-0000000c0001',
  name: 'Drivetrain',
  parentId: null,
  // The path the server derives from the name, which the tree shows and never rebuilds.
  slug: 'drivetrain',
  partCount: 2,
}

/**
 * Every request the page makes, and what each `PATCH` gets answered with.
 *
 * `refuse` names the part ids the move route should answer `409 duplicateName` for, which
 * is the shape the real route uses — see `movePart`. Returning the refusal rather than a
 * failure is the point: a refused move is an *answer*, and the page has to keep the model
 * where it was rather than treat it as an error that stops the run.
 */
function stub({ refuse = new Set<string>() }: { refuse?: ReadonlySet<string> } = {}) {
  const calls: { url: string; method: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET'
      const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
      calls.push({ url, method, body })
      if (method === 'PATCH') {
        const refused = [...refuse].some((id) => url.includes(id))
        if (refused) {
          return {
            ok: false,
            status: 409,
            json: async () => ({ reason: 'duplicateName' }),
          }
        }
        return { ok: true, status: 200, json: async () => ({}) }
      }
      if (url.includes('/folders')) {
        return { ok: true, status: 200, json: async () => [DRIVETRAIN] }
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({ parts: [SHAFT_COLLAR, IDLER_PULLEY], next: null }),
      }
    }),
  )
  return calls
}

function renderPage() {
  const rootRoute = createRootRoute({
    component: () => <Organize library={DEFAULT_LIBRARY_ID} />,
  })
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

async function tick(name: string): Promise<void> {
  fireEvent.click(await screen.findByRole('checkbox', { name: new RegExp(name) }))
}

/**
 * The plain path: tick two, choose a category, and two `PATCH`es go out — one per model,
 * because there is no bulk route and `Promise.all` against a route that renames a directory
 * per call is a thundering herd rather than a feature.
 *
 * The bodies are asserted, not just the count. A loop wired to `loaded[0]` would send two
 * requests and pass a count-only assertion while moving one model twice.
 */
test('filing a selection moves each picked model into the chosen category', async () => {
  const calls = stub()
  renderPage()

  await tick(SHAFT_COLLAR.name)
  await tick(IDLER_PULLEY.name)

  fireEvent.change(screen.getByRole('combobox', { name: strings.organize.fileInto }), {
    target: { value: DRIVETRAIN.id },
  })

  await waitFor(() => expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(2))
  const moved = calls.filter((call) => call.method === 'PATCH')
  expect(moved.map((call) => call.url)).toEqual([
    `/api/parts/${SHAFT_COLLAR.id}`,
    `/api/parts/${IDLER_PULLEY.id}`,
  ])
  // Never `acknowledgeDuplicate: true` from a bulk move — see `file` in `organize.tsx`. A
  // run of fifty must not be able to overwrite a collision nobody was shown.
  for (const call of moved) {
    expect(call.body).toEqual({ folderId: DRIVETRAIN.id, acknowledgeDuplicate: false })
  }
})

/**
 * **The case this file exists for.** One model of the two is refused for a duplicate name.
 *
 * Three things have to hold at once, and each of them is a different way to get this wrong:
 * the run does not stop at the refusal, the report says both numbers, and the refused model
 * stays ticked — because the selection is the only record of which models still need a
 * decision, and clearing it would throw that away.
 */
test('a model refused for a duplicate name stays put, stays selected, and is reported', async () => {
  const calls = stub({ refuse: new Set([IDLER_PULLEY.id]) })
  renderPage()

  await tick(SHAFT_COLLAR.name)
  await tick(IDLER_PULLEY.name)
  fireEvent.change(screen.getByRole('combobox', { name: strings.organize.fileInto }), {
    target: { value: DRIVETRAIN.id },
  })

  // Both were attempted: a loop that returned early on the first refusal would send one.
  await waitFor(() => expect(calls.filter((call) => call.method === 'PATCH')).toHaveLength(2))

  expect(await screen.findByRole('status')).toHaveProperty(
    'textContent',
    strings.organize.filed(1, 1),
  )
  // The one that moved is unticked; the one that did not is still ticked and still there.
  expect(
    (screen.getByRole('checkbox', { name: new RegExp(SHAFT_COLLAR.name) }) as HTMLInputElement)
      .checked,
  ).toBe(false)
  expect(
    (screen.getByRole('checkbox', { name: new RegExp(IDLER_PULLEY.name) }) as HTMLInputElement)
      .checked,
  ).toBe(true)
})

/**
 * A refusal is not data loss and the copy may not read as though it were.
 *
 * `CLAUDE.md` treats a non-destructive outcome worded as one as a correctness bug, and this
 * is the sentence a person reads after filing forty models — so it is asserted as text
 * rather than left to whoever edits `strings.ts` next.
 */
test('the filing report says what was left alone rather than what failed', () => {
  expect(strings.organize.filed(39, 1)).toBe(
    'Filed 39 models. 1 kept its place — a model of that name is already in that category. It is still selected.',
  )
  // A clean run says nothing about refusals: "and 0 were left behind" invites the reader to
  // work out whether that is good news.
  expect(strings.organize.filed(39, 0)).toBe('Filed 39 models.')
})
