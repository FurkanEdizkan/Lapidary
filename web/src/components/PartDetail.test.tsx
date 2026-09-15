import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { expect, test, vi } from 'vitest'
import { Detail, warmViewer, warmViewerWhenIdle } from './PartDetail'
import { strings } from '../lib/strings'
import type { AssemblyNode, AssemblyTree, PartDetail, PartRevision } from '../lib/types'

const { mounts, prepare, drawn } = vi.hoisted(() => ({
  mounts: [] as string[],
  prepare: vi.fn(async () => {}),
  // What the stand-in view reports drawing, and the parts it was last told to hide.
  drawn: { parts: 3 as number | null, hidden: [] as number[], ghost: null as string | null, annotated: false },
}))

// jsdom draws no WebGL, so the view is a stand-in that records each time it is mounted.
vi.mock('../lib/viewer-math', async (original) => ({
  ...(await original<typeof import('../lib/viewer-math')>()),
  hasWebGL: () => true,
}))
vi.mock('./Viewer', async () => {
  const { useEffect } = await import('react')
  return {
    default: function Viewer({
      part,
      hidden,
      onParts,
      ghost,
      annotated,
    }: {
      part: PartDetail
      hidden?: ReadonlySet<number>
      onParts?: (parts: number | null) => void
      ghost?: string | null
      annotated?: boolean
    }) {
      useEffect(() => {
        mounts.push(part.id)
      }, [])
      useEffect(() => {
        onParts?.(drawn.parts)
      }, [onParts])
      drawn.hidden = [...(hidden ?? [])].sort((a, b) => a - b)
      drawn.ghost = ghost ?? null
      drawn.annotated = annotated ?? false
      return null
    },
    prepare,
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
  kernelVersion: 'occt occt-8.0.1-bridge-4+deflection-0.1+glb-1+cpu-1',
  lock: null,
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

test('the 3D view starts over for another part, and keeps its place for a finer rung of the same one', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, status: 200, json: async () => [] })))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const page = (part: PartDetail) => (
    <QueryClientProvider client={client}>
      <Detail part={part} />
    </QueryClientProvider>
  )
  const { rerender } = render(page(BRACKET))
  await waitFor(() => expect(mounts).toEqual([BRACKET.id]))

  rerender(page({ ...BRACKET, tessellationL1: '4444444444444444444444444444444444444444444444444444444444444444' }))
  rerender(page(PIN))
  await waitFor(() => expect(mounts).toEqual([BRACKET.id, PIN.id]))
  vi.unstubAllGlobals()
})

/** A hovered card warms the view: its chunk is fetched and its shaders compiled before the open. */
test('warming the viewer prepares it', async () => {
  await warmViewer()
  expect(prepare).toHaveBeenCalledTimes(1)
})

/** A tap, or a link straight to a part, has no hover to warm on, so a screen warms once the browser is idle. */
test('warming when idle waits for the browser to be idle, and can be called off', async () => {
  let idle: (() => void) | undefined
  const cancel = vi.fn()
  vi.stubGlobal('requestIdleCallback', (callback: () => void) => {
    idle = callback
    return 7
  })
  vi.stubGlobal('cancelIdleCallback', cancel)
  prepare.mockClear()
  const stop = warmViewerWhenIdle()
  await Promise.resolve()
  expect(prepare).not.toHaveBeenCalled()
  idle?.()
  await waitFor(() => expect(prepare).toHaveBeenCalledTimes(1))
  stop()
  expect(cancel).toHaveBeenCalledWith(7)
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
  const page = (recordable: boolean) => (
    <QueryClientProvider client={client}>
      <Detail part={tagged} recordable={recordable} />
    </QueryClientProvider>
  )
  const { rerender } = render(page(true))

  fireEvent.change(screen.getByLabelText(strings.tags.field), { target: { value: 'spare' } })
  fireEvent.click(screen.getByRole('button', { name: strings.tags.add }))
  await waitFor(() => expect(puts).toEqual([{ tags: ['welding jig', 'spare'] }]))
  await waitFor(() => expect(screen.getByRole('button', { name: strings.tags.add })).toBeTruthy())

  fireEvent.click(screen.getByRole('button', { name: strings.tags.remove('welding jig') }))
  await waitFor(() => expect(puts).toEqual([{ tags: ['welding jig', 'spare'] }, { tags: [] }]))

  rerender(page(false))
  expect(screen.getByText('welding jig')).toBeTruthy()
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

/** A branch hides and shows all of its parts, Isolate hides every other, and Show all undoes either. */
test('the assembly tree hides, shows and isolates parts in the view', async () => {
  drawn.parts = 3
  renderAssembly()
  const station = 'bracket-station-lp-9002-00'

  fireEvent.click(await screen.findByRole('button', { name: strings.detail.hidePart(station) }))
  await waitFor(() => expect(drawn.hidden).toEqual([1, 2]))
  fireEvent.click(screen.getByRole('button', { name: strings.detail.showPart(station) }))
  await waitFor(() => expect(drawn.hidden).toEqual([]))

  fireEvent.click(screen.getByRole('button', { name: strings.detail.isolatePart('stop-pin-d10x20-lp-9007-00') }))
  await waitFor(() => expect(drawn.hidden).toEqual([0, 1]))
  fireEvent.click(screen.getByRole('button', { name: strings.detail.showAll }))
  await waitFor(() => expect(drawn.hidden).toEqual([]))
  vi.unstubAllGlobals()
})

/** A view that drew no parts to hide, such as an older rung, leaves the tree without the buttons. */
test('the tree offers no hiding when the view did not draw its parts', async () => {
  drawn.parts = null
  renderAssembly()
  expect(await screen.findByText('bracket-station-lp-9002-00')).toBeTruthy()
  expect(screen.queryByRole('button', { name: strings.detail.hidePart('bracket-station-lp-9002-00') })).toBeNull()
  drawn.parts = 3
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

/** The list can be drawn in the view, and says which annotations the view has no face to put beside. */
test('dimensions and tolerances can be shown in the 3D view, and say which the view cannot place', async () => {
  const pmiHash = '7777777777777777777777777777777777777777777777777777777777777777'
  const entitiesHash = '8888888888888888888888888888888888888888888888888888888888888888'
  const cylinder = { prototype: '0:1:1:1', face: 1 }
  const freeform = { prototype: '0:1:1:1', face: 9 }
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({
      ok: true,
      status: 200,
      json: async () =>
        url.endsWith(pmiHash)
          ? {
              dimensions: [{ type: 'diameter', value: 22, upper: 0.05, lower: 0, faces: [cylinder] }],
              tolerances: [{ type: 'profile_of_surface', value: 0.1, datums: [], faces: [freeform] }],
              datums: [],
            }
          : url.endsWith(entitiesHash)
            ? [{ type: 'cylinder', prototype: '0:1:1:1', face: 1, radius: 11, origin: [0, 0, 0], axis: [0, 0, 1] }]
            : [],
    })),
  )
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <Detail part={{ ...BRACKET, pmi: pmiHash, entities: entitiesHash, tessellationL0: 'a0'.repeat(32) }} />
    </QueryClientProvider>,
  )

  const toggle = await screen.findByRole('button', { name: strings.pmi.showInView })
  expect(toggle.getAttribute('aria-pressed')).toBe('false')
  expect(drawn.annotated).toBe(false)
  const item = (text: string) => screen.getByText(text).closest('li')?.textContent ?? ''
  await waitFor(() => expect(item(strings.pmi.dimension('diameter', 22, 0.05, 0))).toContain(strings.pmi.face('cylinder')))
  expect(item(strings.pmi.tolerance('profile_of_surface', 0.1, []))).not.toContain(strings.pmi.notDrawn(false))

  fireEvent.click(toggle)
  expect(toggle.getAttribute('aria-pressed')).toBe('true')
  await waitFor(() => expect(drawn.annotated).toBe(true))
  expect(item(strings.pmi.tolerance('profile_of_surface', 0.1, []))).toContain(strings.pmi.notDrawn(false))
  expect(item(strings.pmi.dimension('diameter', 22, 0.05, 0))).not.toContain(strings.pmi.notDrawn(false))
  vi.unstubAllGlobals()
})

/** Faces that could not be read are said once, not blamed on each annotation as a face the view cannot place. */
test('when the faces cannot be read, the list says so once rather than calling every annotation undrawable', async () => {
  const pmiHash = '7777777777777777777777777777777777777777777777777777777777777777'
  const entitiesHash = '8888888888888888888888888888888888888888888888888888888888888888'
  const cylinder = { prototype: '0:1:1:1', face: 1 }
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) =>
      url.endsWith(entitiesHash)
        ? { ok: false, status: 500, json: async () => ({ message: 'The entities blob is missing from the store.' }) }
        : {
            ok: true,
            status: 200,
            json: async () =>
              url.endsWith(pmiHash)
                ? { dimensions: [{ type: 'diameter', value: 22, upper: 0.05, lower: 0, faces: [cylinder] }], tolerances: [], datums: [] }
                : [],
          },
    ),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={{ ...BRACKET, pmi: pmiHash, entities: entitiesHash, tessellationL0: 'a0'.repeat(32) }} />
    </QueryClientProvider>,
  )

  fireEvent.click(await screen.findByRole('button', { name: strings.pmi.showInView }))
  expect(await screen.findByText(strings.pmi.facesUnread)).toBeTruthy()
  const item = screen.getByText(strings.pmi.dimension('diameter', 22, 0.05, 0)).closest('li')?.textContent ?? ''
  expect(item).not.toContain(strings.pmi.notDrawn(false))
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
  // A centre of mass neither revision recorded is said so per axis, not shown as no movement.
  expect(table.textContent).toContain(strings.detail.centreAxis(2))
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

/** The comparison's From revision, drawn as a ghost in the 3D view, and said plainly when it cannot be. */
test('the comparison draws its From revision as a ghost, and says when that revision has only a coarse mesh or none', async () => {
  const revision = (
    id: string,
    revLabel: string,
    parent: string | null,
    rungs: { l0: string | null; l1: string | null },
  ): PartRevision => ({
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
    tessellationL0: rungs.l0,
    tessellationL1: rungs.l1,
  })
  const [first, second] = ['01931b6e-0000-7000-8000-0000000000d1', '01931b6e-0000-7000-8000-0000000000d2']
  const [coarse, fine0, fine1] = ['a0'.repeat(32), 'b0'.repeat(32), 'b1'.repeat(32)]
  const newest = revision(second, '2', first, { l0: fine0, l1: fine1 })
  let history = [newest, revision(first, '1', null, { l0: coarse, l1: null })]
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
  const page = () => (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Detail part={BRACKET} />
    </QueryClientProvider>
  )

  const view = render(page())
  const box = await screen.findByLabelText(strings.detail.ghost)
  expect(drawn.ghost).toBeNull()
  fireEvent.click(box)
  // Revision 1 was not opened again after revision 2 arrived: its ghost is the L0 ingest wrote.
  await waitFor(() => expect(drawn.ghost).toBe(coarse))
  expect(screen.getByText(strings.detail.ghostCoarse)).toBeTruthy()
  fireEvent.click(box)
  await waitFor(() => expect(drawn.ghost).toBeNull())
  view.unmount()

  // A From revision with no mesh at all offers no ghost, and says why.
  history = [newest, revision(first, '1', null, { l0: null, l1: null })]
  render(page())
  expect(await screen.findByText(strings.detail.ghostNoMesh('1'))).toBeTruthy()
  expect(screen.queryByLabelText(strings.detail.ghost)).toBeNull()
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
