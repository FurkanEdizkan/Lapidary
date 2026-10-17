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
 * context logs that the method is not implemented. The probe's context is let go at once, so asking
 * holds none of the few a browser allows.
 */
export function hasWebGL(): boolean {
  if (webgl === undefined) {
    try {
      const probe =
        typeof WebGL2RenderingContext === 'undefined' ? null : document.createElement('canvas').getContext('webgl2')
      webgl = probe !== null
      probe?.getExtension('WEBGL_lose_context')?.loseContext()
    } catch {
      webgl = false
    }
  }
  return webgl
}

/**
 * The grid's spacing for a part `span` millimetres across its floor (A12): 1, 2 or 5 × 10ⁿ, the step that
 * puts about ten squares across the part, so a 40 mm bracket sits on 5 mm squares and a 700 mm terrain
 * board on 50 mm ones.
 */
export function gridStep(span: number): number {
  if (!Number.isFinite(span) || span <= 0) return 1
  const rough = span / 10
  const power = 10 ** Math.floor(Math.log10(rough))
  const scaled = rough / power
  const nice = scaled < 1.5 ? 1 : scaled < 3.5 ? 2 : scaled < 7.5 ? 5 : 10
  return nice * power
}

/** A length in millimetres as the view writes it: three significant figures, never an exponent. */
export function millimetres(value: number): string {
  const size = Math.abs(value)
  const places = size >= 100 ? 1 : size >= 10 ? 2 : size >= 1 ? 2 : 3
  return value.toFixed(places)
}

/** Two picks' distance and their difference on each axis, second minus first. */
export function between(a: Vec3, b: Vec3): { distance: number; delta: Vec3 } {
  const delta: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
  return { distance: Math.hypot(...delta), delta }
}
