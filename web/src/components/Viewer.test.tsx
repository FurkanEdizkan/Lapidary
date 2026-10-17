import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import { strings } from '../lib/strings'
import type { PartDetail } from '../lib/types'

/**
 * **What the viewer asks for, and what it says when the answer cannot be drawn.**
 *
 * `PartDetail.test.tsx` mocks this component out entirely, so until this file existed nothing
 * checked the one line `R1`'s "done when" turns on: pressing **view** paints from
 * `?variant=original`. A mutation to `blobUrl(part.tessellationL0)` — the rung this replaced —
 * passed every test in the suite.
 *
 * jsdom has no WebGL, so three's renderer and its controls are the two things stubbed, and
 * nothing else: `parseMesh`, the fetch, the error mapping and the component's own states are the
 * real ones. A stub that replaced the whole module would make this a test of the stub.
 */
vi.mock('three', async (original) => {
  const three = await original<typeof import('three')>()
  class StubRenderer {
    domElement = document.createElement('canvas')
    setPixelRatio() {}
    setSize() {}
    render() {}
    dispose() {}
  }
  return { ...three, WebGLRenderer: StubRenderer }
})
// The axis gizmo draws its labels on a 2D canvas, which jsdom does not have.
vi.mock('three/addons/helpers/ViewHelper.js', () => ({
  ViewHelper: class {
    center = { set: () => {} }
    animating = false
    setLabels() {}
    render() {}
    handleClick() {
      return false
    }
    update() {}
    dispose() {}
  },
}))
vi.mock('three/addons/controls/OrbitControls.js', () => ({
  OrbitControls: class {
    target = { set: () => {} }
    addEventListener() {}
    update() {}
    dispose() {}
  },
}))

const { parse } = vi.hoisted(() => ({ parse: vi.fn() }))
vi.mock('./mesh', async (original) => {
  const mesh = await original<typeof import('./mesh')>()
  return { ...mesh, parseMesh: parse, loadMesh: async (format: string, bytes: ArrayBuffer) => parse(format, bytes) }
})

afterEach(async () => {
  // The viewer keeps the last model it drew (A11), and every test here draws the same part.
  ;(await import('./Viewer')).forgetLastDrawn()
  parse.mockReset()
  vi.unstubAllGlobals()
})

const PART = {
  id: '01931b6e-0000-7000-8000-00000000aaaa',
  revision: '01931b6e-0000-7000-8000-00000000bbbb',
  name: 'bracket-lp-1042-03',
  sourceFormat: 'stl',
} as unknown as PartDetail

/** The component under test, imported lazily so the mocks above are in place first. */
async function mount(part: PartDetail = PART, keep?: (png: Blob) => Promise<string | null>) {
  const { default: Viewer } = await import('./Viewer')
  // A host with a size, so the renderer's own `setSize` has something to be given.
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, value: 400 })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, value: 400 })
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    },
  )
  await act(async () => {
    render(<Viewer part={part} poster={null} keep={keep} />)
  })
}

/**
 * **The original bytes, by name.** `?variant=original` is the one variant that promises what was
 * ingested, byte for byte — `CLAUDE.md`: downloads are never silently converted — and it is the
 * only thing the browser can read a mesh out of now that nothing tessellates.
 */
test('the view fetches the revision’s original file and nothing else', async () => {
  const fetchMock = vi.fn(async () => ({ ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) }))
  vi.stubGlobal('fetch', fetchMock)
  parse.mockImplementation(async () => {
    throw new Error('not drawn in this test')
  })
  await mount()

  await waitFor(() => expect(fetchMock).toHaveBeenCalled())
  expect(fetchMock.mock.calls).toHaveLength(1)
  expect(String((fetchMock.mock.calls[0] as unknown as [string])[0])).toBe(
    `/api/revisions/${PART.revision}/download?variant=original`,
  )
})

/**
 * **Only our own sentences reach the screen.** A loader throws
 * `THREE.ThreeMFLoader: Cannot find relationship file …` and a failed download throws
 * `Failed to fetch`; neither says what to do and neither comes from `strings.ts`. The one
 * exception is `Unviewable`, which names the format — asserted in `mesh.test.ts`.
 */
test('a failure a reader cannot act on is reported in the words we wrote', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) })),
  )
  parse.mockImplementation(() => {
    throw new Error('THREE.ThreeMFLoader: Cannot find relationship file `rels` in 3MF archive.')
  })
  await mount()

  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toBe(strings.viewer.failed)
  expect(alert.textContent).not.toContain('THREE')
})

/**
 * A part with no source row has no file to read, and **says so** rather than leaving the loading
 * line up for ever. `Preview` makes this unreachable — it offers **view** only for a known mesh
 * format — and it is still written down, because the bug this whole goal was scheduled to fix was
 * precisely an effect that returned early and left nothing to diagnose (`Viewer.tsx`'s old rung
 * effect, on a part with no L0).
 */
test('a revision with no source file says so instead of loading for ever', async () => {
  const fetchMock = vi.fn(async () => ({ ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) }))
  vi.stubGlobal('fetch', fetchMock)
  await mount({ ...PART, sourceFormat: null } as unknown as PartDetail)

  expect((await screen.findByRole('alert')).textContent).toBe(strings.viewer.failed)
  expect(screen.queryByText(strings.viewer.loading)).toBeNull()
  expect(fetchMock).not.toHaveBeenCalled()
})

/**
 * **Keeping the view as the part's picture** (F1). The canvas as it is on screen goes to `keep` as a
 * PNG, and a refusal from the server is shown in its own words; with no `keep` there is no button.
 */
test('the view on screen can be kept as the picture, and a refusal says why', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) })),
  )
  const three = await import('three')
  parse.mockImplementation(
    () =>
      new three.Mesh(
        new three.BufferGeometry().setAttribute(
          'position',
          new three.Float32BufferAttribute([0, 0, 0, 40, 0, 0, 0, 60, 0], 3),
        ),
      ),
  )
  const png = new Blob(['a picture'], { type: 'image/png' })
  HTMLCanvasElement.prototype.toBlob = function (callback: BlobCallback) {
    callback(png)
  }
  const keep = vi.fn(async () => null as string | null)
  await mount(PART, keep)

  fireEvent.click(await screen.findByRole('button', { name: strings.viewer.keepView }))
  await waitFor(() => expect(keep).toHaveBeenCalledWith(png))
  expect(await screen.findByRole('button', { name: strings.viewer.keptView })).toBeTruthy()

  keep.mockImplementation(async () => 'That image is 40×60, and the smallest side must be at least 64 pixels.')
  fireEvent.click(screen.getByRole('button', { name: strings.viewer.keptView }))
  expect((await screen.findByRole('alert')).textContent).toContain('at least 64 pixels')
})

test('with nowhere to keep it, the view offers no button', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: true, status: 200, arrayBuffer: async () => new ArrayBuffer(8) })),
  )
  const three = await import('three')
  parse.mockImplementation(
    () =>
      new three.Mesh(
        new three.BufferGeometry().setAttribute(
          'position',
          new three.Float32BufferAttribute([0, 0, 0, 40, 0, 0, 0, 60, 0], 3),
        ),
      ),
  )
  await mount(PART)
  await waitFor(() => expect(parse).toHaveBeenCalled())
  expect(screen.queryByRole('button', { name: strings.viewer.keepView })).toBeNull()
})
