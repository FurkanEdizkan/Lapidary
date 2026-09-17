import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Group,
  AlwaysStencilFunc,
  BackSide,
  Box3,
  BufferGeometry,
  Color,
  DecrementWrapStencilOp,
  DoubleSide,
  Float32BufferAttribute,
  FrontSide,
  IncrementWrapStencilOp,
  Line,
  LineBasicMaterial,
  Mesh,
  MeshBasicMaterial,
  MeshStandardMaterial,
  NotEqualStencilFunc,
  OrthographicCamera,
  Plane,
  PlaneGeometry,
  Points,
  PointsMaterial,
  Raycaster,
  ReplaceStencilOp,
  Scene,
  Vector2,
  Vector3,
  WebGLRenderer,
  type Intersection,
  type Object3D,
  type Material,
} from 'three'
import { CSS2DObject, CSS2DRenderer } from 'three/examples/jsm/renderers/CSS2DRenderer.js'
import { annotationsOf, labelsFor, type Label } from '../lib/annotations'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { blobUrl, fetchBatchStatus, fetchPmi,
  fetchEntities, fetchStructure, requestRung } from '../lib/api'
import { PICKS, measure, nearestCorner, placeEntities, withoutParts, type Pick, type Tool } from '../lib/measure'
import { strings } from '../lib/strings'
import type { BatchId, BlobHash, PartDetail } from '../lib/types'
import {
  capPlacement,
  frameBox,
  kept,
  sectionPlane,
  visibleRanges,
  type PlaneLike,
  type Section,
  type Vec3, explodeOffsets, partCentres } from '../lib/viewer-math'
import { MeasureBar, SectionBar, ExplodeBar } from './Measure'
import { disposeModel, partMaterial, studioLights } from './studio'
import { tween } from '../lib/motion'

type View = {
  show: (model: Object3D) => void
  /** What a click at a point on the screen met, or `null` where it met no part. */
  pick: (x: number, y: number) => Pick | null
  /** Where a ray from a pick, straight into the part, leaves it again: the far side of a wall. */
  through: (from: Pick) => Pick | null
  mark: (points: readonly Vec3[]) => void
  /** A finished reading's line through `points` and its `text` beside them, drawn in; `null` for none. */
  callout: (points: readonly Vec3[] | null, text: string | null) => void
  /** Leave these parts out, by their depth-first place in the tree; kept for every rung shown after. */
  hide: (hidden: ReadonlySet<number>) => void
  /** Cut the part along a plane across its box, or stop cutting; kept for every rung shown after. */
  section: (section: Section | null) => void
  /** Whether the part is known to be a closed mesh, which is when a section's cut face is filled. */
  closed: (closed: boolean) => void
  /** Draw an earlier revision's rung as a ghost over the part, or stop; never picked. */
  ghost: (model: Object3D | null) => void
  /** Draw these labels over the part, each beside its face, or none. */
  annotate: (labels: readonly Label[] | null) => void
  /** Draw an assembly's parts apart by `amount`, from 0 as assembled to 1; kept for every rung shown after. */
  explode: (amount: number) => void
  dispose: () => void
}

const NONE: ReadonlySet<number> = new Set()

/** A reading's label: the figure in the mono face on the card ground, edged in the marks' accent. */
const CALLOUT =
  'tabular whitespace-pre rounded-sm border border-[var(--color-accent)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[11px] leading-tight text-[var(--color-bright)]'

/** A PMI label beside its face: small, on the panel colour, one annotation to a line. */
const LABEL =
  'whitespace-pre rounded-sm border border-[var(--color-edge)] bg-[var(--color-surface)] px-1 text-[10px] leading-tight text-[var(--color-bright)]'

/** The mesh a rung counts its parts on (`extras.parts`), or `null` for a rung that counts none. */
function partsMesh(model: Object3D): Mesh | null {
  let found: Mesh | null = null
  model.traverse((object) => {
    if (found === null && object instanceof Mesh && Array.isArray(object.userData.parts)) found = object
  })
  return found
}

/** How many placed parts a rung counts triangles for (`extras.parts`), or `null` when it counts none. */
function partsOf(model: Object3D): number | null {
  let count: number | null = null
  model.traverse((object) => {
    if (object instanceof Mesh && Array.isArray(object.userData.parts)) count = object.userData.parts.length
  })
  return count
}

/** A press that moved less than this, in CSS pixels, is a click and picks; further, it turned the part. */
const CLICK_SLOP_PX = 4

/** The accent, `--color-accent`, for the marks a pick leaves. */
const MARK = 0x2cb4f5

/** What a view draws with. Everything here holds GPU state; the lights do not, so they are not here. */
type Kit = {
  renderer: WebGLRenderer
  material: MeshStandardMaterial
  markMaterial: PointsMaterial
  /** A reading's callout: an accent line between its picks, drawn over the part like the marks. */
  calloutMaterial: LineBasicMaterial
  ghostMaterial: MeshBasicMaterial
  capMaterial: MeshBasicMaterial
  capBack: MeshBasicMaterial
  capFront: MeshBasicMaterial
}

function kit(): Kit {
  // A stencil buffer, which three 0.186 no longer gives by default: a section's cap is drawn where it says
  // the cut crosses material.
  const renderer = new WebGLRenderer({ antialias: true, alpha: true, stencil: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  // On for every view, so a section needs nothing of its own. With no plane on a material it changes
  // no program, so `prepare` still compiles what a view draws.
  renderer.localClippingEnabled = true
  return {
    renderer,
    material: partMaterial(),
    markMaterial: new PointsMaterial({ color: MARK, size: 7, sizeAttenuation: false, depthTest: false }),
    calloutMaterial: new LineBasicMaterial({ color: MARK, depthTest: false }),
    // Drawn through the part rather than hidden behind it: a smaller earlier revision sits inside
    // the current one, and a ghost only visible where it sticks out would read as no change there.
    // Amber, `--color-warn`, not grey: a grey ghost over a grey part on a near-black ground showed
    // almost nothing in the browser check, and the accent is the marks' colour.
    ghostMaterial: new MeshBasicMaterial({
      color: new Color(0xe8b06a),
      transparent: true,
      opacity: 0.4,
      depthTest: false,
      depthWrite: false,
    }),
    // A section's filled face, flat and unlit, so it reads as the inside of the part and never as a lit
    // surface of it: brick, distinct from the part's grey and the ghost's amber. Drawn only where the
    // stencil passes below left the count non-zero, and it sets the count back as it draws.
    capMaterial: new MeshBasicMaterial({
      color: new Color(0xc4665a),
      side: DoubleSide,
      stencilWrite: true,
      stencilRef: 0,
      stencilFunc: NotEqualStencilFunc,
      stencilFail: ReplaceStencilOp,
      stencilZFail: ReplaceStencilOp,
      stencilZPass: ReplaceStencilOp,
    }),
    // The two stencil passes, three's clipping-stencil technique: clipped by the section's plane, a
    // closed mesh's back faces count up and its front faces count down, so the count is non-zero exactly
    // where a ray from the eye enters material behind the cut. Neither writes colour or depth.
    capBack: stencilPass(BackSide, IncrementWrapStencilOp),
    capFront: stencilPass(FrontSide, DecrementWrapStencilOp),
  }
}

function stencilPass(side: typeof BackSide | typeof FrontSide, op: typeof IncrementWrapStencilOp | typeof DecrementWrapStencilOp) {
  return new MeshBasicMaterial({
    side,
    colorWrite: false,
    depthWrite: false,
    depthTest: false,
    stencilWrite: true,
    stencilFunc: AlwaysStencilFunc,
    stencilFail: op,
    stencilZFail: op,
    stencilZPass: op,
  })
}

/**
 * No marks, as an empty position buffer rather than no buffer at all. three compiles a different
 * program for points with no position attribute, and a view's marks gain one as soon as anything
 * is marked, so both states start from this one.
 */
function noMarks(): BufferGeometry {
  return new BufferGeometry().setAttribute('position', new Float32BufferAttribute([], 3))
}

/**
 * One kit for the session. A renderer made per open compiled the part's shaders again on every
 * open. Its materials are never disposed either, because disposing a material frees the program
 * compiled for it. A second view open at the same time, which nothing does today, gets its own.
 */
let session: Kit | null = null
let sessionInUse = false
let prepared: Promise<void> | null = null

/**
 * Compile the view's shaders before any part is opened; the grid calls this on hover. three keys a
 * program on the material, the lights and the geometry's attributes, so this scene matches a real
 * view in all three: the session's own materials, the same two lights, a position-only triangle
 * like a rung's, and the view's empty marks. Anything else warms a program no view uses, which a
 * timing run shows as a shader linked during the first open (`web/scripts/open-timing.mjs`).
 */
export function prepare(): Promise<void> {
  prepared ??= (async () => {
    session ??= kit()
    const triangle = new BufferGeometry().setAttribute(
      'position',
      new Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0], 3),
    )
    const scene = new Scene()
    scene.add(
      ...studioLights(),
      new Mesh(triangle, session.material),
      new Points(noMarks(), session.markMaterial),
      // The callout's line, so the first reading does not link a shader as it draws in.
      new Line(triangle, session.calloutMaterial),
    )
    await session.renderer.compileAsync(scene, new OrthographicCamera())
    triangle.dispose()
  })()
  return prepared
}

/**
 * The part in 3D. L0 first, because ingest built it and it paints at once; then L1, asked for
 * through `POST /api/parts/{id}/rungs/l1` the first time anyone opens the part, and swapped in
 * without moving the camera when it lands.
 *
 * Drawn on demand, never in a loop: a frame when the camera moves, the box resizes or a rung
 * arrives, and nothing otherwise. Nothing spins or eases on its own, so there is no motion for
 * `prefers-reduced-motion` to take away.
 *
 * Drawn as the thumbnail is (`raster.rs`): orthographic, so nothing tapers toward the lens, and
 * shaded flat, one normal per face. The GLB carries no normals on purpose (`glb.rs`), and
 * averaging them across a sharp rim is what blurred the first cylinder's cap into its side.
 *
 * Measuring draws L2, the bridge's own mesh, and picks wait for it: a pick snaps to the entity its
 * triangle's corners lie on (`measure.ts`), and only L2's corners lie on the B-rep. Once anyone
 * picks a tool, L2 stays, so the marks never sit on a coarser surface than the one they were put on.
 *
 * A section cuts the part along a plane across its box. It changes what is drawn and what a pick
 * can meet, and nothing else: a reading comes from the geometry its picks landed on, which a cut
 * never moves. The cut is open, so it shows the part's inside surfaces rather than a filled face.
 *
 * An assembly's parts can be hidden. `hidden` names them by their depth-first place in the tree,
 * and `onParts` says how many placed parts the rung drawn counts, so the tree offers to hide parts
 * only when the view can.
 *
 * The default export, for `lazy()`: three.js is in this chunk and nowhere else.
 */
export default function Viewer({
  part,
  poster,
  hidden = NONE,
  onParts,
  ghost = null,
  annotated = false,
  stage = false,
}: {
  part: PartDetail
  poster: ReactNode
  hidden?: ReadonlySet<number>
  onParts?: (parts: number | null) => void
  /** An earlier revision's rung, drawn as a grey ghost over the part, or `null` for none. */
  ghost?: BlobHash | null
  /** Whether the file's PMI is drawn beside the faces it names. */
  annotated?: boolean
  /** Fill the parent on the lamp's ground, as the quick look's stage does, rather than a 22rem square. */
  stage?: boolean
}) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<View | null>(null)
  const pressed = useRef<{ x: number; y: number } | null>(null)
  const [painted, setPainted] = useState(false)
  const [failed, setFailed] = useState(false)
  const [shown, setShown] = useState<BlobHash | null>(null)
  const [batch, setBatch] = useState<BatchId | null>(null)
  const [tool, setTool] = useState<Tool | null>(null)
  const [picks, setPicks] = useState<Pick[]>([])
  const [noWall, setNoWall] = useState(false)
  const [fine, setFine] = useState(false)
  const [fineFailed, setFineFailed] = useState(false)
  const [parts, setParts] = useState<number | null>(null)
  const [section, setSection] = useState<Section | null>(null)
  const [explosion, setExplosion] = useState(0)
  const [ghostFailed, setGhostFailed] = useState<BlobHash | null>(null)
  const queryClient = useQueryClient()
  const hash = (fine ? part.tessellationL2 : null) ?? part.tessellationL1 ?? part.tessellationL0

  useEffect(() => {
    const node = host.current
    if (node === null || hash === null) return
    if (view.current === null) view.current = createView(node, () => setPainted(true))
    const current = view.current
    let stale = false
    // Rungs are written with `EXT_meshopt_compression` (`glb.rs`); the decoder is in this chunk only.
    new GLTFLoader()
      .setMeshoptDecoder(MeshoptDecoder)
      .loadAsync(blobUrl(hash))
      .then((gltf) => {
        if (stale) return
        current.show(gltf.scene)
        setShown(hash)
        setParts(partsOf(gltf.scene))
      })
      .catch(() => {
        if (!stale) setFailed(true)
      })
    return () => {
      stale = true
    }
  }, [hash])

  // After the rung's effect, which creates the view: a ghost has nothing to be drawn in before it.
  useEffect(() => {
    const current = view.current
    if (current === null) return
    if (ghost === null) {
      current.ghost(null)
      return
    }
    let stale = false
    new GLTFLoader()
      .setMeshoptDecoder(MeshoptDecoder)
      .loadAsync(blobUrl(ghost))
      .then((gltf) => {
        if (!stale) current.ghost(gltf.scene)
      })
      .catch(() => {
        if (stale) return
        current.ghost(null)
        setGhostFailed(ghost)
      })
    return () => {
      stale = true
    }
  }, [ghost])

  useEffect(() => {
    view.current?.hide(hidden)
  }, [hidden])
  useEffect(() => {
    view.current?.section(section)
  }, [section])
  // After the rung is shown, since the parts are the rung's.
  useEffect(() => {
    view.current?.explode(explosion)
  }, [explosion, shown])
  useEffect(() => {
    view.current?.closed(part.isWatertight === true)
  }, [part.isWatertight])
  useEffect(() => {
    onParts?.(parts)
  }, [parts, onParts])

  useEffect(
    () => () => {
      view.current?.dispose()
      view.current = null
    },
    [],
  )

  // Ask once per part for the rung it lacks: L1 to look at, and L2 once someone measures. A rung
  // that exists but the detail did not know about yet is a stale detail, so it is read again; a
  // queued one is watched until its batch finishes.
  const level = fine ? 'l2' : 'l1'
  const wanted = fine ? part.tessellationL2 : part.tessellationL1
  useEffect(() => {
    if (part.tessellationL0 === null || wanted !== null) return
    let stale = false
    requestRung(part.id, level)
      .then((answer) => {
        if (stale) return
        if (answer.kind === 'ready') {
          void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
        } else if (answer.queued.queued > 0) {
          setBatch(answer.queued.batchId)
        }
      })
      // Without L1 the view is still a view, on L0. Without L2 there is nothing to measure on,
      // which the measuring line says.
      .catch(() => {
        if (!stale && level === 'l2') setFineFailed(true)
      })
    return () => {
      stale = true
    }
  }, [part.id, part.tessellationL0, wanted, level, queryClient])

  const refining = useQuery({
    queryKey: ['batch', part.library, batch],
    queryFn: () => fetchBatchStatus(part.library, batch as BatchId),
    enabled: batch !== null,
    refetchInterval: (query) => (query.state.data?.finishedAt == null ? 1000 : false),
  })
  const finished = refining.data?.finishedAt != null
  useEffect(() => {
    if (finished) void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
  }, [finished, part.id, queryClient])

  const entities = useQuery({
    queryKey: ['entities', part.entities],
    queryFn: () => fetchEntities(part.entities as BlobHash),
    enabled: (fine || annotated) && part.entities !== null,
  })
  const structure = useQuery({
    queryKey: ['structure', part.structure],
    queryFn: () => fetchStructure(part.structure as BlobHash),
    enabled: (fine || annotated) && part.structure !== null,
  })
  const pmi = useQuery({
    queryKey: ['pmi', part.pmi],
    queryFn: () => fetchPmi(part.pmi as BlobHash),
    enabled: annotated && part.pmi !== null,
  })
  // Each entity where the mesh drew it, or `null` while either half is on its way. A part whose
  // entities or tree could not be read is measured as a mesh is, every value approximate, rather
  // than snapped to entities left where they were never drawn.
  const placed = useMemo(() => {
    if (part.entities === null || entities.isError || structure.isError) return []
    if (entities.data === undefined || (part.structure !== null && structure.data === undefined)) return null
    return placeEntities(entities.data, structure.data ?? null)
  }, [part.entities, part.structure, entities.data, entities.isError, structure.data, structure.isError])

  // Labels only on the parts still drawn: placed again without the hidden ones, whose faces are nowhere.
  const labels = useMemo(() => {
    if (!annotated || pmi.data === undefined || placed === null) return null
    const visible =
      hidden.size === 0 || structure.data === undefined || entities.data === undefined
        ? placed
        : placeEntities(entities.data, withoutParts(structure.data, hidden))
    return labelsFor(annotationsOf(pmi.data), visible).labels
  }, [annotated, pmi.data, placed, hidden, structure.data, entities.data])
  // After the rung is shown, so labels asked for before the view existed are drawn once it does.
  useEffect(() => {
    view.current?.annotate(labels)
  }, [labels, shown])

  const ready = part.tessellationL2 !== null && shown === part.tessellationL2 && placed !== null
  const unavailable =
    fineFailed ||
    (fine && finished && part.tessellationL2 === null && (refining.data?.failedTotal ?? 0) > 0)
  const reading = useMemo(
    () => (tool === null || placed === null ? null : measure(tool, picks, placed)),
    [tool, picks, placed],
  )
  const note =
    tool === null
      ? null
      : unavailable
        ? strings.measure.unavailable
        : !ready
          ? strings.measure.loading
          : noWall
            ? strings.measure.noWall
            : strings.measure.prompts[tool]

  useEffect(() => {
    view.current?.mark(picks.map((pick) => pick.point))
  }, [picks])
  useEffect(() => {
    if (tool === null || reading === null) {
      view.current?.callout(null, null)
      return
    }
    const value = (tool === 'angle' ? strings.measure.degrees : strings.measure.millimetres)(reading.value)
    view.current?.callout(
      picks.map((pick) => pick.point),
      reading.approximate ? `${value} ${strings.detail.approximate}` : value,
    )
  }, [tool, reading, picks])

  const choose = (next: Tool | null) => {
    setTool(next)
    setPicks([])
    setNoWall(false)
    if (next !== null) setFine(true)
  }

  const pickAt = (x: number, y: number) => {
    const current = view.current
    if (tool === null || !ready || current === null) return
    const hit = current.pick(x, y)
    if (hit === null) return
    if (tool === 'wall') {
      const far = current.through(hit)
      setNoWall(far === null)
      setPicks(far === null ? [hit] : [hit, far])
      return
    }
    const pick = tool === 'edge' ? { ...hit, point: nearestCorner(hit.point, hit.corners) } : hit
    // A finished measurement, or three points no circle passes through, starts over.
    setPicks(reading !== null || picks.length >= PICKS[tool] ? [pick] : [...picks, pick])
  }

  return (
    <div className={stage ? 'flex h-full min-h-0 w-full flex-col' : 'w-full max-w-[22rem]'}>
      <div
        className={
          stage
            ? 'stage-lamp relative min-h-[16rem] w-full flex-1 overflow-hidden rounded-md'
            : 'relative aspect-square w-full overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]'
        }
      >
        <div
          ref={host}
          role="img"
          aria-label={strings.viewer.label(part.name)}
          hidden={failed}
          className={tool !== null && ready ? 'absolute inset-0 cursor-crosshair' : 'absolute inset-0'}
          onPointerDown={(event) => {
            pressed.current = { x: event.clientX, y: event.clientY }
          }}
          onPointerUp={(event) => {
            const from = pressed.current
            pressed.current = null
            if (from !== null && Math.hypot(event.clientX - from.x, event.clientY - from.y) < CLICK_SLOP_PX) {
              pickAt(event.clientX, event.clientY)
            }
          }}
        />
        {painted && !failed ? null : <div className="absolute inset-0">{poster}</div>}
        {failed ? (
          <p className="absolute inset-x-2 bottom-2 text-xs text-[var(--color-muted)]">
            {strings.viewer.failed}
          </p>
        ) : batch !== null && !finished ? (
          <p aria-live="polite" className="absolute bottom-2 left-2 text-xs text-[var(--color-muted)]">
            {strings.viewer.refining}
          </p>
        ) : ghost !== null && ghostFailed === ghost ? (
          <p role="alert" className="absolute inset-x-2 bottom-2 text-xs text-[var(--color-muted)]">
            {strings.viewer.ghostFailed}
          </p>
        ) : null}
      </div>
      {failed ? null : (
        /*
          On a stage the tools dock in one row on the card ground under it, never over the lamp: the edge
          grey that marks a control does not reach 3:1 on the lamp (`styles.css`), and it does on `surface`.
        */
        <div
          className={
            stage
              ? 'mt-2 flex flex-none flex-wrap items-start gap-x-6 gap-y-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 [&>*]:mt-0'
              : undefined
          }
        >
          <MeasureBar
            tool={tool}
            onTool={choose}
            reading={reading}
            note={note}
            off={explosion > 0 ? strings.explode.measuringOff : null}
          />
          <SectionBar section={section} onSection={setSection} closed={part.isWatertight} />
          {parts !== null && parts > 1 ? (
            <ExplodeBar
              amount={explosion}
              onAmount={(next) => {
                setExplosion(next)
                if (next > 0) choose(null)
              }}
            />
          ) : null}
        </div>
      )}
    </div>
  )
}

function createView(node: HTMLElement, onFirstFrame: () => void): View {
  const own = sessionInUse
  const { renderer, material, markMaterial, calloutMaterial, ghostMaterial, capMaterial, capBack, capFront } = own
    ? kit()
    : (session ??= kit())
  sessionInUse = true
  renderer.setSize(node.clientWidth, node.clientHeight, false)
  renderer.domElement.style.width = '100%'
  renderer.domElement.style.height = '100%'
  node.appendChild(renderer.domElement)

  const scene = new Scene()
  scene.add(...studioLights())
  const camera = new OrthographicCamera(-1, 1, 1, -1, 0.1, 10)
  camera.up.set(0, 0, 1)
  let halfHeight = 1
  // The framing holds the part across the view's shorter side. In a wide box that is the height,
  // so a resize changes only how much room is beside the part; in a tall one (a stage on a phone)
  // it is the width, which a height-only framing let the part run out of at both edges.
  const fit = () => {
    const aspect = node.clientWidth / Math.max(node.clientHeight, 1)
    const half = aspect < 1 ? halfHeight / aspect : halfHeight
    camera.left = -half * aspect
    camera.right = half * aspect
    camera.top = half
    camera.bottom = -half
    camera.updateProjectionMatrix()
  }
  fit()
  const controls = new OrbitControls(camera, renderer.domElement)
  // PMI labels: DOM elements laid over the canvas and moved with the camera every frame, so their text is
  // the page's own, crisp and selectable by nothing that picks.
  const labelRenderer = new CSS2DRenderer()
  labelRenderer.setSize(node.clientWidth, node.clientHeight)
  Object.assign(labelRenderer.domElement.style, { position: 'absolute', inset: '0', pointerEvents: 'none' })
  node.appendChild(labelRenderer.domElement)
  const labels = new Group()

  const raycaster = new Raycaster()
  const markers = new Points(noMarks(), markMaterial)
  // Drawn over the part and its ghost, so a mark on a face turned away still shows where it was put.
  markers.renderOrder = 5
  scene.add(markers)
  scene.add(labels)
  // A finished reading, drawn where it was taken: a line through its picks and its value beside them.
  // The label is `aria-hidden`: the reading line under the view is the accessible one, and this is the
  // same figure put where the eye already is.
  const calloutLine = new Line(new BufferGeometry(), calloutMaterial)
  calloutLine.renderOrder = 6
  calloutLine.visible = false
  scene.add(calloutLine)
  const calloutElement = document.createElement('div')
  calloutElement.className = CALLOUT
  calloutElement.setAttribute('aria-hidden', 'true')
  // Written every frame of the draw-in, so the stylesheet's opacity transition must not smooth it.
  calloutElement.style.transition = 'none'
  const calloutLabel = new CSS2DObject(calloutElement)
  calloutLabel.visible = false
  scene.add(calloutLabel)
  let calloutCancel = () => {}
  // A section's cap: a square on the cut, placed by `capPlacement` and drawn after the stencil passes and
  // before the part. Not inside `model`, so no pick or wall ray ever meets it.
  const cap = new Mesh(new PlaneGeometry(1, 1), capMaterial)
  cap.renderOrder = 2
  cap.visible = false
  scene.add(cap)

  let first = true
  let model: Object3D | null = null
  // An earlier revision, drawn over the part and never picked: `pick` and `through` cast at `model`.
  let ghostModel: Object3D | null = null
  let hiddenParts = NONE
  // An assembly drawn apart: a mesh per placed part, sharing the rung's buffers and drawing only its own run of
  // the index, each moved out from the box's centre. Built on the rung's first explode, and dropped with it.
  let exploded: Group | null = null
  let explosion = 0
  // The part's box, from the first rung, which a section cuts across; the cut, and its plane.
  let bounds: { min: Vec3; max: Vec3 } | null = null
  let cut: Section | null = null
  let cutPlane: PlaneLike | null = null
  let closedMesh = false
  const clip = new Plane()
  const applyCut = () => {
    const wasCut = (material.clippingPlanes?.length ?? 0) > 0
    cutPlane = cut === null || bounds === null ? null : sectionPlane(cut.axis, cut.at, cut.flip, bounds.min, bounds.max)
    if (cutPlane !== null) {
      clip.normal.set(...cutPlane.normal)
      clip.constant = cutPlane.constant
    }
    material.clippingPlanes = cutPlane === null ? null : [clip]
    // The ghost is cut by the same plane, so a section hides the same half of both revisions.
    ghostMaterial.clippingPlanes = material.clippingPlanes
    // three compiles a program per count of planes; moving the one plane is only a uniform.
    capBack.clippingPlanes = material.clippingPlanes
    capFront.clippingPlanes = material.clippingPlanes
    if (wasCut !== (cutPlane !== null)) {
      material.needsUpdate = true
      ghostMaterial.needsUpdate = true
      capBack.needsUpdate = true
      capFront.needsUpdate = true
    }
    // Filled only over a mesh known to be closed. An open mesh has no inside, and a guessed cap would
    // say it had one.
    // Not while the parts are apart: a cap fills the assembly's section, which is not where they are.
    const capped = cutPlane !== null && closedMesh && bounds !== null && explosion === 0
    cap.visible = capped
    model?.traverse((object) => {
      if (object.userData.stencil === true) object.visible = capped
    })
    if (capped && cutPlane !== null && bounds !== null) {
      const placed = capPlacement(cutPlane, bounds.min, bounds.max)
      cap.position.set(...placed.position)
      cap.scale.set(placed.size, placed.size, 1)
      cap.quaternion.setFromUnitVectors(new Vector3(0, 0, 1), new Vector3(...placed.normal))
    }
  }
  // three's raycaster meets what a section has cut away, so a hit counts only on the side still drawn.
  const drawn = (hit: Intersection) => cutPlane === null || kept(cutPlane, tuple(hit.point))
  // A hidden part is a gap in the index ranges drawn. three draws, and a raycast meets, only a
  // mesh's groups when its material is an array, so a hidden part is neither seen nor picked.
  const applyHidden = () => {
    if (model !== null) hideParts(model, material, hiddenParts)
  }
  const applyExplode = () => {
    if (model === null) return
    const source = partsMesh(model)
    const index = source?.geometry.index ?? null
    if (explosion === 0 || source === null || index === null || bounds === null) {
      if (exploded !== null) scene.remove(exploded)
      exploded = null
      model.visible = true
      return
    }
    if (exploded === null) {
      const group = new Group()
      const parts = source.userData.parts as number[]
      const position = source.geometry.getAttribute('position')
      const centres = partCentres(position.array, index.array, parts)
      let at = 0
      // The rung's one node has no transform (`glb.rs`), so a piece's position is its offset alone.
      parts.forEach((triangles, part) => {
        const piece = new BufferGeometry()
        piece.setIndex(index)
        piece.setAttribute('position', position)
        piece.setDrawRange(at, triangles * 3)
        at += triangles * 3
        const mesh = new Mesh(piece, material)
        mesh.renderOrder = 3
        mesh.userData = { part, centre: centres[part] }
        group.add(mesh)
      })
      scene.add(group)
      exploded = group
    }
    const { min, max } = bounds
    const offsets = explodeOffsets(
      exploded.children.map((piece) => piece.userData.centre as Vec3),
      [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2],
      explosion,
    )
    exploded.children.forEach((piece, i) => {
      piece.position.set(...(offsets[i] ?? [0, 0, 0]))
      piece.visible = !hiddenParts.has(piece.userData.part as number)
    })
    model.visible = false
  }
  const render = () => {
    renderer.render(scene, camera)
    labelRenderer.render(scene, camera)
    // The first frame with the part in it, not the resize observer's first call on an empty scene:
    // this mark is what the Phase 3 exit times.
    if (first && model !== null) {
      first = false
      performance.mark('lapidary:viewer-first-frame')
      onFirstFrame()
    }
  }
  controls.addEventListener('change', render)
  const resize = new ResizeObserver(() => {
    renderer.setSize(node.clientWidth, node.clientHeight, false)
    labelRenderer.setSize(node.clientWidth, node.clientHeight)
    fit()
    render()
  })
  resize.observe(node)

  return {
    show(next) {
      const meshes: Mesh[] = []
      next.traverse((object) => {
        if (!(object instanceof Mesh)) return
        object.material = material
        object.renderOrder = 3
        meshes.push(object)
      })
      // The cap's stencil passes, a pair per mesh sharing its geometry. Children of the mesh, so they
      // follow its transform, and a raycast passes straight through them.
      for (const mesh of meshes) {
        for (const pass of [capBack, capFront]) {
          const stencil = new Mesh(mesh.geometry, pass)
          stencil.userData.stencil = true
          stencil.renderOrder = 1
          stencil.visible = false
          stencil.raycast = () => {}
          mesh.add(stencil)
        }
      }
      const framing = model === null
      if (model !== null) {
        scene.remove(model)
        disposeModel(model)
      }
      model = next
      scene.add(next)
      applyHidden()
      // The pieces were the last rung's.
      if (exploded !== null) scene.remove(exploded)
      exploded = null
      // Framed once, on the first rung: a finer rung arriving must not move the camera out from
      // under someone who has already turned the part.
      if (framing) {
        const box = new Box3().setFromObject(next)
        bounds = { min: box.min.toArray() as Vec3, max: box.max.toArray() as Vec3 }
        applyCut()
        const frame = frameBox(bounds.min, bounds.max)
        camera.position.set(...frame.position)
        camera.near = frame.near
        camera.far = frame.far
        halfHeight = frame.halfHeight
        fit()
        controls.target.set(...frame.target)
        controls.update()
      }
      applyCut()
      applyExplode()
      render()
    },
    pick(x, y) {
      if (model === null) return null
      const box = renderer.domElement.getBoundingClientRect()
      const ndc = new Vector2(((x - box.left) / box.width) * 2 - 1, 1 - ((y - box.top) / box.height) * 2)
      raycaster.setFromCamera(ndc, camera)
      return picked(raycaster.intersectObject(model, true).find(drawn))
    },
    through(from) {
      if (model === null) return null
      raycaster.set(new Vector3(...from.point), new Vector3(...from.normal).negate())
      // Faces seen from behind only: the ray leaves the part through the far side of the wall. The
      // face it starts on is seen from behind too, at no distance, so distance skips it.
      material.side = BackSide
      const hits = raycaster.intersectObject(model, true)
      material.side = FrontSide
      return picked(hits.find((hit) => hit.distance > 1e-3 && drawn(hit)))
    },
    mark(points) {
      // A new geometry rather than a new attribute on the old one, whose bounding sphere would stay
      // the old marks' and cull the new ones.
      markers.geometry.dispose()
      markers.geometry = new BufferGeometry().setAttribute('position', new Float32BufferAttribute(points.flat(), 3))
      if (model !== null) render()
    },
    hide(next) {
      hiddenParts = next
      applyHidden()
      applyExplode()
      if (model !== null) render()
    },
    section(next) {
      cut = next
      applyCut()
      if (model !== null) render()
    },
    closed(next) {
      closedMesh = next
      applyCut()
      if (model !== null) render()
    },
    ghost(next) {
      if (ghostModel !== null) {
        scene.remove(ghostModel)
        disposeModel(ghostModel)
      }
      ghostModel = next
      if (next !== null) {
        next.traverse((object) => {
          if (!(object instanceof Mesh)) return
          object.material = ghostMaterial
          // After the part, which is opaque, and before the marks.
          object.renderOrder = 4
        })
        scene.add(next)
      }
      if (model !== null) render()
    },
    explode(next) {
      explosion = next
      // A label marks a face where the assembly drew it, which is not where a moved part is.
      labels.visible = next === 0
      applyExplode()
      applyCut()
      if (model !== null) render()
    },
    callout(points, text) {
      calloutCancel()
      calloutLine.geometry.dispose()
      if (points === null || points.length === 0 || text === null) {
        calloutLine.visible = false
        calloutLabel.visible = false
        render()
        return
      }
      const origin = points[0]!
      const mean = (axis: 0 | 1 | 2) => points.reduce((sum, point) => sum + point[axis], 0) / points.length
      const centre: Vec3 = [mean(0), mean(1), mean(2)]
      calloutElement.textContent = text
      calloutLabel.position.set(...centre)
      calloutLine.visible = points.length > 1
      calloutLabel.visible = true
      const positions = new Float32BufferAttribute(points.flat(), 3)
      calloutLine.geometry = new BufferGeometry().setAttribute('position', positions)
      // Drawn in from the first pick outward, with the label fading up, over `--duration-base`.
      const progress = { k: 0 }
      const draw = () => {
        points.forEach((point, index) => {
          positions.setXYZ(
            index,
            origin[0] + (point[0] - origin[0]) * progress.k,
            origin[1] + (point[1] - origin[1]) * progress.k,
            origin[2] + (point[2] - origin[2]) * progress.k,
          )
        })
        positions.needsUpdate = true
        calloutElement.style.opacity = String(progress.k)
        render()
      }
      draw()
      calloutCancel = tween(progress, { k: 1 }, draw)
    },
    annotate(next) {
      // `clear` removes each label, and three takes a removed label's element out of the page.
      labels.clear()
      for (const { text, at } of next ?? []) {
        const element = document.createElement('div')
        element.className = LABEL
        element.textContent = text
        const label = new CSS2DObject(element)
        label.position.set(...at)
        labels.add(label)
      }
      render()
    },
    dispose() {
      calloutCancel()
      scene.remove(calloutLabel)
      calloutLine.geometry.dispose()
      labels.clear()
      labelRenderer.domElement.remove()
      resize.disconnect()
      controls.dispose()
      if (model !== null) disposeModel(model)
      if (ghostModel !== null) disposeModel(ghostModel)
      markers.geometry.dispose()
      cap.geometry.dispose()
      // The session's material outlives this view, and the next view starts uncut.
      cut = null
      applyCut()
      renderer.domElement.remove()
      if (own) {
        markMaterial.dispose()
        ghostMaterial.dispose()
        capMaterial.dispose()
        capBack.dispose()
        capFront.dispose()
        material.dispose()
        renderer.dispose()
      } else {
        sessionInUse = false
      }
    },
  }
}

/**
 * An assembly's hidden parts, left out of what each mesh draws: its groups cover only the parts left,
 * and three draws only a mesh's groups when its material is an array. The cap's stencil passes share the
 * mesh's geometry, so they take the same array, or they would count a hidden part's inside and fill its
 * section.
 */
export function hideParts(model: Object3D, material: Material, hidden: ReadonlySet<number>) {
  model.traverse((object) => {
    if (!(object instanceof Mesh)) return
    if (object.userData.stencil === true) {
      const pass: Material | undefined = Array.isArray(object.material) ? object.material[0] : object.material
      if (pass === undefined) return
      object.material = hidden.size > 0 && Array.isArray(object.parent?.userData.parts) ? [pass] : pass
      return
    }
    const parts: unknown = object.userData.parts
    object.geometry.clearGroups()
    if (hidden.size === 0 || !Array.isArray(parts)) {
      object.material = material
      return
    }
    for (const { start, count } of visibleRanges(parts as number[], hidden)) {
      object.geometry.addGroup(start, count, 0)
    }
    object.material = [material]
  })
}

/** A raycast hit as `measure.ts` reads it: the point, the triangle's corners and its outward normal. */
function picked(hit: Intersection | undefined): Pick | null {
  const object = hit?.object
  const face = hit?.face
  if (hit === undefined || !(object instanceof Mesh) || face == null) return null
  const position = object.geometry.getAttribute('position')
  const corner = (index: number) =>
    tuple(new Vector3().fromBufferAttribute(position, index).applyMatrix4(object.matrixWorld))
  return {
    point: tuple(hit.point),
    normal: tuple(face.normal.clone().transformDirection(object.matrixWorld)),
    corners: [corner(face.a), corner(face.b), corner(face.c)],
  }
}

const tuple = (v: Vector3): Vec3 => [v.x, v.y, v.z]
