import {
  AmbientLight,
  Color,
  DataTexture,
  DirectionalLight,
  DoubleSide,
  LinearSRGBColorSpace,
  Mesh,
  MeshBasicMaterial,
  MeshLambertMaterial,
  MeshStandardMaterial,
  PlaneGeometry,
  type Object3D,
} from 'three'
import { LIGHT_DIR, type Vec3 } from '../lib/viewer-math'

/**
 * How a part is lit and what it is made of, wherever three.js draws one: the part's 3D view and
 * the grid's turntable.
 *
 * One definition because both views replace a thumbnail in place, and `raster.rs` drew that
 * thumbnail under exactly this light: a sun along `LIGHT_DIR` over a flat ambient fill, on a
 * pale grey, flat-shaded part. A canvas lit any other way would change the part's look the
 * moment it replaced the picture.
 *
 * `prepare()` compiles the view's shaders against these lights, and three keys a program on its
 * lights, so a change here reaches the compile and every view in one edit.
 */
export function studioLights(): Object3D[] {
  const sun = new DirectionalLight(0xffffff, 1.8)
  sun.position.set(...LIGHT_DIR)
  return [new AmbientLight(0xffffff, 0.45), sun]
}

export function partMaterial(): MeshStandardMaterial {
  return new MeshStandardMaterial({ color: new Color(0xb8bcc4), roughness: 0.75, flatShading: true })
}

/**
 * `raster.rs`'s shading, exactly: a face is `BASE × (AMBIENT + (1 − AMBIENT) × |n·L|)`, written out
 * as sRGB bytes with no conversion. A canvas drawing over a thumbnail in place (the turntable) has
 * to match it to the byte, or the crossfade shows a darker, flatter part arriving.
 *
 * three's Lambert term is `colour × (ambient + sun × max(n·L, 0)) / π`, so lights of `AMBIENT × π`
 * and `(1 − AMBIENT) × π` give the rasteriser's `k`, and a linear output with the colour taken as
 * linear writes it unconverted. Double-sided, for the `abs`: `raster.rs` lights both faces of a
 * triangle alike. The renderer drawing this must set `outputColorSpace` to `LinearSRGBColorSpace`.
 *
 * The part's 3D view does not use this yet: its marks, ghost and cap are colours chosen in sRGB,
 * and a linear output would shift every one of them.
 */
const RASTER_BASE = [0.82, 0.84, 0.88] as const
const RASTER_AMBIENT = 0.18

export function rasterLights(): Object3D[] {
  const sun = new DirectionalLight(0xffffff, (1 - RASTER_AMBIENT) * Math.PI)
  sun.position.set(...LIGHT_DIR)
  return [new AmbientLight(0xffffff, RASTER_AMBIENT * Math.PI), sun]
}

export function rasterMaterial(): MeshLambertMaterial {
  return new MeshLambertMaterial({
    color: new Color().setRGB(...RASTER_BASE, LinearSRGBColorSpace),
    flatShading: true,
    side: DoubleSide,
  })
}

/** A model's geometry, freed. Materials are shared across models and are not. */
export function disposeModel(model: Object3D) {
  model.traverse((object) => {
    if (object instanceof Mesh) object.geometry.dispose()
  })
}

/** How dark the contact shadow is at its centre. Enough to set a part down, not enough to read as a hole. */
export const SHADOW_OPACITY = 0.6

let shadowTexture: DataTexture | null = null

/** A soft round falloff, black, opaque at the centre and clear at the rim, drawn once for the session. */
function falloff(): DataTexture {
  if (shadowTexture !== null) return shadowTexture
  const size = 64
  const data = new Uint8Array(size * size * 4)
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const r = Math.min(1, Math.hypot(x - (size - 1) / 2, y - (size - 1) / 2) / ((size - 1) / 2))
      // Flat-topped and soft-edged, so the pool shows past the part's footprint rather than only under it.
      data[(y * size + x) * 4 + 3] = Math.round(255 * (1 - r * r) ** 1.5)
    }
  }
  shadowTexture = new DataTexture(data, size, size)
  shadowTexture.needsUpdate = true
  return shadowTexture
}

/** The contact shadow's material: never writes depth, so it can never hide the part it sits under. */
export function shadowMaterial(): MeshBasicMaterial {
  return new MeshBasicMaterial({
    color: 0x000000,
    map: falloff(),
    transparent: true,
    opacity: SHADOW_OPACITY,
    depthWrite: false,
  })
}

/**
 * A part set down on the lamp's ground: a soft dark pool on the plane of its lowest point.
 *
 * Without it every part floats, and on a flat ground floating reads as a picture pasted over the
 * stage rather than an object standing on it. Round, so a turning part needs no turning shadow, and
 * a little wider than the part's footprint. Never pickable: a measurement meets the part or nothing.
 */
export function contactShadow(min: Vec3, max: Vec3, material: MeshBasicMaterial): Mesh {
  const shadow = new Mesh(new PlaneGeometry(1, 1), material)
  placeShadow(shadow, min, max)
  shadow.renderOrder = -1
  shadow.raycast = () => {}
  return shadow
}

/** Move and size an existing contact shadow to a part's box. */
export function placeShadow(shadow: Mesh, min: Vec3, max: Vec3) {
  const across = Math.max(Math.hypot(max[0] - min[0], max[1] - min[1]), 1e-3) * 1.5
  shadow.scale.set(across, across, 1)
  // A hair under the lowest point, so the part's own bottom face is never fought over.
  shadow.position.set((min[0] + max[0]) / 2, (min[1] + max[1]) / 2, min[2] - across * 0.002)
}
