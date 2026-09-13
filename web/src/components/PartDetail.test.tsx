import { render, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { expect, test, vi } from 'vitest'
import { Detail } from './PartDetail'
import type { PartDetail } from '../lib/types'

const { mounts } = vi.hoisted(() => ({ mounts: [] as string[] }))

// jsdom draws no WebGL, so the view is a stand-in that records each time it is mounted.
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
      }, [])
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
