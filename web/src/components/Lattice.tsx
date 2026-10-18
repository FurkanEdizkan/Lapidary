import { useEffect, useRef, useState } from 'react'
import { lattice, type LatticeEdge } from '../lib/lattice'
import { drawIn, reduced } from '../lib/motion'

/** Computed once: the geometry never changes, and two renders must draw the same part. */
const GEAR = lattice()

/**
 * Settled strokes (DESIGN.md § Lattice): Edge for the outline, farther edges fainter so the part reads in depth, and
 * the Scribe Line for the face triangulation, so the outline reads first and the mesh is texture.
 */
function tone({ depth, inner }: LatticeEdge): { stroke: string; strokeOpacity?: number } {
  if (inner) return { stroke: 'var(--color-border)' }
  return { stroke: 'var(--color-edge)', strokeOpacity: Math.round((1 - 0.5 * depth) * 100) / 100 }
}

/** The grey lines, built once and shared by every Lattice on the page: a still one costs one element tree. */
const LINES = GEAR.edges.map((edge) => (
  <path key={edge.d} d={edge.d} vectorEffect="non-scaling-stroke" {...tone(edge)} />
))

/**
 * The drawn motif (A19): a gear's wireframe that draws itself once, line by line, with a short accent head at each
 * line's tip, then stays grey and still. Decorative, so hidden from assistive technology.
 *
 * `still` renders the final frame at once and never animates: for standing in where there is no picture, many at a
 * time. Set part-way through a drawing (setup's Skip), it ends the drawing at its final frame, with no accent left.
 * Under reduced motion every Lattice is still, and the accent is never rendered.
 *
 * The default export is for `lazy()` on sign-in, so anime.js arrives with that chunk.
 */
export default function Lattice({
  className,
  still = false,
  total = 2400,
}: {
  className?: string
  still?: boolean
  /** First line to last settled, in ms: 2.4 s on sign-in, `--duration-intro` in setup's intro (A20). */
  total?: number
}) {
  const lines = useRef<SVGGElement>(null)
  const heads = useRef<SVGGElement>(null)
  const [drawing, setDrawing] = useState(() => !still && !reduced())

  useEffect(() => {
    if (still || lines.current === null) return
    const paths = (group: SVGGElement | null) =>
      group === null ? [] : Array.from(group.querySelectorAll<SVGGeometryElement>('path'))
    // Long enough to read as drawing (DESIGN.md § Lattice). Turning `still` on part-way reverts to the drawn frame.
    return drawIn(paths(lines.current), () => setDrawing(false), total, paths(heads.current))
  }, [still, total])

  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={GEAR.viewBox}
      fill="none"
      strokeWidth={1}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className === undefined ? 'transition-none' : `transition-none ${className}`}
    >
      {/*
        Fresh nodes once still: reverting a drawing part-way (or after it ended) left anime's dash state on the
        strokes and the lines blank, so a Lattice told to stop draws its final frame on new paths instead.
      */}
      <g ref={lines} key={still ? 'still' : 'drawing'}>
        {LINES}
      </g>
      {drawing && !still ? (
        <g ref={heads} stroke="var(--color-accent)" data-draw-head="">
          {GEAR.edges.map(({ d }) => (
            <path key={d} d={d} vectorEffect="non-scaling-stroke" className="transition-none" />
          ))}
        </g>
      ) : null}
    </svg>
  )
}
