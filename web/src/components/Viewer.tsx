import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AmbientLight,
  Box3,
  Color,
  DirectionalLight,
  Mesh,
  MeshStandardMaterial,
  OrthographicCamera,
  Scene,
  WebGLRenderer,
  type Object3D,
} from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { blobUrl, fetchBatchStatus, requestRung } from '../lib/api'
import { strings } from '../lib/strings'
import type { BatchId, PartDetail } from '../lib/types'
import { LIGHT_DIR, frameBox, type Vec3 } from '../lib/viewer-math'

type View = { show: (model: Object3D) => void; dispose: () => void }

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
 * The default export, for `lazy()`: three.js is in this chunk and nowhere else.
 */
export default function Viewer({ part, poster }: { part: PartDetail; poster: ReactNode }) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<View | null>(null)
  const [painted, setPainted] = useState(false)
  const [failed, setFailed] = useState(false)
  const [batch, setBatch] = useState<BatchId | null>(null)
  const queryClient = useQueryClient()
  const hash = part.tessellationL1 ?? part.tessellationL0

  useEffect(() => {
    const node = host.current
    if (node === null || hash === null) return
    if (view.current === null) view.current = createView(node, () => setPainted(true))
    const current = view.current
    let stale = false
    new GLTFLoader()
      .loadAsync(blobUrl(hash))
      .then((gltf) => {
        if (!stale) current.show(gltf.scene)
      })
      .catch(() => {
        if (!stale) setFailed(true)
      })
    return () => {
      stale = true
    }
  }, [hash])

  useEffect(
    () => () => {
      view.current?.dispose()
      view.current = null
    },
    [],
  )

  // Ask for L1 once per part that lacks it. A rung that exists but the detail did not know about
  // yet is a stale detail, so it is read again; a queued one is watched until its batch finishes.
  useEffect(() => {
    if (part.tessellationL0 === null || part.tessellationL1 !== null) return
    let stale = false
    requestRung(part.id, 'l1')
      .then((answer) => {
        if (stale) return
        if (answer.kind === 'ready') {
          void queryClient.invalidateQueries({ queryKey: ['part', part.id] })
        } else if (answer.queued.queued > 0) {
          setBatch(answer.queued.batchId)
        }
      })
      // L0 is already on screen, and a view without the finer rung is still a view.
      .catch(() => undefined)
    return () => {
      stale = true
    }
  }, [part.id, part.tessellationL0, part.tessellationL1, queryClient])

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

  return (
    <div className="relative aspect-square w-full max-w-[22rem] overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
      <div
        ref={host}
        role="img"
        aria-label={strings.viewer.label(part.name)}
        hidden={failed}
        className="absolute inset-0"
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
  )
}

function createView(node: HTMLElement, onFirstFrame: () => void): View {
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  renderer.setSize(node.clientWidth, node.clientHeight, false)
  renderer.domElement.style.width = '100%'
  renderer.domElement.style.height = '100%'
  node.appendChild(renderer.domElement)

  const scene = new Scene()
  scene.add(new AmbientLight(0xffffff, 0.45))
  const sun = new DirectionalLight(0xffffff, 1.8)
  sun.position.set(...LIGHT_DIR)
  scene.add(sun)
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
  const material = new MeshStandardMaterial({
    color: new Color(0xb8bcc4),
    roughness: 0.75,
    flatShading: true,
  })

  let first = true
  let model: Object3D | null = null
  const render = () => {
    renderer.render(scene, camera)
    if (first) {
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
    dispose() {
      resize.disconnect()
      controls.dispose()
      if (model !== null) disposeModel(model)
      material.dispose()
      renderer.dispose()
      renderer.domElement.remove()
    },
  }
}

function disposeModel(model: Object3D) {
  model.traverse((object) => {
    if (object instanceof Mesh) object.geometry.dispose()
  })
}
