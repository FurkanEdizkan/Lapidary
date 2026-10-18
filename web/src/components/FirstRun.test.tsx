import { act, fireEvent, render, screen } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { FirstRun } from './FirstRun'
import { strings } from '../lib/strings'

test('an empty library offers Import first and Upload second, and each opens its own way in', () => {
  const onUpload = vi.fn()
  const onImport = vi.fn()
  render(<FirstRun onImport={onImport} onUpload={onUpload} />)
  const section = screen.getByRole('region', { name: strings.emptyLibrary.title })
  const [first, second] = screen.getAllByRole('button')
  expect(first?.textContent).toBe(strings.addModels.button)
  expect(second?.textContent).toBe(strings.toolbar.uploadFolder)
  fireEvent.click(screen.getByRole('button', { name: strings.addModels.button }))
  expect(onImport).toHaveBeenCalledTimes(1)
  fireEvent.click(screen.getByRole('button', { name: strings.toolbar.uploadFolder }))
  expect(onUpload).toHaveBeenCalledTimes(1)
  expect(section.textContent).toContain(strings.emptyLibrary.dropHint)
})

/**
 * **The turning bench is gone** (R1), and what stood here asserted it was drawn.
 *
 * It was three bundled glTF rungs on a canvas, which put the renderer and `turntable.ts` into the
 * one page in the application that has nothing of its own to render — and nothing produces rungs
 * any more. So this is the opposite assertion: an empty library loads no renderer and asks for no
 * file, which is also what made the scene wrong in a browser that draws no 3D.
 *
 * `fetch` as well as the canvas, for the same reason `Grid.test.tsx` counts it: jsdom has no WebGL,
 * so a canvas that was attempted never gets appended, and "no canvas" alone would pass against a
 * page that tried and failed.
 */
test('an empty library loads no renderer and fetches nothing', async () => {
  const fetchMock = vi.fn(async () => ({ ok: true, status: 200, json: async () => ({}) }))
  vi.stubGlobal('fetch', fetchMock)
  const { container } = render(<FirstRun onImport={() => {}} onUpload={() => {}} />)
  await act(async () => {
    await vi.dynamicImportSettled()
  })
  expect(fetchMock).not.toHaveBeenCalled()
  expect(container.querySelector('canvas')).toBeNull()
  expect(container.querySelector('.stage-lamp')).toBeNull()
  vi.unstubAllGlobals()
})
