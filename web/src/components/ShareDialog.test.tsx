import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, expect, test, vi } from 'vitest'
import { strings } from '../lib/strings'
import type { PartId } from '../lib/types'
import { ShareDialog } from './ShareDialog'

afterEach(() => vi.unstubAllGlobals())

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
}

const FLANGE = '01931b6e-0000-7000-8000-0000000b0001' as PartId
const GEAR = '01931b6e-0000-7000-8000-0000000b0002' as PartId
const MIRA = '01931b6e-0000-7000-8000-00000000b002'

test('several models get exactly the people ticked, one PUT each', async () => {
  const puts: [string, unknown][] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') {
        puts.push([url, JSON.parse(String(init.body))])
        return json(200, {})
      }
      return json(200, [{ id: MIRA, username: 'mira.okafor', displayName: 'Mira Okafor', isAdmin: false }])
    }),
  )
  const onClose = vi.fn()
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ShareDialog parts={[FLANGE, GEAR]} onClose={onClose} />
    </QueryClientProvider>,
  )
  expect(screen.getByText(strings.sharing.title(2))).toBeTruthy()
  fireEvent.click(await screen.findByLabelText('Mira Okafor', { exact: false }))
  fireEvent.click(screen.getByRole('button', { name: strings.sharing.save }))
  await waitFor(() => expect(onClose).toHaveBeenCalled())
  expect(puts).toEqual([
    [`/api/parts/${FLANGE}/shares`, { everyone: false, people: [MIRA] }],
    [`/api/parts/${GEAR}/shares`, { everyone: false, people: [MIRA] }],
  ])
})
