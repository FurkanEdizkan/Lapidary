/**
 * The viewer's arithmetic, kept out of the component so it can be tested without a GPU.
 *
 * The directions are `raster.rs`'s, so the 3D view's first frame looks at a part from where its
 * thumbnail did, and the canvas taking the image's place does not jump.
 */
export type Vec3 = readonly [number, number, number]

export const VIEW_DIR: Vec3 = [0.577_350_27, -0.577_350_27, 0.577_350_27]
export const LIGHT_DIR: Vec3 = [0.408_248_3, -0.408_248_3, 0.816_496_6]

export type Framing = { target: Vec3; position: Vec3; halfHeight: number; near: number; far: number }

/**
 * An orthographic framing of a bounding box, the way `raster.rs` draws the thumbnail: no
 * perspective, so a cylinder does not taper toward the lens and the canvas matches the poster it
 * replaces. The camera looks along the thumbnail's direction from outside the box's bounding
 * sphere, and the view is tall enough to hold that sphere with a little margin, which also holds
 * it at every angle an orbit reaches. `near` and `far` bracket the sphere, and orbiting keeps the
 * distance, so the part never clips. A degenerate box frames a 1 mm sphere rather than nothing.
 */
export function frameBox(min: Vec3, max: Vec3): Framing {
  const target: Vec3 = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2]
  const radius = Math.max(Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2, 0.5)
  const distance = radius * 4
  // Normalised here: `raster.rs`'s constants are rounded to eight places.
  const norm = Math.hypot(...VIEW_DIR)
  const position: Vec3 = [
    target[0] + (VIEW_DIR[0] / norm) * distance,
    target[1] + (VIEW_DIR[1] / norm) * distance,
    target[2] + (VIEW_DIR[2] / norm) * distance,
  ]
  return { target, position, halfHeight: radius * 1.05, near: radius, far: distance + radius * 3 }
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
