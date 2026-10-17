import { useEffect, useRef, useState, type ReactNode } from 'react'
import {
  Box3,
  Box3Helper,
  BufferGeometry,
  Color,
  GridHelper,
  Line,
  LineBasicMaterial,
  Mesh,
  MeshBasicMaterial,
  OrthographicCamera,
  Raycaster,
  Scene,
  SphereGeometry,
  Vector2,
  Vector3,
  WebGLRenderer,
  type MeshStandardMaterial,
  type Object3D,
} from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { ViewHelper } from 'three/addons/helpers/ViewHelper.js'
import { downloadUrl } from '../lib/api'
import { strings } from '../lib/strings'
import type { PartDetail } from '../lib/types'
import { between, frameBox, gridStep, millimetres, type Vec3 } from '../lib/viewer-math'
import { Figure } from './Figure'
import { Unviewable, loadMesh, meshFrom } from './mesh'
import { contactShadow, disposeModel, partMaterial, shadowMaterial, studioLights } from './studio'

/** What a view can be told. One part, shown once; everything else about it is the pointer's. */
type View = {
  show: (model: Object3D) => void
  /** The grid, the box and measuring, on or off (A12). */
  setTools: (tools: Tools) => void
  /** The view as it is on screen now, as a PNG. `null` when the browser will not hand the canvas over. */
  snapshot: () => Promise<Blob | null>
  dispose: () => void
}

/** The view's three aids (A12). */
type Tools = { grid: boolean; size: boolean; measure: boolean }
const NO_TOOLS: Tools = { grid: false, size: false, measure: false }
const TOOL_NAMES = Object.keys(NO_TOOLS) as (keyof Tools)[]

/** What the view tells the page about the part on screen: its box, the grid's step, and the picks. */
type Readout = { size: Vec3 | null; step: number | null; picks: Vec3[] }

/** A click that moved less than this is a pick; more is a turn of the view. */
const CLICK_PX = 4

/** What a view draws with. Everything here holds GPU state; the lights do not, so they are not here. */
type Kit = {
  renderer: WebGLRenderer
  material: MeshStandardMaterial
  shadowMaterial: MeshBasicMaterial
}

function kit(): Kit {
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  return { renderer, material: partMaterial(), shadowMaterial: shadowMaterial() }
}

/**
 * One kit for the session. A renderer made per open compiled the part's shaders again on every
 * open. Its materials are never disposed either, because disposing a material frees the program
 * compiled for it. A second view open at the same time, which nothing does today, gets its own.
 */
let session: Kit | null = null
let sessionInUse = false

/**
 * **The part in 3D, from the part's own file.**
 *
 * The owner's call (2026-10-03): Lapidary is a visual library you look through and search, not a
 * renderer. So nothing tessellates a part for us to draw — a person presses **view** and the
 * browser fetches `?variant=original` and reads it with three's own loader for that format
 * (`mesh.ts`). The bytes are the bytes that were ingested; we never hand over a conversion.
 *
 * What that costs, said plainly, because it is the trade and not an oversight:
 *
 * - **A large mesh is a large download.** There is no coarse rung to paint first, so a 200 MB STL
 *   is 200 MB before anything appears. The picture holds the frame until it does, and a failure
 *   says to download the file and open it in a real tool.
 * - **Measuring is on the mesh, and says so** (A12, owner 2026-10-18). The pivot took out the measuring
 *   that snapped to CAD entities; what came back is a grid, the part's box, and the distance between two
 *   points clicked on the drawn triangles, which are the file's own. Every figure carries its ≈ through
 *   `Figure`. No section, no explode, no PMI: those needed geometry we no longer produce.
 * - **A STEP part has no view here at all.** `Preview` offers the button only for the three mesh
 *   formats; a CAD part keeps its download and its figures, which is what it had before.
 *
 * Drawn on demand, never in a loop: a frame when the camera moves, the box resizes, the model
 * arrives or an aid changes, and nothing otherwise — except while the axis gizmo turns the view, which
 * is instant under `prefers-reduced-motion`. Nothing spins or eases on its own, so there is no motion for
 * `prefers-reduced-motion` to take away.
 *
 * Shaded flat, one normal per face, as the thumbnail was (`raster.rs`): averaging normals across a
 * sharp rim is what blurred the first cylinder's cap into its side, and a part is looked at here to
 * see its form.
 *
 * The default export, for `lazy()`: three.js is in this chunk and nowhere else.
 */
export default function Viewer({
  part,
  poster,
  stage = false,
  keep,
}: {
  part: PartDetail
  /** The part's picture, held under the canvas until the first frame is drawn over it. */
  poster: ReactNode
  /** Fill the parent on the lamp's ground, as the quick look's stage does, rather than a 22rem square. */
  stage?: boolean
  /**
   * Keeps what is on screen as the part's picture: the PNG, and back the server's refusal or
   * `null`. Absent where the page records nothing, and then there is no button.
   */
  keep?: (png: Blob) => Promise<string | null>
}) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<View | null>(null)
  const [painted, setPainted] = useState(false)
  const [posterGone, setPosterGone] = useState(false)
  useEffect(() => {
    if (!painted) return
    const timer = setTimeout(() => setPosterGone(true), 220)
    return () => clearTimeout(timer)
  }, [painted])
  const [failed, setFailed] = useState<string | null>(null)
  const [tools, setTools] = useState<Tools>(NO_TOOLS)
  const [readout, setReadout] = useState<Readout>({ size: null, step: null, picks: [] })
  useEffect(() => {
    view.current?.setTools(tools)
  }, [tools])
  const toggle = (tool: keyof Tools) => setTools((now) => ({ ...now, [tool]: !now[tool] }))
  const [keeping, setKeeping] = useState<'idle' | 'saving' | 'kept' | string>('idle')
  const keepLabel =
    keeping === 'saving'
      ? strings.viewer.keepingView
      : keeping === 'kept'
        ? strings.viewer.keptView
        : strings.viewer.keepView
  const keepNote = keeping === 'idle' || keeping === 'saving' || keeping === 'kept' ? null : keeping

  const format = part.sourceFormat
  const revision = part.revision
  useEffect(() => {
    const node = host.current
    if (node === null) return
    // A revision with no source row has no file to read. `Preview` offers **view** only for a
    // known mesh format, so nothing reaches this — but it is said rather than returned from,
    // because a silent return is exactly the failure this goal exists to remove: the rung effect
    // this replaced returned early on a part with no L0 and left no spinner, no error and
    // nothing to diagnose.
    if (format === null) {
      setFailed(strings.viewer.failed)
      return
    }
    if (view.current === null) view.current = createView(node, () => setPainted(true), setReadout)
    const current = view.current
    // The model last drawn is kept (A11): the quick look, then the part's page, read it once.
    if (lastDrawn?.revision === revision) {
      current.show(meshFrom(lastDrawn.positions))
      return
    }
    const aborter = new AbortController()
    let stale = false
    // `?variant=original` returns the ingested bytes, byte for byte. Fetched rather than handed
    // to a loader's own `load`, so the parse is the function `mesh.test.ts` exercises and the
    // request is one this component can abandon when the part changes under it.
    fetch(downloadUrl(revision), { signal: aborter.signal })
      .then((response) => {
        if (!response.ok) throw new Error(String(response.status))
        return response.arrayBuffer()
      })
      // Read off the page's thread (A11), so a large model never freezes the screen while it loads.
      .then((bytes) => (stale ? null : loadMesh(format, bytes, aborter.signal)))
      .then((mesh) => {
        if (stale || mesh === null) return
        lastDrawn = { revision, positions: mesh.geometry.getAttribute('position').array as Float32Array }
        current.show(mesh)
      })
      .catch((error: unknown) => {
        // An abort is this effect being replaced, not a failure anyone needs told about.
        if (stale || (error instanceof DOMException && error.name === 'AbortError')) return
        // **Only our own sentences reach the screen.** `Unviewable` is the one failure with
        // something useful to say — it names the format — and everything else is three's loader
        // text or the browser's `Failed to fetch`, which tell a reader nothing and come from no
        // `strings.ts`. Both are the same thing to somebody looking at it: this did not draw, so
        // download the file and open it in a tool that will.
        setFailed(error instanceof Unviewable ? error.message : strings.viewer.failed)
      })
    return () => {
      stale = true
      aborter.abort()
    }
  }, [revision, format])

  useEffect(
    () => () => {
      view.current?.dispose()
      view.current = null
    },
    [],
  )

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
          hidden={failed !== null}
          className="absolute inset-0"
        />
        {/*
          The poster stays until the canvas has faded in under it: it goes to opacity 0 on the first
          frame, over the stylesheet's own opacity transition, and leaves once that has run. The view
          is lit as a studio and the picture is a picture, so a cut between them would jump.
        */}
        {painted && posterGone && failed === null ? null : (
          <div className={painted && failed === null ? 'absolute inset-0 opacity-0' : 'absolute inset-0'}>
            {poster}
          </div>
        )}
        {painted && failed === null ? (
          <>
            <div
              role="toolbar"
              aria-label={strings.viewer.tools}
              className="absolute top-2 left-2 flex gap-1 rounded-[var(--radius-ctl)] bg-[var(--color-surface)]/80 p-0.5"
            >
              {TOOL_NAMES.map((tool) => (
                <button
                  key={tool}
                  type="button"
                  aria-pressed={tools[tool]}
                  onClick={() => toggle(tool)}
                  className={`${TOOL} ${tools[tool] ? 'bg-[var(--color-raised)] text-[var(--color-accent)]' : 'text-[var(--color-muted)]'}`}
                >
                  {strings.viewer.tool[tool]}
                </button>
              ))}
            </div>
            <Readouts tools={tools} readout={readout} />
          </>
        ) : null}
        {failed === null ? (
          painted ? null : (
            <p aria-live="polite" className="absolute bottom-2 left-2 text-xs text-[var(--color-muted)]">
              {strings.viewer.loading}
            </p>
          )
        ) : (
          <p role="alert" className="absolute inset-x-2 bottom-2 text-xs text-[var(--color-muted)]">
            {failed}
          </p>
        )}
      </div>
      {keep === undefined || !painted || failed !== null ? null : (
        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
          <button
            type="button"
            disabled={keeping === 'saving'}
            onClick={() => {
              const current = view.current
              if (current === null) return
              setKeeping('saving')
              void current
                .snapshot()
                .then((png) => (png === null ? strings.viewer.keepViewFailed : keep(png)))
                .then(
                  (refusal) => setKeeping(refusal ?? 'kept'),
                  () => setKeeping(strings.viewer.keepViewFailed),
                )
            }}
            className="ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-2 py-1 duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50"
          >
            {keepLabel}
          </button>
          {keepNote === null ? null : (
            <span role="alert" className="text-[var(--color-muted)]">
              {keepNote}
            </span>
          )}
        </div>
      )}
    </div>
  )
}

const TOOL =
  'ease-mechanical rounded-[var(--radius-ctl)] px-2 py-1 text-xs duration-[var(--duration-fast)] hover:text-[var(--color-text)]'

/** A length with its ≈: every figure here is read from the mesh. */
function Length({ mm }: { mm: number }) {
  return <Figure figure={{ value: mm, approximate: true }} render={(v) => strings.viewer.mm(millimetres(v))} />
}

/** What the aids that are on say, in the view's lower left, above nothing the gizmo needs. */
function Readouts({ tools, readout }: { tools: Tools; readout: Readout }) {
  const { size, step, picks } = readout
  if (!tools.grid && !tools.size && !tools.measure) return null
  const [a, b] = picks
  const measured = a !== undefined && b !== undefined ? between(a, b) : null
  return (
    <div
      aria-live="polite"
      className="tabular absolute bottom-2 left-2 flex max-w-[calc(100%-9rem)] flex-col gap-0.5 rounded-[var(--radius-ctl)] bg-[var(--color-surface)]/85 px-2 py-1.5 text-xs text-[var(--color-text)]"
    >
      {tools.grid && step !== null ? (
        <p>
          {strings.viewer.gridStep}
          {/* Not a measurement: the step the grid was drawn at, so no ≈. */}
          {strings.viewer.mm(String(Number(step.toPrecision(3))))}
        </p>
      ) : null}
      {tools.size && size !== null ? (
        <p>
          {strings.viewer.size}
          <Length mm={size[0]} />
          {strings.viewer.by}
          <Length mm={size[1]} />
          {strings.viewer.by}
          <Length mm={size[2]} />
        </p>
      ) : null}
      {tools.measure ? (
        <>
          {picks.length < 2 ? (
            <p className="text-[var(--color-muted)]">
              {picks.length === 0 ? strings.viewer.measureFirst : strings.viewer.measureSecond}
            </p>
          ) : null}
          {picks.map((point, i) => (
            <p key={i}>
              {strings.viewer.point(i)}
              <Length mm={point[0]} />, <Length mm={point[1]} />, <Length mm={point[2]} />
            </p>
          ))}
          {measured === null ? null : (
            <>
              <p className="font-semibold text-[var(--color-bright)]">
                {strings.viewer.distance}
                <Length mm={measured.distance} />
              </p>
              <p className="text-[var(--color-muted)]">
                {strings.viewer.delta}
                <Length mm={measured.delta[0]} />, <Length mm={measured.delta[1]} />, <Length mm={measured.delta[2]} />
              </p>
            </>
          )}
        </>
      ) : null}
    </div>
  )
}

/**
 * The positions of the last model drawn, by revision (A11). One model, because a large one is hundreds of
 * megabytes. ponytail: a single slot; a small cache of several is the upgrade if people flick between parts.
 */
let lastDrawn: { revision: PartDetail['revision']; positions: Float32Array } | null = null

/** For tests, which draw one part many times. */
export function forgetLastDrawn() {
  lastDrawn = null
}

function createView(node: HTMLElement, onFirstFrame: () => void, onReadout: (readout: Readout) => void): View {
  const own = sessionInUse
  const { renderer, material, shadowMaterial: shadowSurface } = own ? kit() : (session ??= kit())
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

  let first = true
  let model: Object3D | null = null
  let shadow: Mesh | null = null

  // The aids (A12). Drawn over the part in the accent, so a measurement reads against any surface.
  const accent = new Color(
    getComputedStyle(document.documentElement).getPropertyValue('--color-accent').trim() || '#3b82f6',
  )
  const aids = new LineBasicMaterial({ color: accent, depthTest: false, transparent: true })
  const marker = new MeshBasicMaterial({ color: accent, depthTest: false, transparent: true })
  let tools: Tools = NO_TOOLS
  let box: Box3 | null = null
  let radius = 1
  let grid: GridHelper | null = null
  let outline: Box3Helper | null = null
  let picks: Vec3[] = []
  const pickMarks: Object3D[] = []
  const gizmo = new ViewHelper(camera, renderer.domElement)
  gizmo.setLabels(strings.viewer.axisX, strings.viewer.axisY, strings.viewer.axisZ)
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false

  const readout = () =>
    onReadout({
      size: box === null ? null : (box.getSize(new Vector3()).toArray() as Vec3),
      step: grid === null || box === null ? null : gridStep(floorSpan(box)),
      picks,
    })
  const clearPicks = () => {
    for (const mark of pickMarks) {
      scene.remove(mark)
      if (mark instanceof Mesh || mark instanceof Line) mark.geometry.dispose()
    }
    pickMarks.length = 0
    picks = []
  }
  const placeAids = () => {
    if (grid !== null) {
      scene.remove(grid)
      grid.dispose()
      grid = null
    }
    if (outline !== null) {
      scene.remove(outline)
      outline.geometry.dispose()
      outline = null
    }
    if (box === null) return
    if (tools.grid) {
      const step = gridStep(floorSpan(box))
      const cells = Math.ceil((floorSpan(box) * 1.6) / step / 2) * 2
      grid = new GridHelper(cells * step, cells, accent, 0x4a5160)
      // three's grid lies in XZ; the part's floor is XY, Z up, as the file has it.
      grid.rotation.x = Math.PI / 2
      const centre = box.getCenter(new Vector3())
      grid.position.set(centre.x, centre.y, box.min.z)
      for (const material of [grid.material].flat()) {
        material.transparent = true
        material.opacity = 0.55
        material.depthWrite = false
      }
      scene.add(grid)
    }
    if (tools.size) {
      outline = new Box3Helper(box, accent)
      outline.material = aids
      scene.add(outline)
    }
    if (!tools.measure) clearPicks()
  }
  const mark = (point: Vec3) => {
    const dot = new Mesh(new SphereGeometry(radius * 0.012, 12, 8), marker)
    dot.position.set(...point)
    dot.renderOrder = 10
    scene.add(dot)
    pickMarks.push(dot)
    if (picks.length === 2) {
      const line = new Line(new BufferGeometry().setFromPoints(picks.map((p) => new Vector3(...p))), aids)
      line.renderOrder = 10
      scene.add(line)
      pickMarks.push(line)
    }
  }

  const draw = (withAids: boolean) => {
    for (const aid of [grid, outline, ...pickMarks]) if (aid !== null) aid.visible = withAids
    // Cleared once, here: the gizmo renders into its corner with a second `render`, and an automatic
    // clear there would wipe the part drawn a line above.
    renderer.autoClear = false
    renderer.clear()
    renderer.render(scene, camera)
    if (withAids && model !== null) gizmo.render(renderer)
  }
  const render = () => {
    draw(true)
    // The first frame with the part in it, not the resize observer's first call on an empty scene.
    if (first && model !== null) {
      first = false
      performance.mark('lapidary:viewer-first-frame')
      onFirstFrame()
    }
  }
  controls.addEventListener('change', render)

  // A click is a pick, or a turn on the gizmo; a drag is the controls' and nothing else's.
  const pointer = new Vector2()
  const raycaster = new Raycaster()
  let down: { x: number; y: number } | null = null
  let turning = false
  const turn = (last: number) => {
    const now = performance.now()
    gizmo.update(reduced ? 1e9 : (now - last) / 1000)
    render()
    if (gizmo.animating) requestAnimationFrame(() => turn(now))
    else {
      turning = false
      controls.update()
    }
  }
  const onDown = (event: PointerEvent) => {
    down = { x: event.clientX, y: event.clientY }
  }
  const onUp = (event: PointerEvent) => {
    const from = down
    down = null
    if (from === null || Math.hypot(event.clientX - from.x, event.clientY - from.y) > CLICK_PX) return
    if (!turning && gizmo.handleClick(event)) {
      turning = true
      requestAnimationFrame(() => turn(performance.now()))
      return
    }
    if (!tools.measure || model === null) return
    const rect = renderer.domElement.getBoundingClientRect()
    pointer.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1)
    raycaster.setFromCamera(pointer, camera)
    const hit = raycaster.intersectObject(model, true)[0]
    if (hit === undefined) return
    // A third pick starts the next measurement.
    if (picks.length === 2) clearPicks()
    picks = [...picks, hit.point.toArray() as Vec3]
    mark(picks[picks.length - 1]!)
    readout()
    render()
  }
  renderer.domElement.addEventListener('pointerdown', onDown)
  renderer.domElement.addEventListener('pointerup', onUp)
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
      if (model !== null) {
        scene.remove(model)
        disposeModel(model)
      }
      model = next
      scene.add(next)
      // A loader's own axes, untouched. `glb.rs` swapped nothing either, so a Z-up STL stands
      // here exactly as it stood in the rung this replaced — and `mesh.test.ts` is what says the
      // box the browser arrives at is the box the part's figures state.
      const box3 = new Box3().setFromObject(next)
      let framed: Box3 | null = null
      const bounds: { min: Vec3; max: Vec3 } = {
        min: box3.min.toArray() as Vec3,
        max: box3.max.toArray() as Vec3,
      }
      // An empty or non-finite box frames to nothing and leaves the camera at the origin, so a
      // mesh that parsed to no geometry is a failure of the parse rather than a blank canvas.
      if (!box3.isEmpty() && Number.isFinite(box3.min.length()) && Number.isFinite(box3.max.length())) {
        framed = box3
        shadow = contactShadow(bounds.min, bounds.max, shadowSurface)
        scene.add(shadow)
        const frame = frameBox(bounds.min, bounds.max)
        camera.position.set(...frame.position)
        camera.near = frame.near
        camera.far = frame.far
        halfHeight = frame.halfHeight
        fit()
        controls.target.set(...frame.target)
        controls.update()
        gizmo.center.set(...frame.target)
        radius = frame.halfHeight
      }
      box = framed
      clearPicks()
      placeAids()
      readout()
      render()
    },
    setTools(next) {
      tools = next
      placeAids()
      readout()
      render()
    },
    snapshot() {
      // Drawn and read in one task: without `preserveDrawingBuffer` the browser may clear the
      // buffer once a frame is shown, so a read on its own later would be a blank picture. The aids
      // are left out: the picture is the part's, not a measurement of it.
      draw(false)
      const shot = new Promise<Blob | null>((resolve) => renderer.domElement.toBlob(resolve, 'image/png'))
      draw(true)
      return shot
    },
    dispose() {
      clearPicks()
      grid?.dispose()
      outline?.geometry.dispose()
      aids.dispose()
      marker.dispose()
      gizmo.dispose()
      renderer.domElement.removeEventListener('pointerdown', onDown)
      renderer.domElement.removeEventListener('pointerup', onUp)
      shadow?.geometry.dispose()
      resize.disconnect()
      controls.dispose()
      if (model !== null) disposeModel(model)
      renderer.domElement.remove()
      if (own) {
        material.dispose()
        renderer.dispose()
      } else {
        sessionInUse = false
      }
    },
  }
}

/** The part's floor: the longer of its two sides on the XY plane, which the grid lies on. */
function floorSpan(box: Box3): number {
  return Math.max(box.max.x - box.min.x, box.max.y - box.min.y)
}
