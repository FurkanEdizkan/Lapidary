import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, test, vi } from 'vitest'
import { strings } from '../lib/strings'
import type { LibraryId } from '../lib/types'
import { ImportDialog } from './ImportDialog'

afterEach(() => vi.unstubAllGlobals())

const LIBRARY = '01931b6e-0000-7000-8000-000000000001' as LibraryId

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

test('a folder is opened, a file ticked and added with its page and picture, then the library is scanned', async () => {
  const calls: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push(`${init?.method ?? 'GET'} ${url}`)
      if (url.startsWith('/api/storage/browse?path=Printables')) {
        return json(200, {
          path: 'Printables',
          truncated: false,
          entries: [{ name: '3DBenchy.stl', path: 'Printables/3DBenchy.stl', isFolder: false, sizeBytes: 11_285_384 }],
        })
      }
      if (url.startsWith('/api/storage/browse')) {
        return json(200, {
          path: '',
          truncated: false,
          entries: [{ name: 'Printables', path: 'Printables', isFolder: true, sizeBytes: null }],
        })
      }
      if (url === '/api/settings/import') return json(200, { fetchDetails: true })
      if (url.endsWith('/sources') && init?.method === 'POST') {
        return json(201, { id: 'b7a1', path: 'Printables/3DBenchy.stl', isFolder: false, pageUrl: 'x', fetchDetails: true })
      }
      if (url.endsWith('/pictures')) return new Response(null, { status: 204 })
      return json(200, [])
    }),
  )
  const onScan = vi.fn()
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ImportDialog library={LIBRARY} onClose={() => {}} onScan={onScan} onUpload={() => {}} onFromLink={() => {}} />
    </QueryClientProvider>,
  )
  fireEvent.click(await screen.findByRole('button', { name: strings.addModels.open('Printables') }))
  fireEvent.click(await screen.findByRole('checkbox', { name: strings.addModels.select('3DBenchy.stl') }))
  fireEvent.change(screen.getByLabelText(strings.addModels.pageLabel, { exact: false }), {
    target: { value: 'https://www.printables.com/model/3161-3d-benchy' },
  })
  const picture = new File([new Uint8Array([0x52, 0x49, 0x46, 0x46])], 'benchy.webp', { type: 'image/webp' })
  fireEvent.change(screen.getByLabelText(strings.addModels.picturesLabel, { exact: false }), {
    target: { files: [picture] },
  })
  fireEvent.click(screen.getByRole('button', { name: strings.addModels.add(1) }))
  await waitFor(() => expect(onScan).toHaveBeenCalledTimes(1))
  const writes = calls.filter((call) => !call.startsWith('GET'))
  expect(writes).toEqual([
    `POST /api/libraries/${LIBRARY}/sources`,
    `POST /api/libraries/${LIBRARY}/sources/b7a1/pictures`,
  ])
  expect(screen.getByRole('status').textContent).toBe(strings.addModels.added(1))
})
