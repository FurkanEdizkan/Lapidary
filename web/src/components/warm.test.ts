import { waitFor } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { loadViewerWhenIdle, warmViewer } from './PartDetail'

const { imported, prepare } = vi.hoisted(() => ({
  imported: { count: 0 },
  prepare: vi.fn(async () => {}),
}))

// jsdom draws no WebGL, so the viewer is a stand-in that counts how often its chunk is loaded. Nothing else in
// this file loads it, so the count is the warm-up's alone.
vi.mock('../lib/viewer-math', async (original) => ({
  ...(await original<typeof import('../lib/viewer-math')>()),
  hasWebGL: () => true,
}))
vi.mock('./Viewer', () => {
  imported.count += 1
  return { default: () => null, prepare }
})

/**
 * A tap, or a press before the pointer rested, has no hover to warm on, so the grid loads the viewer's chunk
 * once the browser is idle: its code only, so a visitor who never opens a part holds no WebGL context.
 */
test('when idle the grid loads the viewer code and prepares nothing, and can be called off', async () => {
  let idle: (() => void) | undefined
  const cancel = vi.fn()
  vi.stubGlobal('requestIdleCallback', (callback: () => void) => {
    idle = callback
    return 7
  })
  vi.stubGlobal('cancelIdleCallback', cancel)
  const stop = loadViewerWhenIdle()
  await Promise.resolve()
  expect(imported.count).toBe(0)

  idle?.()
  await waitFor(() => expect(imported.count).toBe(1))
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(prepare).not.toHaveBeenCalled()
  stop()
  expect(cancel).toHaveBeenCalledWith(7)
  vi.unstubAllGlobals()
})

/** A hovered card warms the view: its chunk is fetched and its shaders compiled before the open. */
test('hovering a card prepares the viewer', async () => {
  await warmViewer()
  expect(prepare).toHaveBeenCalledTimes(1)
})
