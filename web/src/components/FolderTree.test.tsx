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
  slug: 'Terain',
  partCount: 34,
}
const ROCKS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0002',
  parentId: TERRAIN.id,
  name: 'Rocks',
  slug: 'Rocks',
  partCount: 12,
}
const FASTENERS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0003',
  parentId: null,
  name: 'Fasteners',
  slug: 'Fasteners',
  partCount: 7,
}
/** One model and no children: the singular branch of the delete confirmation. */
const CABLE_CLIPS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0004',
  parentId: null,
  name: 'Cable clips',
  slug: 'Cable clips',
  partCount: 1,
}
/**
 * No models and two subcategories, which is the shape the delete copy used to lie about:
 * "No models are inside it. Nothing is removed…", and then two rows leave the sidebar.
 */
const ENCLOSURES: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0005',
  parentId: null,
  name: 'Enclosures',
  slug: 'Enclosures',
  partCount: 0,
}
const VENTS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0006',
  parentId: ENCLOSURES.id,
  name: 'Vent grilles',
  slug: 'Vent grilles',
  partCount: 0,
}
const LIDS: FolderNode = {
  id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0007',
  parentId: ENCLOSURES.id,
  name: 'Snap-fit lids',
  slug: 'Snap-fit lids',
  partCount: 0,
}

/** The model being moved, in the shape a drag payload and the chooser both take. */
const CLIFF = {
  id: '01931b6e-0000-7000-8000-0000000a0007',
  name: 'Basalt cliff face, 180 mm span',
}

type StubResponse = { ok: boolean; status?: number; json?: () => Promise<unknown> }

const pending = () => new Promise<StubResponse>(() => {})

const ok = (body: unknown) => async (): Promise<StubResponse> => ({ ok: true, json: async () => body })

/**
 * A `409` refusal, with the `reason` the route would carry — or without one, to stand in
 * for an old server or a body this client cannot make sense of. Only `'duplicateName'` is
 * an answer a retry can act on; every other reason, `undefined` included, is a dead end.
 */
const conflict =
  (reason?: string) =>
  async (): Promise<StubResponse> => ({
    ok: false,
    status: 409,
    json: async () => (reason === undefined ? {} : { reason }),
  })

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
  folderCreate?: () => Promise<StubResponse>
  folderPatch?: () => Promise<StubResponse>
}) {
  const fetchMock = vi.fn((url: string, init?: { method?: string }) => {
    // Before the bare `/folders` arm: a create is a POST to the same path the tree is a
    // GET of, so order is what tells them apart and not the URL.
    if (url.endsWith('/folders') && init?.method === 'POST') return (routes.folderCreate ?? pending)()
    if (url.endsWith('/folders')) return (routes.folders ?? pending)()
    if (url.startsWith('/api/parts/') && init?.method === 'PATCH') return (routes.move ?? pending)()
    if (url.startsWith('/api/folders/') && init?.method === 'DELETE') {
      return (routes.folderDelete ?? pending)()
    }
    if (url.startsWith('/api/folders/') && init?.method === 'PATCH') {
      return (routes.folderPatch ?? pending)()
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

/**
 * What a card puts on the drag, read back the way a folder row reads it.
 *
 * `types` as well as `getData`, because that is the pair a real drag exposes: the payload
 * is withheld until the drop, and the type list is all `dragover` can read — which is where
 * a row decides whether it is a target at all.
 */
const draggingCliff = {
  dataTransfer: {
    types: [PART_DRAG_TYPE],
    getData: (type: string) => (type === PART_DRAG_TYPE ? partDragPayload(CLIFF) : ''),
  },
}

/** A drag from outside the app: files off a desktop, carrying nothing of ours. */
const draggingFiles = {
  dataTransfer: { types: ['Files'], getData: () => '' },
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
  // `fireEvent` returns false when a handler called `preventDefault()`, and calling it on
  // `dragover` is the only thing that marks an element as willing to take a drop. jsdom
  // fires `drop` either way, so without this the call could be deleted and every drag test
  // here would stay green while drag-to-move stopped working in every real browser.
  expect(fireEvent.dragOver(rocks, draggingCliff)).toBe(false)
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

  // Files dragged in from a desktop are not offered a target at all: `preventDefault()` is
  // withheld, so the browser never fires a drop here and the row does not advertise itself
  // as somewhere to put them. The drop below is what jsdom will deliver regardless, and it
  // moves nothing either.
  expect(fireEvent.dragOver(rocks, draggingFiles)).toBe(true)
  fireEvent.drop(rocks, draggingFiles)
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
    move: inOrder(conflict('duplicateName'), ok({})),
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
  const fetchMock = stubFetch({
    folders: ok([TERRAIN]),
    // Realistic race: the target is deleted between the ask and the confirm, so the retry
    // comes back `noSuchFolder` rather than the collision again.
    move: inOrder(conflict('duplicateName'), conflict('noSuchFolder')),
  })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)
  fireEvent.click(
    within(await screen.findByRole('dialog')).getByRole('button', {
      name: strings.folders.duplicateConfirm,
    }),
  )

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(2))
  // The retry can surface a different reason than the one it acknowledged, so a refusal
  // that survives the acknowledgement is not the collision again — re-opening the same
  // dialog would be a loop with no exit.
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(screen.getByText(strings.folders.noSuchFolderRefusal)).toBeDefined()
})

test('a target in another library refuses the move without offering to retry', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: conflict('crossLibrary') })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  // No dialog: acknowledging cannot move a category into this library, so the fix is a
  // different target, not a retry of the same one.
  expect(screen.getByText(strings.folders.crossLibraryRefusal)).toBeDefined()
  expect(screen.queryByRole('dialog')).toBeNull()
})

test('a model still migrating refuses the move without offering to retry', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: conflict('migrationPending') })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  // Same wording the card's own "not movable yet" state uses. No dialog: acknowledging
  // cannot finish a migration, so offering "move anyway" would invite a retry that can
  // never succeed.
  expect(screen.getByText(strings.folders.notMigrated)).toBeDefined()
  expect(screen.queryByRole('dialog')).toBeNull()
})

test('a folder that no longer exists refuses the move without offering to retry', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: conflict('noSuchFolder') })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  expect(screen.getByText(strings.folders.noSuchFolderRefusal)).toBeDefined()
  expect(screen.queryByRole('dialog')).toBeNull()
})

test('a 409 with no reason this client recognises is a dead end, not a guessed duplicate', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: conflict() })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  // An absent `reason` — an old server, or a body this client cannot make sense of — must
  // never open the acknowledge-and-retry dialog: that would invite the user to confirm a
  // move that can never succeed, for a refusal this client cannot even name.
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(screen.getByText(strings.folders.moveRefused)).toBeDefined()
})

test('a 409 with a reason this client does not recognise is a dead end too', async () => {
  // Distinct from the absent-`reason` case above: this exercises the branch where a
  // `reason` is present but is not one of the four names this client knows, rather than
  // the branch where the field is missing outright.
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: conflict('teapot') })
  renderTree()

  fireEvent.drop(await screen.findByRole('button', { name: 'Terrain' }), draggingCliff)

  await waitFor(() => expect(moveCalls(fetchMock)).toHaveLength(1))
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(screen.getByText(strings.folders.moveRefused)).toBeDefined()
})

test('the delete confirmation names the models, the subcategories, and what stays on disk', async () => {
  const fetchMock = stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    folderDelete: ok({ foldersHidden: 2, partsHidden: TERRAIN.partCount }),
  })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )

  const dialog = screen.getByRole('dialog')
  expect(within(dialog).getByText(strings.folders.deleteTitle(TERRAIN.name))).toBeDefined()
  const body = dialog.textContent ?? ''
  // Literal wording, deliberately, and not `deleteBody(TERRAIN.partCount)` again: an
  // assertion that renders the copy through the same function that produced it passes
  // just as happily over copy that has been gutted. Every phrase below is one this
  // confirmation cannot lose without changing what it promises.
  //
  // The counts are both on screen — a generic "Are you sure?" is what this requirement
  // exists to rule out — and the model count is the subtree count, because the delete
  // cascades through subcategories exactly as the subcategory count says it does.
  expect(body).toContain('34 models and 1 subcategory are inside it')
  expect(body).toContain('counting every level')
  // Soft delete, and the copy has to keep it apart from a purge and from cache eviction:
  // the models are marked deleted and hidden, and nothing leaves the storage folder.
  expect(body).toContain('marks them deleted')
  expect(body).toContain('hides them from the grid')
  expect(body).toContain('hides the subcategory under it')
  expect(body).toContain('nothing is removed from your storage folder')
  expect(body).toContain('no file moves on disk')
  expect(body).not.toMatch(/delete[sd]? the files|erase|remove the files/i)

  fireEvent.click(within(dialog).getByRole('button', { name: strings.folders.deleteConfirm }))
  await waitFor(() =>
    expect(fetchMock).toHaveBeenCalledWith(`/api/folders/${TERRAIN.id}`, { method: 'DELETE' }),
  )
})

/**
 * The two branches nothing rendered before, which is how both of them came to be wrong.
 *
 * The singular was an explicit requirement and was written correctly; it was simply never
 * exercised. The empty category was not: `soft_delete_subtree` marks every descendant
 * FOLDER deleted as well, so a category holding nothing but subcategories confirmed with
 * "No models are inside it. Nothing is removed…" and then took every one of those rows off
 * the sidebar — a destructive confirmation naming none of what it destroys.
 */
test('the singular reads as one, and an empty category still names the subcategories that go', async () => {
  stubFetch({ folders: ok([CABLE_CLIPS, ENCLOSURES, VENTS, LIDS]) })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(CABLE_CLIPS.name) }),
  )
  const one = screen.getByRole('dialog').textContent ?? ''
  expect(one).toContain('1 model and no subcategories are inside it')
  expect(one).toContain('marks that model deleted and hides it from the grid')
  expect(one).not.toContain('1 models')
  expect(one).not.toContain('them from the grid')
  fireEvent.click(
    within(screen.getByRole('dialog')).getByRole('button', { name: strings.folders.cancel }),
  )

  fireEvent.click(screen.getByRole('button', { name: strings.folders.deleteFor(ENCLOSURES.name) }))
  const empty = screen.getByRole('dialog').textContent ?? ''
  // Two subcategories, neither holding a model, and the dialog says so rather than
  // reporting the category as empty.
  expect(empty).toContain('No models and 2 subcategories are inside it')
  expect(empty).toContain('hides this category and the subcategories under it')
  expect(empty).toContain('nothing is removed from your storage folder')
})

/**
 * A `404 noSuchFolder` is an answer, not an outage — the same shape as the move's `409`,
 * off the same route family and the same `refused` helper. Throwing on it landed the user
 * on "Check that the api service is running, then try again", which is wrong in both
 * halves: the service answered, and a retry can only 404 again.
 */
test('a category that is already gone says so instead of blaming the api service', async () => {
  const fetchMock = stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    folderDelete: async () => ({
      ok: false,
      status: 404,
      json: async () => ({
        reason: 'noSuchFolder',
        message: 'There is no category with that id. It may have been deleted.',
      }),
    }),
  })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )
  fireEvent.click(
    within(screen.getByRole('dialog')).getByRole('button', { name: strings.folders.deleteConfirm }),
  )

  expect(await screen.findByText(strings.folders.deleteGone)).toBeDefined()
  expect(screen.queryByText(strings.folders.deleteFailed)).toBeNull()
  // Announced, not merely present in the DOM somewhere.
  expect(screen.getByRole('alert').textContent).toBe(strings.folders.deleteGone)
  // And the row that is already gone is on its way off screen: the tree is re-read rather
  // than left showing a category the server says does not exist.
  await waitFor(() =>
    expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/folders'))).toHaveLength(2),
  )
})

/**
 * The count in the confirmation is read when it opens, and the library can move underneath
 * an open dialog. The route answers `{ foldersHidden, partsHidden }` precisely so the
 * caller can check what it warned about against what happened; the previous version threw
 * that body away.
 */
test('a delete that hid different numbers than it promised reports the ones that happened', async () => {
  stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    // A scan filed seven more models under Terrain, and two more subcategories with them,
    // between the dialog opening and the confirm landing.
    folderDelete: ok({ foldersHidden: 4, partsHidden: 41 }),
  })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )
  fireEvent.click(
    within(screen.getByRole('dialog')).getByRole('button', { name: strings.folders.deleteConfirm }),
  )

  // `foldersHidden` counts the category itself along with its descendants, so four rows
  // hidden is three subcategories — the dialog had named one.
  const note = await screen.findByText(strings.folders.deleteCountsDiffered(41, 3))
  expect(note.textContent).toContain('41 models and 3 subcategories were hidden')
  expect(note.getAttribute('role')).toBe('alert')
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
  const fetchMock = stubFetch({ folders: ok([TERRAIN]), move: inOrder(conflict('duplicateName'), ok({})) })
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

/**
 * Focus, which no test here used to read at all — every `autoFocus` in this file could have
 * been deleted and all of them stayed green.
 *
 * Two claims, and the second is the one that was broken: a dialog takes focus onto the safe
 * control when it opens, and hands it back to whatever opened it when it closes. Landing on
 * `<body>` costs a keyboard user their place in a sidebar that is hundreds of rows long.
 */
test('the delete confirmation takes focus onto cancel and hands it back to the row that opened it', async () => {
  stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    folderDelete: ok({ foldersHidden: 2, partsHidden: TERRAIN.partCount }),
  })
  renderTree()

  const trigger = await screen.findByRole('button', {
    name: strings.folders.deleteFor(TERRAIN.name),
  })
  // A real activation focuses the control first; `fireEvent.click` does not, and the
  // trigger is precisely what focus has to come back to.
  trigger.focus()
  fireEvent.click(trigger)

  const dialog = screen.getByRole('dialog')
  const cancel = within(dialog).getByRole('button', { name: strings.folders.cancel })
  // Cancel, never the destructive control: this dialog's safe answer is to do nothing.
  expect(document.activeElement).toBe(cancel)

  fireEvent.click(cancel)
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(trigger)
})

/**
 * The break the findings describe: pressing an action disables the button that had focus,
 * a disabled button loses focus, and focus falls to `<body>` — which is not a descendant of
 * the overlay, so an `onKeyDown` there stops receiving keys and the dialog becomes
 * keyboard-undismissable exactly while a request is in flight or has just been refused.
 *
 * Both halves are pinned. Focus is pulled back into the dialog rather than left nowhere,
 * and Escape is heard even when the key event never touches the overlay's own subtree —
 * which is what an overlay-mounted handler could not do and a document-mounted one can.
 */
test('a chooser whose rows are all disabled keeps focus, and Escape still closes it', async () => {
  stubFetch({ folders: ok([TERRAIN]), move: pending })
  const onClose = vi.fn()
  render(
    <QueryClientProvider client={newClient()}>
      <MovePartDialog part={CLIFF} library={LIBRARY} onClose={onClose} />
    </QueryClientProvider>,
  )

  const terrain = await screen.findByRole('button', { name: strings.folders.moveInto(TERRAIN.name) })
  terrain.focus()
  expect(document.activeElement).toBe(terrain)
  fireEvent.click(terrain)
  // The move never settles, so every row stays disabled — the window this finding is about.
  await waitFor(() => expect(terrain.hasAttribute('disabled')).toBe(true))
  // jsdom does not blur an element it disables; a browser does, and this is the state a
  // browser leaves behind.
  terrain.blur()

  const dialog = screen.getByRole('dialog')
  expect(dialog.contains(document.activeElement)).toBe(true)
  expect(document.activeElement).not.toBe(document.body)

  // Dispatched outside the overlay on purpose: a handler mounted on the overlay hears
  // nothing here, which is exactly how Escape used to die with the focus.
  fireEvent.keyDown(document.body, { key: 'Escape' })
  expect(onClose).toHaveBeenCalled()
})

/**
 * `aria-modal="true"` asserts a modality, and nothing was enforcing it: Tab from the last
 * control walked into the next card's buttons behind an opaque scrim, still activatable.
 * The portal is half the fix — the dialog is no longer a descendant of the card it was
 * opened from — and this is the other half.
 */
test('Tab stays inside the dialog and comes back when focus has fallen out of it', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS]), folderDelete: pending })
  renderTree()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }),
  )
  const dialog = screen.getByRole('dialog')
  const cancel = within(dialog).getByRole('button', { name: strings.folders.cancel })
  const confirm = within(dialog).getByRole('button', { name: strings.folders.deleteConfirm })

  confirm.focus()
  fireEvent.keyDown(document, { key: 'Tab' })
  expect(document.activeElement).toBe(cancel)

  fireEvent.keyDown(document, { key: 'Tab', shiftKey: true })
  expect(document.activeElement).toBe(confirm)

  // Focus on the box itself — the shape a disabled control leaves behind — and the next
  // Tab returns it to a control instead of making the user walk in from the top of the
  // page.
  dialog.focus()
  fireEvent.keyDown(document, { key: 'Tab' })
  expect(document.activeElement).toBe(cancel)
})

/**
 * A delete that fails leaves its confirmation open, so the failure belongs in the dialog
 * the user is looking at rather than on a row behind the scrim — and the row's own note
 * from an earlier action has to get out of the way, since a mutation's error state lives
 * until its own next run and would otherwise be the only thing that ever spoke for that
 * row.
 */
test('a delete that fails says so in the dialog, and clears what the row said before', async () => {
  stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    move: conflict('crossLibrary'),
    folderDelete: async () => ({ ok: false, status: 500 }),
  })
  renderTree()

  const terrain = await screen.findByRole('button', { name: 'Terrain' })
  fireEvent.drop(terrain, draggingCliff)
  expect(await screen.findByText(strings.folders.crossLibraryRefusal)).toBeDefined()

  fireEvent.click(screen.getByRole('button', { name: strings.folders.deleteFor(TERRAIN.name) }))
  fireEvent.click(
    within(screen.getByRole('dialog')).getByRole('button', { name: strings.folders.deleteConfirm }),
  )

  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByText(strings.folders.deleteFailed)).toBeDefined()
  // Still open, because nothing was deleted and the confirmation is where the retry is.
  expect(within(dialog).getByRole('button', { name: strings.folders.deleteConfirm })).toBeDefined()
  expect(screen.queryByText(strings.folders.crossLibraryRefusal)).toBeNull()
})

/**
 * A refusal that nobody is looking at is a refusal nobody gets. Every one of these notes
 * was a plain muted `<p>` at the foot of the whole `<nav>` — no role, no announcement, and
 * arbitrarily far from the row that was dropped on.
 */
test('a refused move is announced, and under the row it was refused for', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS]), move: conflict('crossLibrary') })
  renderTree()

  const rocks = await screen.findByRole('button', { name: 'Rocks' })
  fireEvent.drop(rocks, draggingCliff)

  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toBe(strings.folders.crossLibraryRefusal)
  // The row it belongs to, not the bottom of the sidebar: the note is inside the list item
  // for the category that refused the drop.
  expect(alert.closest('li')?.contains(rocks)).toBe(true)
})

test('a library with no categories says so instead of showing an empty tree', async () => {
  stubFetch({ folders: ok([]) })
  renderTree()

  expect(await screen.findByText(strings.folders.empty)).toBeDefined()
  // The unfiltered library is still a target, so a model can always be filed back out of
  // a category.
  expect(screen.getByRole('button', { name: strings.folders.root })).toBeDefined()
})

/** The body of the first request made with `method`, parsed. */
function bodyOf(fetchMock: ReturnType<typeof stubFetch>, method: string): unknown {
  const call = fetchMock.mock.calls.find(
    ([, init]) => (init as { method?: string } | undefined)?.method === method,
  )
  return JSON.parse(((call?.[1] as { body?: string } | undefined)?.body ?? 'null') as string)
}

/** The dialog's one text field, and the value a user would have typed into it. */
const nameField = () => screen.getByRole('textbox', { name: strings.folders.createLabel })
const typeName = (value: string) => fireEvent.change(nameField(), { target: { value } })

test('creates a category under the one that is selected', async () => {
  const fetchMock = stubFetch({
    folders: ok([TERRAIN, ROCKS]),
    folderCreate: ok({ ...ROCKS, id: '01a06b30-4c11-7a92-8f03-6d1e5c9a0009', name: 'Scree' }),
  })
  renderTree(TERRAIN.id)

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.newCategory }))
  // The title is where the destination is stated. One control whose target moves with the
  // sidebar selection owes the user that answer before they type into it.
  expect(
    screen.getByRole('heading', { name: strings.folders.createTitle('Terrain') }),
  ).toBeTruthy()

  typeName('Scree')
  fireEvent.click(screen.getByRole('button', { name: strings.folders.createConfirm }))

  await waitFor(() =>
    expect(bodyOf(fetchMock, 'POST')).toEqual({ parentId: TERRAIN.id, name: 'Scree' }),
  )
})

test('creates at the library root when no category is selected', async () => {
  const fetchMock = stubFetch({
    folders: ok([TERRAIN]),
    folderCreate: ok({ ...TERRAIN, id: '01a06b30-4c11-7a92-8f03-6d1e5c9a000a', name: 'Fasteners' }),
  })
  renderTree(null)

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.newCategory }))
  expect(screen.getByRole('heading', { name: strings.folders.createTitle(null) })).toBeTruthy()
  typeName('Fasteners')
  fireEvent.click(screen.getByRole('button', { name: strings.folders.createConfirm }))

  await waitFor(() =>
    expect(bodyOf(fetchMock, 'POST')).toEqual({ parentId: null, name: 'Fasteners' }),
  )
})

/**
 * **The trap this test exists for.** `FolderPatch.parentId` is optional *and* nullable, and
 * the two are different requests: omitted means "leave it where it is", `null` means "move
 * it to the library root". A rename body built by spreading an object that carries a
 * `parentId` key at all — or written from a mental model where absent and null are the same
 * thing — moves every renamed category to the root, silently, on every rename.
 *
 * So the body is asserted by deep equality and not by looking for `name` in it: this has to
 * fail on an *extra* key, which `toMatchObject` would let through.
 */
test('a rename sends the name alone and never a parent', async () => {
  const fetchMock = stubFetch({ folders: ok([TERRAIN, ROCKS]), folderPatch: ok({}) })
  renderTree()

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.renameFor('Rocks') }))
  typeName('Boulders')
  fireEvent.click(screen.getByRole('button', { name: strings.folders.renameConfirm }))

  await waitFor(() => expect(bodyOf(fetchMock, 'PATCH')).toEqual({ name: 'Boulders' }))
})

/**
 * The rename dialog names the folder on disk, and it has to read that off `slug` rather than
 * off the name it is about to replace. `TERRAIN`'s slug is `Terain` — created with the typo,
 * renamed to fix it, folder unchanged — which is exactly the state `DATA.md` §1.1 makes
 * ordinary, and the one a dialog printing `folder.name` would get wrong while looking
 * correct against every other fixture here.
 */
test('the rename dialog names the folder on disk, not the category', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS]) })
  renderTree()

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.renameFor('Terrain') }))
  expect(screen.getByText(strings.folders.renameKeepsDirectory('Terain'))).toBeTruthy()
})

/**
 * A refusal keeps the dialog open with what the user typed still in it: editing that name is
 * the one thing they can do about this, and closing the dialog would throw it away.
 *
 * `slugTaken` specifically, because the rename decision is what made it reachable this way —
 * a sibling renamed away from `Rocks` still occupies that folder, so the *name* is free and
 * the *directory* is not.
 */
test('a refused rename keeps the dialog open and says which refusal it was', async () => {
  stubFetch({ folders: ok([TERRAIN, ROCKS]), folderPatch: conflict('slugTaken') })
  renderTree()

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.renameFor('Rocks') }))
  typeName('Scree')
  fireEvent.click(screen.getByRole('button', { name: strings.folders.renameConfirm }))

  const note = await screen.findByRole('alert')
  expect(note.textContent).toBe(strings.folders.slugTaken)
  expect((nameField() as HTMLInputElement).value).toBe('Scree')
})

/** A name of nothing but spaces is not a name, and the confirm cannot be pressed on one. */
test('the confirm stays disabled until the name has something in it', async () => {
  stubFetch({ folders: ok([TERRAIN]) })
  renderTree()

  fireEvent.click(await screen.findByRole('button', { name: strings.folders.newCategory }))
  const create = screen.getByRole('button', { name: strings.folders.createConfirm })
  expect((create as HTMLButtonElement).disabled).toBe(true)

  typeName('   ')
  expect((create as HTMLButtonElement).disabled).toBe(true)
})
