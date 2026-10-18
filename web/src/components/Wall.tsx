import { useQuery } from '@tanstack/react-query'
import { useState, type CSSProperties } from 'react'
import { fetchWall } from '../lib/api'
import { reduced } from '../lib/motion'
import { strings } from '../lib/strings'
import type { WallPicture } from '../lib/types'
import { Icon } from './Icon'

/** Columns at `lg`, and below it in the Doorway's band; each holds `ROWS` tiles, twice, so it scrolls without a seam. */
const COLUMNS = 5
const ROWS = 6

/**
 * Which picture tile `index` of `tiles` shows: with as many pictures as tiles they repeat to fill the wall; with
 * fewer, each shows once, spread evenly among empty frames. A fresh install is all frames.
 */
export function pictureAt(pictures: WallPicture[], tiles: number, index: number): WallPicture | undefined {
  if (pictures.length === 0) return undefined
  const step = Math.max(1, Math.floor(tiles / pictures.length))
  return index % step === 0 ? pictures[(index / step) % pictures.length] : undefined
}

/**
 * The sign-in wall (A28): columns of cards drifting past, each the first picture of a part shared with everyone and not
 * hidden from the wall, and nothing else about it. Decorative, so hidden from assistive technology; it loops, so it has
 * a Pause (WCAG 2.2.2). Under reduced motion it stands still and has none. The columns are a CSS animation of
 * transform only, which a hidden tab does not run.
 */
export default function Wall() {
  const wall = useQuery({ queryKey: ['wall'], queryFn: fetchWall, staleTime: Infinity, retry: false })
  const [paused, setPaused] = useState(false)
  const moving = !reduced()
  const pictures = wall.data ?? []
  return (
    <div className="absolute inset-0">
      <div
        aria-hidden="true"
        data-wall
        className={`wall absolute inset-0 grid gap-3 overflow-hidden px-3 lg:gap-4 lg:px-4 ${moving ? 'wall-moving' : ''} ${paused ? 'wall-paused' : ''}`}
        style={{ gridTemplateColumns: `repeat(${COLUMNS}, minmax(0, 1fr))` }}
      >
        {Array.from({ length: COLUMNS }, (_, column) => (
          <div key={column} className="wall-column" style={{ '--wall-column': column } as CSSProperties}>
            {[0, 1].map((half) =>
              Array.from({ length: ROWS }, (_, row) => {
                const picture = pictureAt(pictures, COLUMNS * ROWS, column * ROWS + row)
                return (
                  <div key={`${half}-${row}`} className="pb-3 lg:pb-4">
                    <div
                      className={`wall-tile relative aspect-square overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] ${picture === undefined ? '' : 'bg-[var(--color-surface)]'} ${(column * 7 + row) % 11 === 3 ? 'wall-found' : ''}`}
                    >
                      {picture === undefined ? null : (
                        <img
                          src={picture.src}
                          alt=""
                          loading="lazy"
                          decoding="async"
                          className="size-full opacity-70 transition-none"
                          style={{
                            objectFit: picture.fit === 'contain' ? 'contain' : 'cover',
                            objectPosition: `${picture.focusX * 100}% ${picture.focusY * 100}%`,
                          }}
                        />
                      )}
                    </div>
                  </div>
                )
              }),
            )}
          </div>
        ))}
      </div>
      {/* The mark and the pitch stand on the wall's foot, so it fades into the page there. */}
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-2/5 bg-gradient-to-t from-[var(--color-bg)] to-transparent" />
      {moving ? (
        <button
          type="button"
          aria-pressed={paused}
          onClick={() => setPaused(!paused)}
          className="ease-mechanical absolute top-3 right-3 z-10 flex items-center gap-1.5 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] lg:top-auto lg:right-6 lg:bottom-6"
        >
          <Icon name={paused ? 'play' : 'pause'} />
          {paused ? strings.doorway.playWall : strings.doorway.pauseWall}
        </button>
      ) : null}
    </div>
  )
}
