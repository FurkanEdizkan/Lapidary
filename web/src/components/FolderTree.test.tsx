import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { expect, test, vi } from 'vitest'
import { FolderTree, MovePartDialog, PART_DRAG_TYPE, partDragPayload } from './FolderTree'
import { strings } from '../lib/strings'
import type { FolderId, FolderNode } from '../lib/types'

/**
 * The tree renders without a router: the selection is a prop, and the route component is
 * what reads it out of the URL. `index.test.tsx` covers that half, where the router is.
 *
 * `fireEvent`, not `userEvent`: `@testing-library/user-event` is not a dependency of this
 * project and the plan's snippet assumed it was. Neither is `@testing-library/jest-dom`,
 * so assertions are plain vitest ones. Adding either to make a snippet compile would be a
 * dependency added for a test's convenience.
 */
const LIBRARY = '01931b6e-0000-7000-8000-000000000001'

const TERRAIN: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0001',
  parentId: null,
  name: 'Terrain',
  partCount: 34,
}
const ROCKS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0002',
  parentId: TERRAIN.id,
  name: 'Rocks',
  partCount: 12,
}
const FASTENERS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0003',
  parentId: null,
  name: 'Fasteners',
  partCount: 7,
}

/** The model being moved, in the shape a drag payload and the chooser both take. */
const CLIFF = {
  id: '01931b6e-0000-7000-8000-0000000a0007',
  name: 'Basalt cliff face, 180 mm span',
}

type StubResponse = { ok: boolean; status?: number; json?: () => Promise<unknown> }

const pending = () => new Promise<StubResponse>(() => {})

const ok = (body: unknown) => async (): Promise<StubResponse> => ({ ok: true, json: async () => body })

/** The name collision. An answer, not a failure — the client asks and re-sends. */
const conflict = async (): Promise<StubResponse> => ({ ok: false, status: 409, json: async () => ({}) })

/** One response per call, in order, for the ask-then-acknowledge pair. */
function inOrder(...responses: Array<() => Promise<StubResponse>>) {
  let index = 0
  return () => (responses[Math.min(index++, responses.length - 1)] ?? pending)()
}

/**
 * Dispatch on the URL and the method. Unstubbed routes hang rather than resolve, so no
 * test asserts against a body it did not ask for, and an unrecognised path rejects loudly
 * instead of pending forever.
 */
function stubFetch(routes: {
  folders?: () => Promise<StubResponse>
  move?: () => Promise<StubResponse>
  folderDelete?: () => Promise<StubResponse>
}) {
  const fetchMock = vi.fn((url: string, init?: { method?: string }) => {
    if (url.endsWith('/folders')) return (routes.folders ?? pending)()
    if (url.startsWith('/api/parts/') && init?.method === 'PATCH') return (routes.move ?? pending)()
    if (url.startsWith('/api/folders/') && init?.method === 'DELETE') {
      return (routes.folderDelete ?? pending)()
    }
    return Promise.reject(new Error(`unstubbed request: ${url}`))
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function newClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function renderTree(selected: FolderId | null = null) {
  const onSelect = vi.fn()
  render(
    <QueryClientProvider client={newClient()}>
      <FolderTree library={LIBRARY} selected={selected} onSelect={onSelect} />
    </QueryClientProvider>,
  )
  return { onSelect }
}

/** What a card puts on the drag, read back the way a folder row reads it. */
const draggingCliff = {
  dataTransfer: { getData: (type: string) => (type === PART_DRAG_TYPE ? partDragPayload(CLIFF) : '') },
}

const moveCalls = (fetchMock: ReturnType<typeof stubFetch>) =>
  fetchMock.mock.calls.filter(([url]) => String(url).startsWith('/api/parts/'))

/** The body of the nth move, parsed. Counted among the moves, not among every request. */
function moveBody(fetchMock: ReturnType<typeof stubFetch>, index: number): unknown {
  const init = moveCalls(fetchMock)[index]?.[1] as { body?: string } | undefined
  return JSON.parse(init?.body ?? 'null')
}

test('nests a child under its parent and reports the category that was selected', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS, FASTENERS]) })
  const { onSelect } = renderTree()

  const terrain = await screen.findByRole('button', { name: 'Terrain' })
  const rocks = screen.getByRole('button', { name: 'Rocks' })
  // Nesting is structural, not a left margin: `Rocks` lives inside `Terrain`'s own list
  // item. An indented sibling would look identical and be a different tree.
  expect(terrain.closest('li')?.contains(rocks)).toBe(true)
  expect(screen.getByRole('button', { name: 'Fasteners' }).closest('li')?.contains(terrain)).toBe(
    false,
  )

  fireEvent.click(rocks)
  expect(onSelect).toHaveBeenCalledWith(ROCKS.id)
  fireEvent.click(screen.getByRole('button', { name: strings.folders.root }))
  // The library unfiltered is null, never an id and never an empty string: the route reads
  // an absent parameter as the whole library.
  expect(onSelect).toHaveBeenLastCalledWith(null)
})

test('dropping a card on a category files it there, and ignores anything else dropped', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN, ROCKS]), move: ok({}) })
  renderTree()

  const rocks = await screen.findByRole('button', { name: 'Rocks' })
  fireEvent.dragOver(rocks)
  fireEvent.drop(rocks, draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  expect(fetchMock).toHaveBeenCalledWith(
    `/api/parts/${CLIFF.id}`,
    expect.objectContaining({ method: 'PATCH' }),
  )
  // Both fields, always. The route defaults the flag, but a request that omitted it would
  // be indistinguishable on the wire from one that meant false.
  expect(moveBody(fetchMock, 0)).toEqual({
    folderId: ROCKS.id,
    acknowledgeDuplicate: false,
  })

  // Text dragged in from another window carries no payload of ours and moves nothing.
  fireEvent.drop(rocks, { dataTransfer: { getData: () => '' } })
  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
})

test('dropping a card on All models files it under no category at all', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: ok({}) })
  renderTree(TERRAIN.id)

  fireEvent.drop(screen.getByRole('button', { name: strings.folders.root }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  // `null` is a target — the library root — and is not the same as omitting the field.
  expect(moveBody(fetchMock, 0)).toEqual({ folderId: null, acknowledgeDuplicate: false })
})

test('a name collision asks once, and confirming re-sends the same move acknowledged', async () => {
  const fetchMock = stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    move: inOrder(conflict, ok({})),
  })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Rocks' }), draggingCliff)

  const dialog = await screen.findByRole('dialog')
  // Names the model, so the user knows which collision they are being asked about.
  expect(within(dialog).getByText(strings.folders.duplicateTitle(CLIFF.name))).toBeDefined()
  expect(within(dialog).getByText(strings.folders.duplicateBody)).toBeDefined()

  fireEvent.click(within(dialog).getByRole('button', { name: strings.folders.duplicateConfirm }))

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(2))
  const [first, second] = moveCalls(fetchMock).map(
    (call) => JSON.parse((call[1] as { body: string }).body) as unknown,
  )
  expect(first).toEqual({ folderId: ROCKS.id, acknowledgeDuplicate: false })
  // The SAME target, acknowledged. A re-send that lost the folder would move the model
  // somewhere the user never chose.
  expect(second).toEqual({ folderId: ROCKS.id, acknowledgeDuplicate: true })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
})

test('a refusal that survives the acknowledgement becomes a note, not the dialog again', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: inOrder(conflict, conflict) })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)
  fireEvent.click(
    within(await screen.findByRole('dialog')).getByRole('button', {
      name: strings.folders.duplicateConfirm,
    }),
  )

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(2))
  // The route answers 409 for a model still migrating and for a cross-library target as
  // well, so a second refusal is not the collision again — re-opening the same dialog
  // would be a loop with no exit.
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(screen.getByText(strings.folders.moveRefused)).toBeDefined()
})

test('the delete confirmation names the models in the subtree and what stays on disk', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN, ROCKS]), folderDelete: ok(null) })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )

  const dialog = screen.getByRole('dialog')
  expect(within(dialog).getByText(strings.folders.deleteTitle(TERRAIN.name))).toBeDefined()
  // The count itself is on screen — a generic "Are you sure?" is what this requirement
  // exists to rule out — and it is the subtree count, because the delete cascades.
  expect(dialog.textContent).toContain(String(TERRAIN.partCount))
  expect(within(dialog).getByText(strings.folders.deleteBody(TERRAIN.partCount))).toBeDefined()
  // Soft delete, and the copy has to keep it apart from a purge and from cache eviction:
  // the models are hidden, and nothing leaves the storage folder.
  expect(dialog.textContent).toContain('nothing is removed from your storage folder')
  expect(dialog.textContent).not.toMatch(/delete[sd]? the files|erase|remove the files/i)

  fireEvent.click(within(dialog).getByRole('button', { name: strings.folders.deleteConfirm }))
  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(`/api/folders/${TERRAIN.id}`, { method: 'DELETE' }),
  )
})

test('deleting the category the grid is filtered to clears the filter', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS]), folderDelete: ok(null) })
  // Filtered to the child, deleting the parent: the delete cascades, so the selection is
  // gone too and a grid still asking about it would come back empty with no reason shown.
  const { onSelect } = renderTree(ROCKS.id)

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )
  fireEvent.click(
    within(screen.getByRole('dialog')).getByRole('button', { name: strings.folders.deleteConfirm }),
  )

  await waitFor(() => expect(onSelect).toHaveBeenCalledWith(null))
})

test('the move chooser files a model with no drag at all', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN, ROCKS, FASTENERS]), move: ok({}) })
  const onClose = vi.fn()
  render(
    <QueryClientProvider client={newClient()}>
      <MovePartDialog part={CLIFF} library={LIBRARY} onClose={onClose} />
    </QueryClientProvider>,
  )

  // Every category is a target, the library root included, and every one of them is a
  // plain button: reachable by tab, activated by Enter, with no arrow-key navigation
  // promised. The drag path is unusable from a keyboard, so this one has to be complete.
  const rocks = await screen.findByRole('button', { name: strings.folders.moveInto(ROCKS.name) })
  expect(screen.getByRole('button', { name: strings.folders.moveInto(TERRAIN.name) })).toBeDefined()
  expect(
    screen.getByRole('button', { name: strings.folders.moveInto(strings.folders.root) }),
  ).toBeDefined()
  expect(rocks.tagName).toBe('BUTTON')

  fireEvent.click(rocks)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  expect(moveBody(fetchMock, 0)).toEqual({ folderId: ROCKS.id, acknowledgeDuplicate: false })
  await waitFor(() => expect(onClose).toHaveBeenCalled())
})

test('the chooser asks about a collision too, and keeps the target it was given', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: inOrder(conflict, ok({})) })
  render(
    <QueryClientProvider client={newClient()}>
      <MovePartDialog part={CLIFF} library={LIBRARY} onClose={vi.fn()} />
    </QueryClientProvider>,
  )

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.moveInto(TERRAIN.name) }),
  )
  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.duplicateConfirm }),
  )

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(2))
  expect(moveBody(fetchMock, 1)).toEqual({ folderId: TERRAIN.id, acknowledgeDuplicate: true })
})

test('a library with no categories says so instead of showing an empty tree', async () => {
  stubFetch({ folders: ok([]) })
  renderTree()

  expect(await screen.findByText(strings.folders.empty)).toBeDefined()
  // The unfiltered library is still a target, so a model can always be filed back out of
  // a category.
  expect(screen.getByRole('button', { name: strings.folders.root })).toBeDefined()
})
