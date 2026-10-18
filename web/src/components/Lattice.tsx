import { useEffect, useRef, useState } from 'react'
import { STEP, frame, lattice, rough, type LatticeEdge, type LatticeFace } from '../lib/lattice'
import { cycle, drawIn, reduced, type Cycle } from '../lib/motion'
import { strings } from '../lib/strings'
import { Icon } from './Icon'

/** One step of the brilliant's eightfold symmetry per cycle, turned and then held, so the loop has no seam (A27). */
const TURN_MS = 3200
/** The share of each cycle spent turning; the rest it holds, as a stone is turned and looked at. */
const TURNING = 0.55
/** Cycles the cut takes, from the rough stone to the brilliant, before the turning settles into its rhythm. */
const CUT_CYCLES = 2

/** Computed once: the geometry never changes, and two renders must draw the same stone. */
const STONE = lattice()
const ROUGH = rough()
const FACETS = frame().faces

/** Settled strokes (DESIGN.md § Lattice): Edge, with the lines of facets turned away faint, so the stone reads solid. */
const opacity = ({ front }: LatticeEdge) => String(Math.round((0.2 + 0.75 * front) * 100) / 100)

/** A facet is blue only while it glints or is being cut: Layout Blue on what moves, never at rest. */
const shine = ({ glint, cutting }: LatticeFace) => String(Math.round(Math.max(0.5 * glint, 0.12 * cutting) * 100) / 100)

/** The grey lines, built once and shared by every Lattice on the page: a still one costs one element tree. */
const lines = (edges: readonly LatticeEdge[]) =>
  edges.map((edge) => (
    <path
      key={edge.d}
      d={edge.d}
      vectorEffect="non-scaling-stroke"
      stroke="var(--color-edge)"
      strokeOpacity={opacity(edge)}
    />
  ))
const LINES = lines(STONE.edges)
const ROUGH_LINES = lines(ROUGH.edges)

/**
 * The drawn motif (A19, A27): a brilliant's wireframe that draws itself once, line by line, with a short accent head
 * at each line's tip, then stays grey and still. Decorative, so hidden from assistive technology.
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
   * Sign-in only (A26, A27): it draws the rough stone, cuts it into the brilliant facet by facet, then keeps turning it
   * a step at a time while glints run across its facets, with a Pause control because it loops. Nowhere else does the
   * lattice move after drawing.
   */
  live?: boolean
}) {
  const group = useRef<SVGGElement>(null)
  const heads = useRef<SVGGElement>(null)
  const faces = useRef<SVGGElement>(null)
  const [drawing, setDrawing] = useState(() => !still && !reduced())
  const moving = live && !still && !reduced()
  const [paused, setPaused] = useState(false)
  const turning = useRef<Cycle | null>(null)
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  // After the drawing, the cut and then the turn: every line and facet re-projected each frame, written to the DOM
  // directly so React never re-renders two hundred paths sixty times a second.
  useEffect(() => {
    if (!moving || drawing || group.current === null) return
    const paths = Array.from(group.current.querySelectorAll<SVGPathElement>('path'))
    const facets = Array.from(faces.current?.querySelectorAll<SVGPathElement>('path') ?? [])
    let cycles = 0
    let last = 0
    const turn = cycle(TURN_MS, (progress) => {
      if (progress < last) cycles++
      last = progress
      const t = Math.min(1, progress / TURNING)
      // Each cycle turns one step and holds: a step looks the same as none, so cycles join without a seam, and the
      // lights sweep and come back within it, so the glints run across the facets even while it holds.
      const now = frame({
        turn: STEP * t * t * (3 - 2 * t),
        cut: Math.min(1, (cycles + progress) / CUT_CYCLES),
        light: 0.6 * Math.sin(2 * Math.PI * progress),
      })
      now.edges.forEach((edge, i) => {
        paths[i]?.setAttribute('d', edge.d)
        paths[i]?.setAttribute('stroke-opacity', opacity(edge))
      })
      now.faces.forEach((face, i) => {
        facets[i]?.setAttribute('d', face.d)
        facets[i]?.setAttribute('fill-opacity', shine(face))
      })
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
    if (still || group.current === null) return
    const paths = (group: SVGGElement | null) =>
      group === null ? [] : Array.from(group.querySelectorAll<SVGGeometryElement>('path'))
    // Long enough to read as drawing (DESIGN.md § Lattice). Turning `still` on part-way reverts to the drawn frame.
    return drawIn(paths(group.current), () => setDrawing(false), total, paths(heads.current))
  }, [still, total])

  const drawingSvg = (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={STONE.viewBox}
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
      {moving && !drawing ? (
        <g ref={faces} fill="var(--color-accent)" stroke="none" data-facets="">
          {FACETS.map((face) => (
            <path key={face.d} d={face.d} fillOpacity={0} className="transition-none" />
          ))}
        </g>
      ) : null}
      <g ref={group} key={still ? 'still' : moving && !drawing ? 'turning' : 'drawing'}>
        {moving ? ROUGH_LINES : LINES}
      </g>
      {drawing && !still ? (
        <g ref={heads} stroke="var(--color-accent)" data-draw-head="">
          {(moving ? ROUGH : STONE).edges.map(({ d }) => (
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
