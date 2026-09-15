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

/** A run of the index buffer to draw, in indices. */
export type Range = { start: number; count: number }

/**
 * What to draw when some of an assembly's parts are hidden. `parts` is how many triangles each
 * placed part has, in the tree's depth-first order (`glb.rs` writes it as `extras.parts`), and each
 * part's triangles are one run of the index buffer. Neighbouring visible parts merge into one
 * range, so a view with nothing hidden draws one.
 */
export function visibleRanges(parts: readonly number[], hidden: ReadonlySet<number>): Range[] {
  const ranges: Range[] = []
  let at = 0
  parts.forEach((triangles, part) => {
    const count = triangles * 3
    if (count > 0 && !hidden.has(part)) {
      const last = ranges.at(-1)
      if (last !== undefined && last.start + last.count === at) last.count += count
      else ranges.push({ start: at, count })
    }
    at += count
  })
  return ranges
}

/** The axes a section can cut across. */
export const AXES = ['x', 'y', 'z'] as const
export type Axis = (typeof AXES)[number]

/** A cut across the part: along which axis, where across the part's box from 0 to 1, and which side stays. */
export type Section = { axis: Axis; at: number; flip: boolean }

/** A plane as three's `Plane` holds one: a point `p` lies on it where `normal · p + constant` is 0. */
export type PlaneLike = { normal: Vec3; constant: number }

/**
 * The plane a section cuts along, across a box. three leaves out whatever is on a plane's negative
 * side, so the normal points at what stays: down the axis unless flipped, keeping what is below the
 * cut, and up it when flipped.
 */
export function sectionPlane(axis: Axis, at: number, flip: boolean, min: Vec3, max: Vec3): PlaneLike {
  const index = axis === 'x' ? 0 : axis === 'y' ? 1 : 2
  const position = min[index] + (max[index] - min[index]) * at
  const sign = flip ? 1 : -1
  const normal: Vec3 = [index === 0 ? sign : 0, index === 1 ? sign : 0, index === 2 ? sign : 0]
  return { normal, constant: -sign * position }
}

/**
 * Where a section's cap lies: on the cut plane, over the middle of the part's box, facing along the
 * plane's normal, and wide enough to cover any cross-section the box can hold whatever the axis. The
 * stencil decides which of it is drawn; this only has to be large enough and in the right place.
 */
export function capPlacement(
  plane: PlaneLike,
  min: Vec3,
  max: Vec3,
): { position: Vec3; normal: Vec3; size: number } {
  const centre: Vec3 = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2]
  const [nx, ny, nz] = plane.normal
  const distance = nx * centre[0] + ny * centre[1] + nz * centre[2] + plane.constant
  const position: Vec3 = [centre[0] - nx * distance, centre[1] - ny * distance, centre[2] - nz * distance]
  const size = 2 * Math.max(max[0] - min[0], max[1] - min[1], max[2] - min[2], 1e-3)
  return { position, normal: plane.normal, size }
}

/**
 * Whether a point is on the side of a section's plane that is drawn, give or take a micrometre so a
 * face lying in the plane still counts. three's raycaster ignores clipping, so a pick asks this.
 */
export function kept(plane: PlaneLike, point: Vec3): boolean {
  const [x, y, z] = plane.normal
  return x * point[0] + y * point[1] + z * point[2] + plane.constant >= -1e-3
}
