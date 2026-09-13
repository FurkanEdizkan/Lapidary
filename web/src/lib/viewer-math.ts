/**
 * The viewer's arithmetic, kept out of the component so it can be tested without a GPU.
 *
 * The directions are `raster.rs`'s, so the 3D view's first frame looks at a part from where its
 * thumbnail did, and the canvas taking the image's place does not jump.
 */
export type Vec3 = readonly [number, number, number]

export const VIEW_DIR: Vec3 = [0.577_350_27, -0.577_350_27, 0.577_350_27]
export const LIGHT_DIR: Vec3 = [0.408_248_3, -0.408_248_3, 0.816_496_6]

export type Framing = { target: Vec3; position: Vec3; near: number; far: number }

/**
 * Where the camera sits to fit a bounding box: back from the box's centre along the thumbnail's
 * view direction, far enough that the box's bounding sphere fills the vertical field of view. A
 * degenerate box frames a 1 mm sphere rather than dividing by zero. `near` and `far` leave two
 * decades each way, which is room to orbit and zoom without the part clipping.
 */
export function frameBox(min: Vec3, max: Vec3, fovDegrees: number): Framing {
  const target: Vec3 = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2]
  const radius = Math.max(Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2, 0.5)
  const distance = radius / Math.sin((fovDegrees * Math.PI) / 360)
  // Normalised here: `raster.rs`'s constants are rounded to eight places, and an unnormalised
  // direction would put the camera a part in ten million off the distance that fits the box.
  const norm = Math.hypot(...VIEW_DIR)
  const position: Vec3 = [
    target[0] + (VIEW_DIR[0] / norm) * distance,
    target[1] + (VIEW_DIR[1] / norm) * distance,
    target[2] + (VIEW_DIR[2] / norm) * distance,
  ]
  return { target, position, near: distance / 100, far: distance * 100 }
}

let webgl: boolean | undefined

/**
 * Whether this browser can draw the view, asked once. It is asked before the viewer's code is
 * fetched, so a browser that cannot — jsdom, a locked-down machine — never downloads three.js for
 * nothing. `WebGL2RenderingContext` is checked first: jsdom has none, and asking its canvas for a
 * context logs that the method is not implemented.
 */
export function hasWebGL(): boolean {
  if (webgl === undefined) {
    try {
      webgl =
        typeof WebGL2RenderingContext !== 'undefined' &&
        document.createElement('canvas').getContext('webgl2') !== null
    } catch {
      webgl = false
    }
  }
  return webgl
}
