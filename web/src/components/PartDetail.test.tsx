import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import { expect, test, vi } from 'vitest'
import { Detail } from './PartDetail'
import { strings } from '../lib/strings'
import type { PartDetail, PartRevision } from '../lib/types'

const { mounts } = vi.hoisted(() => ({ mounts: [] as string[] }))

// jsdom draws no WebGL, so the view is a stand-in that records each part it is mounted for. Since
// R1 that list is also the assertion that matters most: the view is mounted when somebody presses
// **view** and at no other time, so an empty `mounts` is a page that loaded no mesh.
vi.mock('../lib/viewer-math', async (original) => ({
  ...(await original<typeof import('../lib/viewer-math')>()),
  hasWebGL: () => true,
}))
vi.mock('./Viewer', async () => {
  const { useEffect } = await import('react')
  return {
    default: function Viewer({ part }: { part: PartDetail }) {
      useEffect(() => {
        mounts.push(part.id)
      }, [part.id])
      return null
    },
  }
})

const BRACKET: PartDetail = {
  yours: true,
  id: '01931b6e-0000-7000-8000-00000000aaaa',
  library: '01931b6e-0000-7000-8000-000000000001',
  revision: '01931b6e-0000-7000-8000-00000000bbbb',
  revLabel: '1',
  name: 'angle-bracket-60x60x40-lp-9004-00',
  partNumber: null,
  tags: [],
  materials: [],
  materialsTyped: false,
  custom: {},
  sourcePath: 'cad/angle-bracket-60x60x40-lp-9004-00.igs',
  triangleCount: 44,
  isWatertight: true,
  bboxMm: { value: [60, 40, 60], approximate: false },
  volumeMm3: { value: 35840, approximate: false },
  surfaceAreaMm2: { value: 11392, approximate: false },
  massG: null,
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceFormat: 'iges',
  sourceBytes: 18204,
  storedBytes: 6120,
  compressed: true,
  directory: 'libraries/default/cad',
  storagePath: 'libraries/default/cad/angle-bracket-60x60x40-lp-9004-00.igs',
  fileRoot: 'store',
  createdAt: '2026-09-13T10:00:00Z',
  updatedAt: '2026-09-13T10:00:00Z',
}
const PIN: PartDetail = {
  ...BRACKET,
  id: '01931b6e-0000-7000-8000-00000000cccc',
  name: 'stop-pin-d10x20-lp-9007-00',
}

/**
 * **The 3D view is asked for, and a part that nothing can draw does not offer it** (R1).
 *
 * Opening a part reads metadata and the gallery; the model is fetched when somebody presses
 * **view**, and `mounts` staying empty until then is the assertion. The part the page shows
 * changing — which is what the quick look does, in place, without unmounting — closes the view
 * rather than carrying one part's model to the next.
 *
 * `BRACKET` is an IGES part, so it is also the other half: `mesh.ts` has no loader for B-rep and
 * the button is not offered at all. Its download and its figures are untouched, which is what it
 * had before.
 */
test('the history appears once a part has a second revision, and says where each came from', async () => {
  const second = {
    id: '01931b6e-0000-7000-8000-00000000dddd',
    parent: BRACKET.revision,
    revLabel: '2',
    origin: 'upload',
    createdAt: '2026-09-14T09:30:00Z',
    triangleCount: 44,
    faceCount: null,
    edgeCount: null,
    bboxMm: { value: [66, 40, 60], approximate: true },
    volumeMm3: { value: 39424, approximate: true },
    surfaceAreaMm2: null,
    massG: { value: 309.478, approximate: true },
    centreMm: null,
    sourceHash: '6666666666666666666666666666666666666666666666666666666666666666',
    sourceFormat: 'stl',
    sourceBytes: 20124,
    onDisk: true,
  } satisfies PartRevision
  const first = {
    ...second,
    id: BRACKET.revision,
    parent: null,
    revLabel: '1',
    origin: 'ingest',
    createdAt: '2026-09-13T10:00:00Z',
    volumeMm3: { value: 35840, approximate: true },
    massG: { value: 281.344, approximate: true },
    // The owner saved revision 2 over it in their folder (M4), so its bytes are nowhere.
    onDisk: false,
  } satisfies PartRevision
  let history: PartRevision[] = [first]
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () => (url.endsWith('/revisions') ? history : []),
    })),
  )
  const settledPage = async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const view = render(
      <QueryClientProvider client={client}>
        <Detail part={BRACKET} />
      </QueryClientProvider>,
    )
    // Settled, not merely asked: an absent section before the answer arrives proves nothing.
    await waitFor(() =>
      expect(client.getQueryState(['revisions', BRACKET.id])?.status).toBe('success'),
    )
    return view
  }

  const one = await settledPage()
  expect(screen.queryByText(strings.detail.history)).toBeNull()
  one.unmount()

  history = [second, first]
  await settledPage()
  expect(await screen.findByText(strings.detail.history)).toBeTruthy()
  const section = screen.getByText(strings.detail.history).closest('section') as HTMLElement
  const item = (label: string) =>
    [...section.querySelectorAll('li')].find((li) =>
      li.textContent?.startsWith(strings.detail.historyRevision(label)),
    )
  expect(item('2')?.textContent).toContain(strings.detail.origin.upload)
  expect(item('2')?.textContent).toContain(strings.detail.approximate)
  expect(item('1')?.textContent).toContain(strings.detail.origin.ingest)
  // The revision on disk downloads; the one it replaced keeps its figures and says why it has no
  // link, rather than offering one that answers 410.
  expect(item('2')?.querySelector('a')?.getAttribute('href')).toBe(
    `/api/revisions/${second.id}/download?variant=original`,
  )
  expect(item('1')?.querySelector('a')).toBeNull()
  expect(item('1')?.textContent).toContain(strings.detail.historyNotOnDisk)
  // No comparison and no diff request: those went with governance (S4).
  expect(section.querySelector('table')).toBeNull()
  expect(fetch).not.toHaveBeenCalledWith(expect.stringContaining('/diff?'))
  vi.unstubAllGlobals()
})

test('the view is offered for a mesh, mounted only when pressed, and closed when the part changes', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200, json: async () => [] })))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const page = (part: PartDetail) => (
    <QueryClientProvider client={client}>
      <Detail part={part} />
    </QueryClientProvider>
  )
  const mesh = { ...BRACKET, sourceFormat: 'stl' }
  const otherMesh = { ...PIN, sourceFormat: '3mf' }

  const { rerender } = render(page(mesh))
  const view = await screen.findByRole('button', { name: strings.viewer.view })
  expect(mounts).toEqual([])

  fireEvent.click(view)
  await waitFor(() => expect(mounts).toEqual([mesh.id]))

  // Another part in the same Preview: the view closes rather than drawing the last part's model.
  rerender(page(otherMesh))
  expect(await screen.findByRole('button', { name: strings.viewer.view })).toBeTruthy()
  expect(mounts).toEqual([mesh.id])
  fireEvent.click(screen.getByRole('button', { name: strings.viewer.view }))
  await waitFor(() => expect(mounts).toEqual([mesh.id, otherMesh.id]))

  // And a B-rep part is never offered one, because nothing in the browser reads it.
  cleanup()
  render(page(BRACKET))
  await waitFor(() => expect(screen.getByText(BRACKET.name)).toBeTruthy())
  expect(screen.queryByRole('button', { name: strings.viewer.view })).toBeNull()
  vi.unstubAllGlobals()
})

/** On the part page tags are added and removed as the whole list; anywhere else they are only listed. */
test('tags are added and removed where the part is recordable, and only listed elsewhere', async () => {
  const puts: unknown[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (_url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'PUT') {
        puts.push(JSON.parse(init.body ?? 'null'))
        return { ok: true, status: 204, json: async () => null }
      }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const tagged = { ...BRACKET, tags: ['welding jig'] }
  // A router, because a tag chip is a `<Link>` to the tag's own page (P3) and `<Link>` reads router
  // context — without a provider the whole panel renders as nothing. The tree is synthetic, as
  // `index.test.tsx`'s is: what this test is about is the editing, and the link wants a real target
  // to resolve against rather than a real route tree to live in.
  const pageFor = (part: typeof tagged, recordable: boolean) => {
    const root = createRootRoute({
      component: () => <Detail part={part} recordable={recordable} />,
    })
    const tags = createRoute({ getParentRoute: () => root, path: '/tags/$tag', component: () => null })
    const router = createRouter({
      routeTree: root.addChildren([tags]),
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })
    return (
      <QueryClientProvider client={client}>
        {/* The synthetic tree is not the registered one, so the cast is confined to this one line. */}
        <RouterProvider router={router as never} />
      </QueryClientProvider>
    )
  }
  const page = (recordable: boolean) => pageFor(tagged, recordable)
  render(page(true))
  // `RouterProvider` has nothing mounted on its first render, so the panel arrives a tick later.
  await screen.findByLabelText(strings.tags.field)

  fireEvent.change(screen.getByLabelText(strings.tags.field), { target: { value: 'spare' } })
  fireEvent.click(screen.getByRole('button', { name: strings.tags.add }))
  await waitFor(() => expect(puts).toEqual([{ tags: ['welding jig', 'spare'] }]))
  await waitFor(() => expect(screen.getByRole('button', { name: strings.tags.add })).toBeTruthy())

  fireEvent.click(screen.getByRole('button', { name: strings.tags.remove('welding jig') }))
  await waitFor(() => expect(puts).toEqual([{ tags: ['welding jig', 'spare'] }, { tags: [] }]))

  // A second render rather than a rerender: each `page()` builds its own router, and swapping one
  // `RouterProvider` for another inside the same tree leaves the new router unmounted.
  cleanup()
  render(page(false))
  await screen.findByText('welding jig')
  // The chip is a link to the tag's page, in the quick look as on the part page: it is the one
  // `about` both layouts draw. Materials get no link — there is no page of a library's materials.
  expect(screen.getByRole('link', { name: 'welding jig' }).getAttribute('href')).toBe(
    '/tags/welding%20jig',
  )

  // A model in a second library: the chip carries the library, or the tag opens the first library's
  // page — the same word about a different set of models.
  cleanup()
  const elsewhere = '01952c40-0000-7000-8000-0000000000f2'
  render(pageFor({ ...tagged, library: elsewhere }, false))
  await screen.findByText('welding jig')
  expect(screen.getByRole('link', { name: 'welding jig' }).getAttribute('href')).toBe(
    `/tags/welding%20jig?library=${elsewhere}`,
  )
  expect(screen.queryByRole('button', { name: strings.tags.remove('welding jig') })).toBeNull()
  expect(screen.queryByLabelText(strings.tags.field)).toBeNull()
  vi.unstubAllGlobals()
})

/** Materials are edited as tags are, and a list nobody typed says it is what the file states. */
test('materials are edited like tags, and a list nobody typed says it is the file’s', async () => {
  const puts: { url: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'PUT') {
        puts.push({ url, body: JSON.parse(init.body ?? 'null') })
        return { ok: true, status: 204, json: async () => null }
      }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const invalidate = vi.spyOn(client, 'invalidateQueries')
  render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, materials: ['AISI 1045 steel'], materialsTyped: false }} recordable />
    </QueryClientProvider>,
  )

  expect(screen.getByText(strings.materials.fromFile)).toBeTruthy()
  fireEvent.change(screen.getByLabelText(strings.materials.field), { target: { value: 'EN AW-6082 T6' } })
  fireEvent.click(screen.getByRole('button', { name: strings.materials.add }))
  await waitFor(() =>
    expect(puts).toEqual([
      { url: `/api/parts/${BRACKET.id}/materials`, body: { materials: ['AISI 1045 steel', 'EN AW-6082 T6'] } },
    ]),
  )
  // Its mass is worked out from its materials, so its history and comparison read again.
  await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: ['revisions', BRACKET.id] }))
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['diff', BRACKET.id] })
  vi.unstubAllGlobals()
})

test('removing a part\u2019s only material says it holds none, and the way back is its own button', async () => {
  // L3, item 4. The × on the only material used to hand the part straight back to its file, which
  // named the material again and said nothing about it. Now the empty list is a decision the page
  // states, and `DELETE` is the undo.
  const sent: { url: string; method: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'PUT' || init?.method === 'DELETE') {
        sent.push({ url, method: init.method, body: JSON.parse(init.body ?? 'null') })
        return { ok: true, status: 204, json: async () => null }
      }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, materials: ['AISI 1045 steel'], materialsTyped: false }} recordable />
    </QueryClientProvider>,
  )
  // Nobody has typed a list yet, so there is nothing to hand back.
  expect(screen.queryByRole('button', { name: strings.materials.reset })).toBeNull()
  fireEvent.click(
    screen.getByRole('button', { name: strings.materials.remove('AISI 1045 steel') }),
  )
  await waitFor(() =>
    expect(sent).toEqual([
      { url: `/api/parts/${BRACKET.id}/materials`, method: 'PUT', body: { materials: [] } },
    ]),
  )
  view.unmount()

  // The part as the server hands it back afterwards: typed, and empty.
  render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, materials: [], materialsTyped: true }} recordable />
    </QueryClientProvider>,
  )
  expect(screen.getByText(strings.materials.none)).toBeTruthy()
  expect(screen.queryByText(strings.materials.fromFile)).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: strings.materials.reset }))
  await waitFor(() =>
    expect(sent.at(-1)).toEqual({
      url: `/api/parts/${BRACKET.id}/materials`,
      method: 'DELETE',
      body: null,
    }),
  )
  vi.unstubAllGlobals()
})

test('a part that holds no material says so where the list cannot be edited', async () => {
  // The quick look does not offer the form, and `WordList` used to render nothing at all for an
  // empty list there. "Holds no material" is exactly the state a reader needs told, because the
  // blank was somebody's decision rather than a gap.
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={{ ...BRACKET, materials: [], materialsTyped: true }} />
    </QueryClientProvider>,
  )
  expect(screen.getByText(strings.materials.none)).toBeTruthy()
  expect(screen.queryByRole('button', { name: strings.materials.reset })).toBeNull()
})

/** One revision is the Identity row, said once; two are a history, each with its origin, its ≈ and its original. */

/** Who holds a check-out shows everywhere; releasing it is the part page's, behind a dialog. */
/** A STEP part's revisions compare its B-rep faces and edges exactly, beside figures a mesh gave. */


/**
 * **The comparison compares figures, and draws no ghost** (R1).
 *
 * The ghost was an earlier revision's rung drawn over the current part in grey, so a shape change
 * was visible rather than only tabulated. It needed a rung per revision; nothing produces rungs,
 * and a revision's *original* file is not a substitute — two full meshes in one scene for a
 * comparison somebody reached through a disclosure is not a trade worth making, and R3 takes the
 * hashes the checkbox read away regardless.
 *
 * Asserted as an absence beside the figures that stayed, because the checkbox sat in the same row
 * as the two revision pickers and would otherwise be a control that quietly stopped doing anything.
 */

/** The link that opens a part in a desktop app, offered only where a save can come back as a revision. */

/** A slicer reads an STL as it is, so only a part it does not read is offered a 3MF, written when asked. */
test('a STEP part offers a 3MF for a slicer, watches it being written, then links to it', async () => {
  const batchId = '01931b6e-0000-7000-8000-0000000000b1'
  let finishedAt: string | null = null
  const fetch = vi.fn(async (url: string, init?: RequestInit) => ({
    ok: true,
    status: init?.method === 'POST' ? 202 : 200,
    json: async () =>
      init?.method === 'POST'
        ? { batchId, queued: 1 }
        : url.endsWith(`/jobs/${batchId}`)
          ? { batchId, failed: [], finishedAt }
          : [],
  }))
  vi.stubGlobal('fetch', fetch)
  const page = (sourceFormat: string) => (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={{ ...BRACKET, sourceHash: '7'.repeat(64), sourceFormat }} />
    </QueryClientProvider>
  )

  const view = render(page('stl'))
  await screen.findByRole('link', { name: strings.download.original })
  expect(screen.queryByRole('button', { name: strings.download.forSlicer })).toBeNull()
  view.unmount()

  render(page('step'))
  fireEvent.click(await screen.findByRole('button', { name: strings.download.forSlicer }))
  expect(await screen.findByRole('button', { name: strings.download.forSlicerBuilding })).toBeTruthy()
  expect(fetch).toHaveBeenCalledWith(`/api/parts/${BRACKET.id}/exports/3mf`, { method: 'POST' })
  finishedAt = '2026-09-15T10:00:00Z'
  const link = await screen.findByRole('link', { name: strings.download.forSlicerReady }, { timeout: 3000 })
  expect(link.getAttribute('href')).toBe(`/api/revisions/${BRACKET.revision}/download?variant=3mf`)
  vi.unstubAllGlobals()
})


test('Copy link copies the part’s current-file link, and shows it to copy where the clipboard is kept from pages', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200, json: async () => [] })))
  const link = `${window.location.origin}/api/parts/${BRACKET.id}/file`
  const page = () =>
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Detail part={BRACKET} />
      </QueryClientProvider>,
    )
  const button = () => screen.getByRole('button', { name: strings.download.copyLinkFor(BRACKET.name) })

  const writeText = vi.fn(async () => undefined)
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
  const one = page()
  fireEvent.click(button())
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(link))
  expect(await screen.findAllByText(strings.download.linkCopied)).not.toHaveLength(0)
  expect(screen.queryByLabelText(strings.download.linkToCopy)).toBeNull()
  one.unmount()

  // Plain http on a LAN: no clipboard at all. The link is shown, selected, rather than nothing happening.
  Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true })
  page()
  fireEvent.click(button())
  const shown = (await screen.findByLabelText(strings.download.linkToCopy)) as HTMLInputElement
  expect(shown.value).toBe(link)
  vi.unstubAllGlobals()
})

/**
 * M1: a picture added on the part's page reached its gallery and not its grid card, which kept the
 * old answer until a reload. Adding one now refreshes every grid page too.
 */
test('adding a picture refreshes the grid as well as the gallery', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (_url: string, init?: { method?: string }) =>
      init?.method === 'POST'
        ? { ok: true, status: 201, json: async () => ({ id: '01931b6e-0000-7000-8000-00000000f00d', width: 128, height: 128 }) }
        : { ok: true, status: 200, json: async () => [] },
    ),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const invalidated = vi.spyOn(client, 'invalidateQueries')
  const view = render(
    <QueryClientProvider client={client}>
      <Detail part={BRACKET} recordable />
    </QueryClientProvider>,
  )
  // Hidden behind its button, which is the control a person uses; the change is what matters here.
  const input = await waitFor(() => {
    const found = view.container.querySelector('input[type="file"][accept^="image/"]')
    if (found === null) throw new Error('no picture input yet')
    return found as HTMLInputElement
  })
  fireEvent.change(input, { target: { files: [new File(['png'], 'bracket.png', { type: 'image/png' })] } })
  await waitFor(() =>
    expect(invalidated.mock.calls.map(([filters]) => filters?.queryKey)).toEqual(
      expect.arrayContaining([['part-images', BRACKET.id], ['parts']]),
    ),
  )
  vi.unstubAllGlobals()
})
