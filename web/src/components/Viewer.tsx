import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AmbientLight,
  BackSide,
  Box3,
  BufferGeometry,
  Color,
  DirectionalLight,
  Float32BufferAttribute,
  FrontSide,
  Mesh,
  MeshStandardMaterial,
  OrthographicCamera,
  Points,
  PointsMaterial,
  Raycaster,
  Scene,
  Vector2,
  Vector3,
  WebGLRenderer,
  type Intersection,
  type Object3D,
} from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { blobUrl, fetchBatchStatus, fetchEntities, fetchStructure, requestRung } from '../lib/api'
import { PICKS, measure, nearestCorner, placeEntities, type Pick, type Tool } from '../lib/measure'
import { strings } from '../lib/strings'
import type { BatchId, BlobHash, PartDetail } from '../lib/types'
import { LIGHT_DIR, frameBox, visibleRanges, type Vec3 } from '../lib/viewer-math'
import { MeasureBar } from './Measure'

type View = {
  show: (model: Object3D) => void
  /** What a click at a point on the screen met, or `null` where it met no part. */
  pick: (x: number, y: number) => Pick | null
  /** Where a ray from a pick, straight into the part, leaves it again: the far side of a wall. */
  through: (from: Pick) => Pick | null
  mark: (points: readonly Vec3[]) => void
  /** Leave these parts out, by their depth-first place in the tree; kept for every rung shown after. */
  hide: (hidden: ReadonlySet<number>) => void
  dispose: () => void
}

const NONE: ReadonlySet<number> = new Set()

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
type Kit = { renderer: WebGLRenderer; material: MeshStandardMaterial; markMaterial: PointsMaterial }

function kit(): Kit {
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  return {
    renderer,
    material: new MeshStandardMaterial({ color: new Color(0xb8bcc4), roughness: 0.75, flatShading: true }),
    markMaterial: new PointsMaterial({ color: MARK, size: 7, sizeAttenuation: false, depthTest: false }),
  }
}

/**
 * No marks, as an empty position buffer rather than no buffer at all. three compiles a different
 * program for points with no position attribute, and a view's marks gain one as soon as anything
 * is marked, so both states start from this one.
 */
function noMarks(): BufferGeometry {
  return new BufferGeometry().setAttribute('position', new Float32BufferAttribute([], 3))
}

/** The lights every view is drawn under. */
function lights(): Object3D[] {
  const sun = new DirectionalLight(0xffffff, 1.8)
  sun.position.set(...LIGHT_DIR)
  return [new AmbientLight(0xffffff, 0.45), sun]
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
    scene.add(...lights(), new Mesh(triangle, session.material), new Points(noMarks(), session.markMaterial))
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
}: {
  part: PartDetail
  poster: ReactNode
  hidden?: ReadonlySet<number>
  onParts?: (parts: number | null) => void
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

  useEffect(() => {
    view.current?.hide(hidden)
  }, [hidden])
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
    enabled: fine && part.entities !== null,
  })
  const structure = useQuery({
    queryKey: ['structure', part.structure],
    queryFn: () => fetchStructure(part.structure as BlobHash),
    enabled: fine && part.structure !== null,
  })
  // Each entity where the mesh drew it, or `null` while either half is on its way. A part whose
  // entities or tree could not be read is measured as a mesh is, every value approximate, rather
  // than snapped to entities left where they were never drawn.
  const placed = useMemo(() => {
    if (part.entities === null || entities.isError || structure.isError) return []
    if (entities.data === undefined || (part.structure !== null && structure.data === undefined)) return null
    return placeEntities(entities.data, structure.data ?? null)
  }, [part.entities, part.structure, entities.data, entities.isError, structure.data, structure.isError])

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
    <div className="w-full max-w-[22rem]">
      <div className="relative aspect-square w-full overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
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
        ) : null}
      </div>
      {failed ? null : <MeasureBar tool={tool} onTool={choose} reading={reading} note={note} />}
    </div>
  )
}

function createView(node: HTMLElement, onFirstFrame: () => void): View {
  const own = sessionInUse
  const { renderer, material, markMaterial } = own ? kit() : (session ??= kit())
  sessionInUse = true
  renderer.setSize(node.clientWidth, node.clientHeight, false)
  renderer.domElement.style.width = '100%'
  renderer.domElement.style.height = '100%'
  node.appendChild(renderer.domElement)

  const scene = new Scene()
  scene.add(...lights())
  const camera = new OrthographicCamera(-1, 1, 1, -1, 0.1, 10)
  camera.up.set(0, 0, 1)
  let halfHeight = 1
  // The view's width follows the box's shape; its height is the framing's, so a resize never
  // changes how large the part is drawn, only how much room is beside it.
  const fit = () => {
    const aspect = node.clientWidth / Math.max(node.clientHeight, 1)
    camera.left = -halfHeight * aspect
    camera.right = halfHeight * aspect
    camera.top = halfHeight
    camera.bottom = -halfHeight
    camera.updateProjectionMatrix()
  }
  fit()
  const controls = new OrbitControls(camera, renderer.domElement)

  const raycaster = new Raycaster()
  const markers = new Points(noMarks(), markMaterial)
  // Drawn over the part, so a mark on a face turned away still shows where it was put.
  markers.renderOrder = 1
  scene.add(markers)

  let first = true
  let model: Object3D | null = null
  let hiddenParts = NONE
  // A hidden part is a gap in the index ranges drawn. three draws, and a raycast meets, only a
  // mesh's groups when its material is an array, so a hidden part is neither seen nor picked.
  const applyHidden = () => {
    model?.traverse((object) => {
      if (!(object instanceof Mesh)) return
      const parts: unknown = object.userData.parts
      object.geometry.clearGroups()
      if (hiddenParts.size === 0 || !Array.isArray(parts)) {
        object.material = material
        return
      }
      for (const { start, count } of visibleRanges(parts as number[], hiddenParts)) {
        object.geometry.addGroup(start, count, 0)
      }
      object.material = [material]
    })
  }
  const render = () => {
    renderer.render(scene, camera)
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
    fit()
    render()
  })
  resize.observe(node)

  return {
    show(next) {
      next.traverse((object) => {
        if (object instanceof Mesh) object.material = material
      })
      const framing = model === null
      if (model !== null) {
        scene.remove(model)
        disposeModel(model)
      }
      model = next
      scene.add(next)
      applyHidden()
      // Framed once, on the first rung: a finer rung arriving must not move the camera out from
      // under someone who has already turned the part.
      if (framing) {
        const box = new Box3().setFromObject(next)
        const frame = frameBox(box.min.toArray() as Vec3, box.max.toArray() as Vec3)
        camera.position.set(...frame.position)
        camera.near = frame.near
        camera.far = frame.far
        halfHeight = frame.halfHeight
        fit()
        controls.target.set(...frame.target)
        controls.update()
      }
      render()
    },
    pick(x, y) {
      if (model === null) return null
      const box = renderer.domElement.getBoundingClientRect()
      const ndc = new Vector2(((x - box.left) / box.width) * 2 - 1, 1 - ((y - box.top) / box.height) * 2)
      raycaster.setFromCamera(ndc, camera)
      return picked(raycaster.intersectObject(model, true)[0])
    },
    through(from) {
      if (model === null) return null
      raycaster.set(new Vector3(...from.point), new Vector3(...from.normal).negate())
      // Faces seen from behind only: the ray leaves the part through the far side of the wall. The
      // face it starts on is seen from behind too, at no distance, so distance skips it.
      material.side = BackSide
      const hits = raycaster.intersectObject(model, true)
      material.side = FrontSide
      return picked(hits.find((hit) => hit.distance > 1e-3))
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
      if (model !== null) render()
    },
    dispose() {
      resize.disconnect()
      controls.dispose()
      if (model !== null) disposeModel(model)
      markers.geometry.dispose()
      renderer.domElement.remove()
      if (own) {
        markMaterial.dispose()
        material.dispose()
        renderer.dispose()
      } else {
        sessionInUse = false
      }
    },
  }
}

function disposeModel(model: Object3D) {
  model.traverse((object) => {
    if (object instanceof Mesh) object.geometry.dispose()
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
