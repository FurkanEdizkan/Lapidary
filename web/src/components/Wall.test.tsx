import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import Wall, { pictureAt } from './Wall'
import { strings } from '../lib/strings'
import type { WallPicture } from '../lib/types'

afterEach(() => {
  vi.unstubAllGlobals()
})

const picture = (n: number): WallPicture => ({ src: `data:image/webp;base64,${n}`, fit: 'cover', focusX: 0.5, focusY: 0.5 })

function wall(pictures: WallPicture[], motion: boolean) {
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: !motion })))
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(pictures))))
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <Wall />
    </QueryClientProvider>,
  )
}

test('a fresh install is all frames; a few pictures show once each among them; many fill the wall', () => {
  expect(Array.from({ length: 30 }, (_, i) => pictureAt([], 30, i)).filter(Boolean)).toEqual([])
  const few = [picture(1), picture(2), picture(3)]
  expect(Array.from({ length: 30 }, (_, i) => pictureAt(few, 30, i)).filter(Boolean)).toEqual(few)
  const many = Array.from({ length: 48 }, (_, i) => picture(i))
  expect(Array.from({ length: 30 }, (_, i) => pictureAt(many, 30, i)).every(Boolean)).toBe(true)
})

test('the wall is decorative, each picture twice so its columns loop, and Pause holds it', async () => {
  const { container } = wall([picture(1), picture(2)], true)
  const tiles = container.querySelector('[data-wall]')!
  expect(tiles.getAttribute('aria-hidden')).toBe('true')
  await waitFor(() => expect(tiles.querySelectorAll('img')).toHaveLength(4))
  for (const img of tiles.querySelectorAll('img')) expect(img.getAttribute('alt')).toBe('')
  fireEvent.click(screen.getByRole('button', { name: strings.doorway.pauseWall }))
  expect(screen.getByRole('button', { name: strings.doorway.playWall }).getAttribute('aria-pressed')).toBe('true')
  expect(tiles.className).toContain('wall-paused')
})

test('under reduced motion the wall stands still and has nothing to pause', () => {
  const { container } = wall([], false)
  expect(container.querySelector('[data-wall]')!.className).not.toContain('wall-moving')
  expect(screen.queryByRole('button')).toBeNull()
})
