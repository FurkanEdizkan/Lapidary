import {
  AmbientLight,
  Color,
  DirectionalLight,
  DoubleSide,
  LinearSRGBColorSpace,
  Mesh,
  MeshLambertMaterial,
  MeshStandardMaterial,
  type Object3D,
} from 'three'
import { LIGHT_DIR } from '../lib/viewer-math'

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
