import { useEffect, useRef, useState } from 'react'
import { PITCH, lattice, latticeAt, type LatticeEdge } from '../lib/lattice'
import { cycle, drawIn, reduced, type Cycle } from '../lib/motion'
import { strings } from '../lib/strings'
import { Icon } from './Icon'

/** One tooth pitch per cycle: the turn after which the gear looks the same, so the loop has no seam (A26). */
const TURN_MS = 3000
/** The marks travelling round the tooth profile and the bore: a short dash of the ring's length. */
const MARK = 0.07

/** Computed once: the geometry never changes, and two renders must draw the same part. */
const GEAR = lattice()
const GEAR_RINGS = latticeAt(0)

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
  live = false,
}: {
  className?: string
  still?: boolean
  /** First line to last settled, in ms: 2.4 s on sign-in, `--duration-intro` in setup's intro (A20). */
  total?: number
  /**
   * Sign-in only (A26): once drawn, the gear keeps turning and three marks travel round it, with a Pause control,
   * because it loops. Nowhere else does the lattice move after drawing.
   */
  live?: boolean
}) {
  const lines = useRef<SVGGElement>(null)
  const heads = useRef<SVGGElement>(null)
  const marks = useRef<SVGGElement>(null)
  const [drawing, setDrawing] = useState(() => !still && !reduced())
  const moving = live && !still && !reduced()
  const [paused, setPaused] = useState(false)
  const turning = useRef<Cycle | null>(null)
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  // After the drawing, the turn: every line re-projected each frame, written to the DOM directly so React never
  // re-renders 350 paths sixty times a second.
  useEffect(() => {
    if (!moving || drawing || lines.current === null) return
    const paths = Array.from(lines.current.querySelectorAll<SVGPathElement>('path'))
    const [front, hub, bore] = Array.from(marks.current?.querySelectorAll<SVGPathElement>('path') ?? [])
    const turn = cycle(TURN_MS, (progress) => {
      const frame = latticeAt(progress * PITCH)
      frame.edges.forEach((edge, i) => {
        const path = paths[i]
        if (path === undefined) return
        path.setAttribute('d', edge.d)
        const { strokeOpacity } = tone(edge)
        if (strokeOpacity !== undefined) path.setAttribute('stroke-opacity', String(strokeOpacity))
      })
      // Whole laps per tooth turned, so these loop seamlessly too: once round the teeth, twice round the hub's rim
      // the other way, and once back round the bore.
      front?.setAttribute('d', frame.front)
      front?.setAttribute('stroke-dashoffset', String(-progress))
      hub?.setAttribute('d', frame.hub)
      hub?.setAttribute('stroke-dashoffset', String(2 * progress))
      bore?.setAttribute('d', frame.bore)
      bore?.setAttribute('stroke-dashoffset', String(progress))
    })
    turning.current = turn
    // Nothing turns while the page is hidden; the reader's own Pause holds across that.
    const hidden = () => (document.hidden ? turn.pause() : pausedRef.current || turn.play())
    document.addEventListener('visibilitychange', hidden)
    return () => {
      document.removeEventListener('visibilitychange', hidden)
      turn.cancel()
      turning.current = null
    }
  }, [moving, drawing])

  useEffect(() => {
    if (still || lines.current === null) return
    const paths = (group: SVGGElement | null) =>
      group === null ? [] : Array.from(group.querySelectorAll<SVGGeometryElement>('path'))
    // Long enough to read as drawing (DESIGN.md § Lattice). Turning `still` on part-way reverts to the drawn frame.
    return drawIn(paths(lines.current), () => setDrawing(false), total, paths(heads.current))
  }, [still, total])

  const drawingSvg = (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={GEAR.viewBox}
      fill="none"
      strokeWidth={1}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={moving ? 'w-full transition-none' : className === undefined ? 'transition-none' : `transition-none ${className}`}
    >
      {/*
        Fresh nodes once still: reverting a drawing part-way (or after it ended) left anime's dash state on the
        strokes and the lines blank, so a Lattice told to stop draws its final frame on new paths instead; a turning one
        (A26) starts on new paths too, since a dash fitted to a line's drawn length gaps once the line changes length.
      */}
      <g ref={lines} key={still ? 'still' : moving && !drawing ? 'turning' : 'drawing'}>
        {LINES}
      </g>
      {moving && !drawing ? (
        <g ref={marks} stroke="var(--color-accent)" strokeWidth={1.5} data-marks="">
          {[GEAR_RINGS.front, GEAR_RINGS.hub, GEAR_RINGS.bore].map((d) => (
            <path
              key={d}
              d={d}
              pathLength={1}
              strokeDasharray={`${MARK} ${1 - MARK}`}
              vectorEffect="non-scaling-stroke"
              className="transition-none"
            />
          ))}
        </g>
      ) : null}
      {drawing && !still ? (
        <g ref={heads} stroke="var(--color-accent)" data-draw-head="">
          {GEAR.edges.map(({ d }) => (
            <path key={d} d={d} vectorEffect="non-scaling-stroke" className="transition-none" />
          ))}
        </g>
      ) : null}
    </svg>
  )
  if (!moving) return drawingSvg
  return (
    // Positioned only at `lg`: below it the drawing is larger than the Doorway's band and cropped, so the Pause takes
    // its place from the band's frame instead, where it can always be seen.
    <div className={className === undefined ? 'lg:relative' : `lg:relative ${className}`}>
      {drawingSvg}
      {/* It loops, so it can be stopped (WCAG 2.2.2): quiet, and only once there is something moving to stop. */}
      {drawing ? null : (
        <button
          type="button"
          aria-pressed={paused}
          onClick={() => {
            const next = !paused
            setPaused(next)
            if (next) turning.current?.pause()
            else turning.current?.play()
          }}
          className="ease-mechanical absolute right-[4.5rem] bottom-3 z-10 flex bg-[var(--color-bg)] lg:right-0 lg:bottom-0 items-center gap-1.5 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 text-xs text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
        >
          <Icon name={paused ? 'play' : 'pause'} />
          {paused ? strings.doorway.play : strings.doorway.pause}
        </button>
      )}
    </div>
  )
}
