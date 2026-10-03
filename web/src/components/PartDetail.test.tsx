import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import { expect, test, vi } from 'vitest'
import { Detail } from './PartDetail'
import { strings } from '../lib/strings'
import type { AssemblyNode, AssemblyTree, PartDetail, PartRevision } from '../lib/types'

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
  pmi: null,
  sourcePath: 'cad/angle-bracket-60x60x40-lp-9004-00.igs',
  thumbnail: null,
  triangleCount: 44,
  isWatertight: true,
  bboxMm: { value: [60, 40, 60], approximate: false },
  volumeMm3: { value: 35840, approximate: false },
  surfaceAreaMm2: { value: 11392, approximate: false },
  massG: null,
  kernelVersion: 'occt occt-8.0.1-bridge-4+deflection-0.1+glb-1+cpu-1',
  lock: null,
  sharedBy: null,
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceFormat: 'iges',
  sourceBytes: 18204,
  storedBytes: 6120,
  compressed: true,
  tessellationL0: '3333333333333333333333333333333333333333333333333333333333333333',
  tessellationL0Bytes: 1400,
  structure: null,
  tessellationL1: null,
  tessellationL2: null,
  entities: null,
  directory: 'libraries/default/cad',
  storagePath: 'libraries/default/cad/angle-bracket-60x60x40-lp-9004-00.igs',
  createdAt: '2026-09-13T10:00:00Z',
  updatedAt: '2026-09-13T10:00:00Z',
}
const PIN: PartDetail = {
  ...BRACKET,
  id: '01931b6e-0000-7000-8000-00000000cccc',
  name: 'stop-pin-d10x20-lp-9007-00',
  tessellationL0: '5555555555555555555555555555555555555555555555555555555555555555',
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

const IDENTITY: AssemblyNode['transform'] = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
const leaf = (name: string): AssemblyNode => ({ name, prototype: '0:1:1:2', transform: IDENTITY, children: [] })
/** A plate beside a station of two parts: three placed parts, the station's the second and third. */
const FIXTURE: AssemblyTree = {
  roots: [
    {
      name: 'fixture-plate-assembly-lp-9000-00',
      prototype: '0:1:1:1',
      transform: IDENTITY,
      children: [
        leaf('base-plate-lp-9001-00'),
        {
          name: 'bracket-station-lp-9002-00',
          prototype: '0:1:1:3',
          transform: IDENTITY,
          children: [leaf('angle-bracket-lp-9004-00'), leaf('stop-pin-d10x20-lp-9007-00')],
        },
      ],
    },
  ],
  parts: 3,
  prototypes: 3,
}

function renderAssembly() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () => (url.startsWith('/api/blob/') ? FIXTURE : []),
    })),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, structure: '6666666666666666666666666666666666666666666666666666666666666666' }} />
    </QueryClientProvider>,
  )
}

/**
 * **The tree reads, and no longer hides** (R1).
 *
 * Hiding and isolating a part needed `extras.parts` — the per-part triangle runs `glb.rs` wrote
 * into a rung — so the viewer could draw the mesh with one part left out. It draws the original
 * file now, which carries no such thing, so there is nothing to hide a part *in*. Two tests stood
 * here, one driving the hide buttons and one asserting they were absent for a rung that counted no
 * parts; the second would now pass against anything at all.
 *
 * What the tree says about the assembly is unchanged, and that is what this asserts: the counts,
 * the nesting, and a branch whose rows are drawn only once it is opened.
 */
test('an assembly lists its tree, each branch a disclosure, and offers nothing to hide', async () => {
  renderAssembly()
  const section = (await screen.findByRole('heading', { name: strings.detail.assembly })).closest('section')
  expect(section).not.toBeNull()
  expect(section!.textContent).toContain(strings.detail.assemblyCounts(3, 3))

  const station = 'bracket-station-lp-9002-00'
  const branch = screen.getByText(station).closest('details') as HTMLDetailsElement
  // Closed, so the station's own two parts are not rows yet.
  expect(screen.queryByText('stop-pin-d10x20-lp-9007-00')).toBeNull()
  branch.open = true
  fireEvent(branch, new Event('toggle'))
  expect(await screen.findByText('stop-pin-d10x20-lp-9007-00')).toBeTruthy()

  // No control in the section: the words below are what the hide, isolate and show-all buttons said.
  expect(section!.querySelectorAll('button')).toHaveLength(0)
  vi.unstubAllGlobals()
})

/** What the file specifies is listed as specified, each annotation on the face it names. */
test('a part lists the dimensions and tolerances its file specifies, labelled as specified', async () => {
  const pmiHash = '7777777777777777777777777777777777777777777777777777777777777777'
  const entitiesHash = '8888888888888888888888888888888888888888888888888888888888888888'
  const cylinder = { prototype: '0:1:1:1', face: 1 }
  const base = { prototype: '0:1:1:1', face: 2 }
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith(pmiHash)
          ? {
              dimensions: [{ type: 'diameter', value: 22, upper: 0.05, lower: 0, faces: [cylinder] }],
              tolerances: [{ type: 'perpendicularity', value: 0.05, datums: ['A'], faces: [cylinder] }],
              datums: [{ name: 'A', faces: [base] }],
            }
          : url.endsWith(entitiesHash)
            ? [
                { type: 'cylinder', prototype: '0:1:1:1', face: 1, radius: 11, origin: [0, 0, 0], axis: [0, 0, 1] },
                { type: 'plane', prototype: '0:1:1:1', face: 2, origin: [0, 0, 0], normal: [0, 0, -1] },
              ]
            : [],
    })),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, pmi: pmiHash, entities: entitiesHash }} />
    </QueryClientProvider>,
  )

  expect(await screen.findByText(strings.pmi.note)).toBeTruthy()
  const item = (text: string) => screen.getByText(text).closest('li')?.textContent
  await waitFor(() =>
    expect(item(strings.pmi.dimension('diameter', 22, 0.05, 0))).toContain(strings.pmi.face('cylinder')),
  )
  expect(strings.pmi.dimension('diameter', 22, 0.05, 0)).toBe('⌀22 mm +0.05 / 0')
  expect(item(strings.pmi.tolerance('perpendicularity', 0.05, ['A']))).toContain(strings.pmi.face('cylinder'))
  expect(item(strings.pmi.datum('A'))).toContain(strings.pmi.face('plane'))
  expect(screen.queryByText(/≈/)).toBeNull()
  vi.unstubAllGlobals()
})

/**
 * **PMI is a list and not an overlay** (R1).
 *
 * Two tests stood here: one toggling the annotations into the 3D view, one about faces that could
 * not be read. Both needed the analytic entities placed by the assembly tree — geometry we
 * produced — to know where on the part a face is. The viewer draws the original file now and has
 * no entities, so there is no "show in view" and nothing to say about which annotations it could
 * not place. The list above says what the file specifies, which was always the honest surface.
 *
 * What is asserted here is the absence, because the list and the overlay shared a section and a
 * leftover control would be a press that does nothing.
 */
test('the specified list offers nothing to draw in the view', async () => {
  const pmiHash = '7777777777777777777777777777777777777777777777777777777777777777'
  const entitiesHash = '8888888888888888888888888888888888888888888888888888888888888888'
  const cylinder = { prototype: '0:1:1:1', face: 1 }
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith(pmiHash)
          ? {
              dimensions: [{ type: 'diameter', value: 22, upper: 0.05, lower: 0, faces: [cylinder] }],
              tolerances: [],
              datums: [],
            }
          : url.endsWith(entitiesHash)
            ? [{ type: 'cylinder', prototype: '0:1:1:1', face: 1, radius: 11, origin: [0, 0, 0], axis: [0, 0, 1] }]
            : [],
    })),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={{ ...BRACKET, pmi: pmiHash, entities: entitiesHash, sourceFormat: 'stl' }} />
    </QueryClientProvider>,
  )

  const row = await screen.findByText(strings.pmi.dimension('diameter', 22, 0.05, 0))
  expect(row.closest('li')?.textContent).toContain(strings.pmi.face('cylinder'))
  const section = row.closest('section')
  expect(section?.querySelectorAll('button')).toHaveLength(0)
  vi.unstubAllGlobals()
})

/** One revision is the Identity row, said once; two are a history, each with its origin, its ≈ and its original. */
test('the history appears once a part has a second revision, and says where each came from', async () => {
  const second = {
    id: '01931b6e-0000-7000-8000-00000000dddd',
    parent: BRACKET.revision,
    revLabel: '2',
    origin: 'agent',
    createdAt: '2026-09-14T09:30:00Z',
    thumbnail: null,
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
    tessellationL0: null,
    tessellationL1: null,
    deltaFromParent: {
      volumeMm3: { from: 35840, to: 39424, change: 3584, percent: 10, approximate: true },
      surfaceAreaMm2: null,
      bboxMm: null,
      triangleCount: null,
      faceCount: null,
      edgeCount: null,
      massG: { from: 281.344, to: 309.478, change: 28.134, percent: 10, approximate: true },
      centreMm: null,
    },
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
    deltaFromParent: null,
  } satisfies PartRevision
  let history: PartRevision[] = [first]
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith('/revisions')
          ? history
          : url.includes('/diff?')
            ? second.deltaFromParent
            : [],
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
  // Within the history's own list: the compare control names every revision too, as options.
  const section = screen.getByText(strings.detail.history).closest('section') as HTMLElement
  const item = (label: string) =>
    [...section.querySelectorAll('li')].find((li) =>
      li.textContent?.startsWith(strings.detail.historyRevision(label)),
    )
  expect(item('2')?.textContent).toContain(strings.detail.origin.agent)
  expect(item('2')?.textContent).toContain(strings.detail.approximate)
  expect(item('1')?.textContent).toContain(strings.detail.origin.ingest)
  expect(item('2')?.querySelector('a')?.getAttribute('href')).toBe(
    `/api/revisions/${second.id}/download?variant=original`,
  )
  // Revision 2 says what changed from its parent, marked as the mesh figure it is.
  expect(item('2')?.textContent).toContain(strings.detail.volumeChange(3584, 10))
  expect(strings.detail.volumeChange(3584, 10)).toBe('+3.58 cm³ (+10%)')
  // A B-rep centre on its axis moves by float noise between revisions: too small for two places, not "−0 mm".
  expect(strings.detail.lengthChange(-2.5e-13, null)).toBe('under 0.01 mm')

  // The comparison opens on the newest against the one before it, and a figure one of them
  // did not record says so rather than showing a zero.
  const table = await screen.findByRole('table')
  expect(fetch).toHaveBeenCalledWith(
    `/api/parts/${BRACKET.id}/diff?from=${first.id}&to=${second.id}`,
  )
  expect(table.textContent).toContain(strings.detail.volumeChange(3584, 10))
  expect(table.textContent).toContain(strings.detail.approximate)
  expect(table.textContent).toContain(strings.detail.notInBoth)
  // Mass beside volume, in the history and the comparison, with the note saying whose density it uses.
  expect(item('2')?.textContent).toContain(strings.detail.massChange(28.134, 10))
  expect(table.textContent).toContain(strings.detail.mass)
  expect(table.textContent).toContain(strings.detail.massChange(28.134, 10))
  expect(screen.getByText(strings.detail.massNote)).toBeTruthy()
  // A centre of mass neither revision recorded is said so per axis, not shown as no movement — and
  // because both revisions have a volume, what it says is that a re-read is queued for it (L3, item
  // 1), not "not measured in both", which would be a claim about a measurement nobody attempted.
  const centre = (axis: 0 | 1 | 2) =>
    [...table.querySelectorAll('tr')].find(
      (tr) => tr.querySelector('th')?.textContent === strings.detail.centreAxis(axis),
    )
  expect(table.textContent).toContain(strings.detail.centreAxis(2))
  for (const axis of [0, 1, 2] as const) {
    expect(centre(axis)?.querySelector('td')?.textContent).toBe(strings.detail.centreQueued)
  }
  vi.unstubAllGlobals()
})

/** Who holds a check-out shows everywhere; releasing it is the part page's, behind a dialog. */
/** A STEP part's revisions compare its B-rep faces and edges exactly, beside figures a mesh gave. */
test('the comparison shows a CAD revision’s faces and edges exactly', async () => {
  const revision = (id: string, revLabel: string, parent: string | null, faces: number, edges: number): PartRevision => ({
    id,
    parent,
    revLabel,
    origin: parent === null ? 'ingest' : 'agent',
    createdAt: '2026-09-15T13:00:00Z',
    thumbnail: null,
    triangleCount: 28576,
    faceCount: faces,
    edgeCount: edges,
    bboxMm: null,
    volumeMm3: null,
    surfaceAreaMm2: null,
    massG: null,
    centreMm: null,
    sourceHash: null,
    sourceFormat: 'step',
    sourceBytes: 190356,
    deltaFromParent: null,
    tessellationL0: null,
    tessellationL1: null,
  })
  const [first, second] = ['01931b6e-0000-7000-8000-0000000000e1', '01931b6e-0000-7000-8000-0000000000e2']
  const exact = (from: number, to: number) => ({ from, to, change: to - from, percent: ((to - from) / from) * 100, approximate: false })
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith('/revisions')
          ? [revision(second, '2', first, 42, 108), revision(first, '1', null, 38, 96)]
          : url.includes('/diff?')
            ? { volumeMm3: null, surfaceAreaMm2: null, bboxMm: null, triangleCount: null, faceCount: exact(38, 42), edgeCount: exact(96, 108) }
            : [],
    })),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={BRACKET} />
    </QueryClientProvider>,
  )

  const table = await screen.findByRole('table')
  const row = (label: string) =>
    [...table.querySelectorAll('tr')].find((tr) => tr.querySelector('th')?.textContent === label)
  await waitFor(() => expect(row(strings.detail.faces)?.textContent).toContain(strings.detail.countChange(4, (4 / 38) * 100)))
  expect(row(strings.detail.faces)?.textContent).not.toContain(strings.detail.approximate)
  expect(row(strings.detail.edges)?.textContent).toContain(strings.detail.countChange(12, (12 / 96) * 100))
  // Neither of these revisions recorded a volume, so there is no centre to queue a re-read for and
  // the row falls back to the ordinary words. Without this the "queued" wording would be untested
  // against the case it must not claim.
  expect(row(strings.detail.centreAxis(0))?.querySelector('td')?.textContent).toBe(
    strings.detail.notInBoth,
  )
})

test('a centre nothing will re-read says so, rather than promising a job that is not queued', async () => {
  // L3, item 1. The page may only say "a re-read is queued" where the worker sweep really queues one,
  // and that sweep takes mesh formats with a volume above zero. A STEP revision is the
  // stale-derivative sweep's and waits on a worker with a CAD kernel; an open mesh has no volume and
  // never gets a centre at all. Both get the ordinary words.
  const revision = (
    id: string,
    revLabel: string,
    parent: string | null,
    sourceFormat: string,
    volumeMm3: number | null,
  ): PartRevision => ({
    id,
    parent,
    revLabel,
    origin: parent === null ? 'ingest' : 'agent',
    createdAt: '2026-09-15T13:00:00Z',
    thumbnail: null,
    triangleCount: 28576,
    faceCount: null,
    edgeCount: null,
    bboxMm: null,
    volumeMm3: volumeMm3 === null ? null : { value: volumeMm3, approximate: true },
    surfaceAreaMm2: null,
    massG: null,
    centreMm: null,
    sourceHash: null,
    sourceFormat,
    sourceBytes: 190356,
    deltaFromParent: null,
    tessellationL0: null,
    tessellationL1: null,
  })
  const [first, second] = ['01931b6e-0000-7000-8000-0000000000f1', '01931b6e-0000-7000-8000-0000000000f2']
  const pair = async (history: PartRevision[]) => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => ({
        ok: true,
        status: 200,
        json: async () =>
          url.endsWith('/revisions')
            ? history
            : url.includes('/diff?')
              ? { volumeMm3: null, surfaceAreaMm2: null, bboxMm: null, centreMm: null, triangleCount: null, faceCount: null, edgeCount: null }
              : [],
      })),
    )
    const view = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Detail part={BRACKET} />
      </QueryClientProvider>,
    )
    const table = await screen.findByRole('table')
    const said = [...table.querySelectorAll('tr')]
      .find((tr) => tr.querySelector('th')?.textContent === strings.detail.centreAxis(0))
      ?.querySelector('td')?.textContent
    view.unmount()
    vi.unstubAllGlobals()
    return said
  }

  // Two STEP revisions with a volume and no centre: nothing this sweep will ever touch.
  expect(
    await pair([
      revision(second, '2', first, 'step', 39424),
      revision(first, '1', null, 'step', 35840),
    ]),
  ).toBe(strings.detail.notInBoth)

  // A mesh the sweep will fill beside an open mesh it never can. `every`, not `some`: claiming a job
  // for the pair would come true for one revision and never for the other.
  expect(
    await pair([
      revision(second, '2', first, 'stl', null),
      revision(first, '1', null, 'stl', 35840),
    ]),
  ).toBe(strings.detail.notInBoth)

  // Both meshes with a volume: this is the pair the sweep does queue.
  expect(
    await pair([
      revision(second, '2', first, 'stl', 39424),
      revision(first, '1', null, 'stl', 35840),
    ]),
  ).toBe(strings.detail.centreQueued)
})

test('a checked-out part names its holder, and its page can release the lock after a confirmation', async () => {
  const posts: { url: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'POST') {
        posts.push({ url, body: JSON.parse(init.body ?? 'null') })
        return { ok: true, status: 204, json: async () => null }
      }
      return { ok: true, status: 200, json: async () => [] }
    }),
  )
  const checkedOut: PartDetail = {
    ...BRACKET,
    lock: {
      id: '01931b6e-0000-7000-8000-00000000eeee',
      holder: 'mira@workshop-pc',
      takenAt: '2026-09-14T08:00:00Z',
    },
  }
  const page = (recordable: boolean) => (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={checkedOut} recordable={recordable} />
    </QueryClientProvider>
  )

  const quickLook = render(page(false))
  expect(
    screen.getByText(strings.detail.checkedOutBy('mira@workshop-pc', '2026-09-14T08:00:00Z')),
  ).toBeTruthy()
  expect(screen.queryByRole('button', { name: strings.detail.releaseLock })).toBeNull()
  quickLook.unmount()

  render(page(true))
  fireEvent.click(screen.getByRole('button', { name: strings.detail.releaseLock }))
  expect(screen.getByRole('dialog').textContent).toContain(
    strings.detail.releaseLockBody('mira@workshop-pc'),
  )
  expect(posts, 'nothing is released before the confirmation').toEqual([])
  fireEvent.click(screen.getByRole('button', { name: strings.detail.releaseLockConfirm }))
  await waitFor(() =>
    expect(posts).toEqual([
      {
        url: `/api/parts/${BRACKET.id}/lock/release`,
        body: { by: strings.detail.releasedBy },
      },
    ]),
  )
  vi.unstubAllGlobals()
})

/**
 * The comparison belongs to the part on screen. Coming back to a part whose history is already
 * cached keeps the History section mounted, so a comparison that remembered the last part's
 * revisions would ask this part about somebody else's, and be refused.
 */
test('the comparison follows the part on screen instead of keeping the last part’s revisions', async () => {
  const revision = (id: string, revLabel: string, parent: string | null): PartRevision => ({
    id,
    parent,
    revLabel,
    origin: parent === null ? 'ingest' : 'agent',
    createdAt: '2026-09-14T09:30:00Z',
    thumbnail: null,
    triangleCount: 44,
    faceCount: null,
    edgeCount: null,
    bboxMm: null,
    volumeMm3: null,
    surfaceAreaMm2: null,
    massG: null,
    centreMm: null,
    sourceHash: null,
    sourceFormat: 'stl',
    sourceBytes: 20124,
    deltaFromParent: null,
    tessellationL0: null,
    tessellationL1: null,
  })
  const [b1, b2] = ['01931b6e-0000-7000-8000-0000000000b1', '01931b6e-0000-7000-8000-0000000000b2']
  const [c1, c2] = ['01931b6e-0000-7000-8000-0000000000c1', '01931b6e-0000-7000-8000-0000000000c2']
  const histories: Record<string, PartRevision[]> = {
    [BRACKET.id]: [revision(b2, '2', b1), revision(b1, '1', null)],
    [PIN.id]: [revision(c2, '2', c1), revision(c1, '1', null)],
  }
  const noChange = { volumeMm3: null, surfaceAreaMm2: null, bboxMm: null, triangleCount: null, faceCount: null, edgeCount: null }
  const fetchMock = vi.fn(async (url: string) => {
    const part = Object.keys(histories).find((id) => url.startsWith(`/api/parts/${id}/`))
    const body =
      part !== undefined && url.endsWith('/revisions')
        ? histories[part]
        : url.includes('/diff?')
          ? noChange
          : []
    return { ok: true, status: 200, json: async () => body }
  })
  vi.stubGlobal('fetch', fetchMock)
  const diffs = () =>
    fetchMock.mock.calls.map(([url]) => url).filter((url) => url.includes('/diff?'))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const page = (part: PartDetail) => (
    <QueryClientProvider client={client}>
      <Detail part={part} />
    </QueryClientProvider>
  )

  const { rerender } = render(page(PIN))
  await waitFor(() => expect(diffs().at(-1)).toBe(`/api/parts/${PIN.id}/diff?from=${c1}&to=${c2}`))
  rerender(page(BRACKET))
  await waitFor(() =>
    expect(diffs().at(-1)).toBe(`/api/parts/${BRACKET.id}/diff?from=${b1}&to=${b2}`),
  )

  // Back to the pin, whose history is cached: the section stays mounted this time.
  rerender(page(PIN))
  await waitFor(() => expect(diffs().at(-1)).toBe(`/api/parts/${PIN.id}/diff?from=${c1}&to=${c2}`))
  expect(diffs(), 'never the bracket’s revisions asked of the pin').not.toContain(
    `/api/parts/${PIN.id}/diff?from=${b1}&to=${b2}`,
  )
  vi.unstubAllGlobals()
})

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
test('the comparison keeps its pickers and offers no ghost', async () => {
  const revision = (id: string, revLabel: string, parent: string | null): PartRevision => ({
    id,
    parent,
    revLabel,
    origin: parent === null ? 'ingest' : 'agent',
    createdAt: '2026-09-15T08:10:00Z',
    thumbnail: null,
    triangleCount: 44,
    faceCount: null,
    edgeCount: null,
    bboxMm: null,
    volumeMm3: null,
    surfaceAreaMm2: null,
    massG: null,
    centreMm: null,
    sourceHash: null,
    sourceFormat: 'stl',
    sourceBytes: 20124,
    deltaFromParent: null,
    tessellationL0: 'a0'.repeat(32),
    tessellationL1: null,
  })
  const [first, second] = ['01931b6e-0000-7000-8000-0000000000d1', '01931b6e-0000-7000-8000-0000000000d2']
  const history = [revision(second, '2', first), revision(first, '1', null)]
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith('/revisions')
          ? history
          : url.includes('/diff?')
            ? { volumeMm3: null, surfaceAreaMm2: null, bboxMm: null, triangleCount: null, faceCount: null, edgeCount: null }
            : [],
    })),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={BRACKET} />
    </QueryClientProvider>,
  )

  // The pickers are still there, which is what says this row was kept rather than emptied.
  const from = await screen.findByLabelText(strings.detail.compareFrom)
  expect(screen.getByLabelText(strings.detail.compareTo)).toBeTruthy()
  // Structurally, not by its old copy: the ghost's strings are gone from `strings.ts`, and a test
  // that kept one alive only to assert its absence would be the one reason the copy still existed.
  // The checkbox stood in this row beside the two pickers, so the row holding none is the assertion.
  const row = from.closest('div')
  expect(row?.querySelectorAll('input[type=checkbox]')).toHaveLength(0)
  vi.unstubAllGlobals()
})

/** The link that opens a part in a desktop app, offered only where a save can come back as a revision. */
test('a controlled library’s part offers to open in a desktop app, and a hobby library’s does not', async () => {
  const part = { ...BRACKET, sourceHash: '7'.repeat(64) }
  let mode: 'controlled' | 'hobby' = 'controlled'
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith('/api/libraries') ? [{ id: part.library, name: 'Workshop', mode, partCount: 6 }] : [],
    })),
  )
  const page = (client: QueryClient) => (
    <QueryClientProvider client={client}>
      <Detail part={part} />
    </QueryClientProvider>
  )

  const view = render(page(new QueryClient({ defaultOptions: { queries: { retry: false } } })))
  const link = await screen.findByRole('link', { name: strings.download.openInApp })
  expect(link.getAttribute('href')).toBe(`lapidary://open?part=${part.id}`)
  expect(screen.getByText(strings.download.openInAppNote)).toBeTruthy()
  view.unmount()

  mode = 'hobby'
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(page(client))
  // Settled, not merely asked: a link absent before the answer arrives proves nothing.
  await waitFor(() => expect(client.getQueryState(['libraries'])?.status).toBe('success'))
  expect(screen.queryByRole('link', { name: strings.download.openInApp })).toBeNull()
  expect(screen.queryByText(strings.download.openInAppNote)).toBeNull()
  vi.unstubAllGlobals()
})

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

test('a part pulled from somebody’s shared library says who it came from', () => {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200, json: async () => [] })))
  const pulled: PartDetail = {
    ...BRACKET,
    sharedBy: { deviceId: '7KQ2M-4XW9P-0HB3R', name: 'Ayşe’s workshop' },
  }
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={pulled} recordable={false} />
    </QueryClientProvider>,
  )
  expect(screen.getByText(strings.detail.sharedBy)).toBeTruthy()
  expect(screen.getByText('Ayşe’s workshop')).toBeTruthy()
})
