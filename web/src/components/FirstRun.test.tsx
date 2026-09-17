import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import { FirstRun } from './FirstRun'
import { strings } from '../lib/strings'

const { bench, stop, webgl } = vi.hoisted(() => {
  const stop = vi.fn()
  return { bench: vi.fn((_host: HTMLElement, _urls: readonly string[]) => stop), stop, webgl: { value: true } }
})
vi.mock('./turntable', () => ({ bench }))
vi.mock('../lib/viewer-math', async (original) => ({
  ...(await original<typeof import('../lib/viewer-math')>()),
  hasWebGL: () => webgl.value,
}))

afterEach(() => {
  bench.mockClear()
  stop.mockClear()
  webgl.value = true
})

test('an empty library offers Upload, and the button opens the picker', () => {
  const onUpload = vi.fn()
  render(<FirstRun onUpload={onUpload} />)
  const section = screen.getByRole('region', { name: strings.emptyLibrary.title })
  fireEvent.click(screen.getByRole('button', { name: strings.toolbar.upload }))
  expect(onUpload).toHaveBeenCalledTimes(1)
  expect(section.textContent).toContain(strings.emptyLibrary.dropHint)
})

test('the scene of example parts starts with the page and stops when it goes', async () => {
  const { unmount } = render(<FirstRun onUpload={() => {}} />)
  await act(async () => {
    await vi.dynamicImportSettled()
  })
  expect(bench).toHaveBeenCalledTimes(1)
  expect(bench.mock.calls[0]![1]).toEqual(['/first-run/spur-gear.glb', '/first-run/flange.glb', '/first-run/vee-block.glb'])
  expect(bench.mock.calls[0]![0].getAttribute('aria-hidden')).toBe('true')
  unmount()
  expect(stop).toHaveBeenCalledTimes(1)
})

test('where the browser cannot draw it, the words are the whole page', async () => {
  webgl.value = false
  const { container } = render(<FirstRun onUpload={() => {}} />)
  await act(async () => {
    await vi.dynamicImportSettled()
  })
  expect(bench).not.toHaveBeenCalled()
  expect(container.querySelector('.stage-lamp')).toBeNull()
})
