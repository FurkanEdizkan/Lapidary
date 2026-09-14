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
  drawn: { parts: 3 as number | null, hidden: [] as number[] },
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
    }: {
      part: PartDetail
      hidden?: ReadonlySet<number>
      onParts?: (parts: number | null) => void
    }) {
      useEffect(() => {
        mounts.push(part.id)
      }, [])
      useEffect(() => {
        onParts?.(drawn.parts)
      }, [onParts])
      drawn.hidden = [...(hidden ?? [])].sort((a, b) => a - b)
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
  pmi: null,
  sourcePath: 'cad/angle-bracket-60x60x40-lp-9004-00.igs',
  thumbnail: null,
  triangleCount: 44,
  isWatertight: true,
  bboxMm: { value: [60, 40, 60], approximate: false },
  volumeMm3: { value: 35840, approximate: false },
  surfaceAreaMm2: { value: 11392, approximate: false },
  kernelVersion: 'occt occt-8.0.1-bridge-4+deflection-0.1+glb-1+cpu-1',
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
    bboxMm: { value: [66, 40, 60], approximate: true },
    volumeMm3: { value: 39424, approximate: true },
    surfaceAreaMm2: null,
    sourceHash: '6666666666666666666666666666666666666666666666666666666666666666',
    sourceFormat: 'stl',
    sourceBytes: 20124,
  } satisfies PartRevision
  const first = {
    ...second,
    id: BRACKET.revision,
    parent: null,
    revLabel: '1',
    origin: 'ingest',
    createdAt: '2026-09-13T10:00:00Z',
    volumeMm3: { value: 35840, approximate: true },
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
  const item = (label: string) =>
    screen.getByText(strings.detail.historyRevision(label)).closest('li')
  expect(item('2')?.textContent).toContain(strings.detail.origin.agent)
  expect(item('2')?.textContent).toContain(strings.detail.approximate)
  expect(item('1')?.textContent).toContain(strings.detail.origin.ingest)
  expect(item('2')?.querySelector('a')?.getAttribute('href')).toBe(
    `/api/revisions/${second.id}/download?variant=original`,
  )
  vi.unstubAllGlobals()
})
