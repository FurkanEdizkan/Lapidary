import { useEffect, useRef, useState } from 'react'
import { lattice } from '../lib/lattice'
import { drawIn, reduced } from '../lib/motion'

/** The whole run, first line to last settled: inside WCAG 2.2.2's five seconds with room to spare. */
const TOTAL = 2400

/** Computed once: the geometry never changes, and two renders must draw the same part. */
const GEAR = lattice()

/** Near edges in Graphite, far ones in the control edge's grey and fainter, so the part reads in depth. */
function tone(depth: number): { stroke: string; strokeOpacity: number } {
  return {
    stroke: depth < 0.5 ? 'var(--color-muted)' : 'var(--color-edge)',
    strokeOpacity: Math.round((1 - 0.55 * depth) * 100) / 100,
  }
}

/**
 * The sign-in page's drawing (A19): a gear's wireframe that draws itself once, line by line, with a short accent head
 * at the drawing's front, then stays grey and still. Decorative, so hidden from assistive technology. Under reduced
 * motion it is simply drawn, and the accent is never rendered at all.
 *
 * The default export is for `lazy()`: anime.js arrives with this chunk, never on the grid's path.
 */
export default function Lattice({ className }: { className?: string }) {
  const lines = useRef<SVGGElement>(null)
  const heads = useRef<SVGGElement>(null)
  const [drawing, setDrawing] = useState(() => !reduced())

  useEffect(() => {
    if (lines.current === null) return
    const paths = (group: SVGGElement | null) =>
      group === null ? [] : Array.from(group.querySelectorAll<SVGGeometryElement>('path'))
    return drawIn(paths(lines.current), () => setDrawing(false), TOTAL, paths(heads.current))
  }, [])

  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={GEAR.viewBox}
      fill="none"
      strokeWidth={1}
      strokeLinecap="round"
      className={`transition-none ${className ?? ''}`}
    >
      <g ref={lines}>
        {GEAR.edges.map(({ d, depth }) => (
          <path key={d} d={d} vectorEffect="non-scaling-stroke" {...tone(depth)} />
        ))}
      </g>
      {drawing ? (
        <g ref={heads} stroke="var(--color-accent)" data-draw-head="">
          {GEAR.edges.map(({ d }) => (
            <path key={d} d={d} vectorEffect="non-scaling-stroke" className="transition-none" />
          ))}
        </g>
      ) : null}
    </svg>
  )
}
