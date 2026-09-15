import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { strings } from '../lib/strings'
import { DensitiesMenuItem, gramsPerCm3, kgPerM3 } from './Densities'

const LIBRARY = '01931b6e-0000-7000-8000-000000000001'

test('a density typed in g/cm³ is stored in kg/m³, and shown the same way it was typed', () => {
  expect(kgPerM3('7.85')).toBe(7850)
  expect(kgPerM3(' 2,7 ')).toBe(2700)
  expect(kgPerM3('heavy')).toBeNull()
  expect(kgPerM3('')).toBeNull()
  expect(gramsPerCm3(7850)).toBe('7.85')
  expect(gramsPerCm3(2710)).toBe('2.71')
})

/** The facet's materials and those with a density are listed, and a density typed in g/cm³ is sent in kg/m³. */
test('the dialog lists materials held and materials with a density, and saves one in kg/m³', async () => {
  const puts: { url: string; body: unknown }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'PUT') {
        puts.push({ url, body: JSON.parse(init.body ?? 'null') })
        return { ok: true, status: 204, json: async () => null }
      }
      return {
        ok: true,
        status: 200,
        json: async () =>
          url.includes('/facets')
            ? { formats: [], materials: [{ value: 'AISI 1045 steel', count: 3 }], tags: [] }
            : url.endsWith('/densities')
              ? [{ material: 'EN AW-6082 T6', densityKgM3: 2710 }]
              : [],
      }
    }),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DensitiesMenuItem library={LIBRARY} />
    </QueryClientProvider>,
  )

  fireEvent.click(screen.getByRole('button', { name: strings.densities.menu }))
  const steel = (await screen.findByLabelText(strings.densities.field('AISI 1045 steel'))) as HTMLInputElement
  const aluminium = screen.getByLabelText(strings.densities.field('EN AW-6082 T6')) as HTMLInputElement
  expect(steel.value).toBe('')
  expect(aluminium.value).toBe('2.71')

  fireEvent.change(steel, { target: { value: '7.85' } })
  fireEvent.submit(steel.closest('form') as HTMLFormElement)
  await waitFor(() =>
    expect(puts).toEqual([{ url: `/api/libraries/${LIBRARY}/densities/AISI%201045%20steel`, body: { densityKgM3: 7850 } }]),
  )
  vi.unstubAllGlobals()
})

test('a density out of range is refused in the unit it was typed in, not the server’s', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string }) =>
      init?.method === 'PUT'
        ? {
            ok: false,
            status: 400,
            json: async () => ({
              reason: 'badDensity',
              message: 'A density is a number of kilograms per cubic metre above 0 and below 25,000, such as 7850 for steel. Type it again.',
            }),
          }
        : {
            ok: true,
            status: 200,
            json: async () =>
              url.includes('/facets') ? { formats: [], materials: [{ value: 'AISI 1045 steel', count: 3 }], tags: [] } : [],
          },
    ),
  )
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DensitiesMenuItem library={LIBRARY} />
    </QueryClientProvider>,
  )

  fireEvent.click(screen.getByRole('button', { name: strings.densities.menu }))
  const steel = (await screen.findByLabelText(strings.densities.field('AISI 1045 steel'))) as HTMLInputElement
  fireEvent.change(steel, { target: { value: '30' } })
  fireEvent.submit(steel.closest('form') as HTMLFormElement)
  expect(await screen.findByText(strings.densities.outOfRange)).toBeTruthy()
  vi.unstubAllGlobals()
})
