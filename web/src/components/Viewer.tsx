import { useEffect, useRef, useState, type ReactNode } from 'react'
import {
  Box3,
  Mesh,
  OrthographicCamera,
  Scene,
  WebGLRenderer,
  type MeshStandardMaterial,
  type MeshBasicMaterial,
  type Object3D,
} from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { downloadUrl } from '../lib/api'
import { strings } from '../lib/strings'
import type { PartDetail } from '../lib/types'
import { frameBox, type Vec3 } from '../lib/viewer-math'
import { Unviewable, parseMesh } from './mesh'
import { contactShadow, disposeModel, partMaterial, shadowMaterial, studioLights } from './studio'

/** What a view can be told. One part, shown once; everything else about it is the pointer's. */
type View = {
  show: (model: Object3D) => void
  dispose: () => void
}

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
 * - **No measuring, no section, no explode, no PMI.** Every one of those needed geometry we had
 *   produced — the B-rep-aligned L2 for a pick to snap to, `extras.parts` for a part to be hidden
 *   or moved, the entity list for a label to be placed. A stock loader provides none of it, and
 *   measuring a mesh "approximately" against entities that were never placed would be exactly the
 *   lie `CLAUDE.md` forbids. Measurement lives in the tool the part is opened in.
 * - **A STEP part has no view here at all.** `Preview` offers the button only for the three mesh
 *   formats; a CAD part keeps its download and its figures, which is what it had before.
 *
 * Drawn on demand, never in a loop: a frame when the camera moves, the box resizes or the model
 * arrives, and nothing otherwise. Nothing spins or eases on its own, so there is no motion for
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
}: {
  part: PartDetail
  /** The part's picture, held under the canvas until the first frame is drawn over it. */
  poster: ReactNode
  /** Fill the parent on the lamp's ground, as the quick look's stage does, rather than a 22rem square. */
  stage?: boolean
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
    if (view.current === null) view.current = createView(node, () => setPainted(true))
    const current = view.current
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
      .then((bytes) => {
        if (stale) return
        current.show(parseMesh(format, bytes))
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
    </div>
  )
}

function createView(node: HTMLElement, onFirstFrame: () => void): View {
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
  const render = () => {
    renderer.render(scene, camera)
    // The first frame with the part in it, not the resize observer's first call on an empty scene.
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
      if (model !== null) {
        scene.remove(model)
        disposeModel(model)
      }
      model = next
      scene.add(next)
      // A loader's own axes, untouched. `glb.rs` swapped nothing either, so a Z-up STL stands
      // here exactly as it stood in the rung this replaced — and `mesh.test.ts` is what says the
      // box the browser arrives at is the box the part's figures state.
      const box = new Box3().setFromObject(next)
      const bounds: { min: Vec3; max: Vec3 } = {
        min: box.min.toArray() as Vec3,
        max: box.max.toArray() as Vec3,
      }
      // An empty or non-finite box frames to nothing and leaves the camera at the origin, so a
      // mesh that parsed to no geometry is a failure of the parse rather than a blank canvas.
      if (!box.isEmpty() && Number.isFinite(box.min.length()) && Number.isFinite(box.max.length())) {
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
      }
      render()
    },
    dispose() {
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
