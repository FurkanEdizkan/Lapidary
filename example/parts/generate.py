#!/usr/bin/env python3
"""Generate the example part library: six real, parametric machine parts as binary STL.

Stdlib only. Two primitives cover every part here:

  extrude(outer, holes, z0, z1) -- a prismatic solid from a 2D outline with circular
      holes. The end caps are triangulated by ear clipping after each hole is bridged
      into the outer loop, so the result is a single closed surface, not a union.
  revolve(profile, segments)    -- a solid of revolution from a closed (r, z) profile.

Both produce watertight meshes; `check_watertight` proves it (every edge traversed
exactly twice, once in each direction) and `volume` reports the enclosed volume so the
numbers can be sanity-checked against the drawing.
"""

import math
import struct
import sys
from pathlib import Path

EPS = 1e-9


# ---------------------------------------------------------------- 2D outlines


def circle(cx, cy, r, n, phase=0.0, ccw=True):
    pts = [
        (cx + r * math.cos(phase + 2 * math.pi * i / n),
         cy + r * math.sin(phase + 2 * math.pi * i / n))
        for i in range(n)
    ]
    return pts if ccw else pts[::-1]


def rounded_rect(w, h, r, n_corner):
    """CCW outline of a w x h rectangle centred on the origin, corners radiused r."""
    hw, hh = w / 2 - r, h / 2 - r
    pts = []
    for cx, cy, a0 in ((hw, hh, 0.0), (-hw, hh, math.pi / 2),
                       (-hw, -hh, math.pi), (hw, -hh, 3 * math.pi / 2)):
        for i in range(n_corner + 1):
            a = a0 + (math.pi / 2) * i / n_corner
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return dedupe(pts)


def hexagon(across_flats, phase=0.0):
    r = across_flats / 2 / math.cos(math.pi / 6)
    return [
        (r * math.cos(phase + math.pi / 3 * i), r * math.sin(phase + math.pi / 3 * i))
        for i in range(6)
    ]


def spur_gear(module, teeth, pressure_angle_deg, phase, flank_pts=5):
    """CCW involute spur gear outline. ISO proportions: addendum 1m, dedendum 1.25m."""
    a = math.radians(pressure_angle_deg)
    r_pitch = module * teeth / 2
    r_base = r_pitch * math.cos(a)
    r_tip = r_pitch + module
    r_root = r_pitch - 1.25 * module
    r_start = max(r_base, r_root) + 1e-6

    def inv(r):
        # Polar angle of the involute point at radius r, measured from where the
        # involute leaves the base circle.
        return math.sqrt(max((r / r_base) ** 2 - 1.0, 0.0)) - math.acos(min(r_base / r, 1.0))

    # Half tooth angle at the pitch circle: circular tooth thickness pi*m/2.
    half_pitch = math.pi / (2 * teeth)
    inv_pitch = inv(r_pitch)

    def offset(r):
        return half_pitch + inv_pitch - inv(r)

    pitch_angle = 2 * math.pi / teeth
    pts = []
    for k in range(teeth):
        c = phase + k * pitch_angle
        radii = [r_start + (r_tip - r_start) * i / (flank_pts - 1) for i in range(flank_pts)]
        # Root fillet stand-in: a radial run from the root circle out to where the
        # involute begins. Real gears have a trochoidal fillet; this is a straight
        # relief, which is what a hobbed gear approximates at this tooth count anyway.
        if r_start > r_root + 1e-6:
            pts.append(polar(r_root, c - offset(r_start)))
        for r in radii:                       # up the trailing flank
            pts.append(polar(r, c - offset(r)))
        for r in reversed(radii):             # down the leading flank
            pts.append(polar(r, c + offset(r)))
        if r_start > r_root + 1e-6:
            pts.append(polar(r_root, c + offset(r_start)))
        # Root arc across to the next tooth.
        a0 = c + offset(r_start) if r_start > r_root + 1e-6 else c + offset(r_root)
        a1 = c + pitch_angle - (offset(r_start) if r_start > r_root + 1e-6 else offset(r_root))
        for i in range(1, 3):
            pts.append(polar(r_root, a0 + (a1 - a0) * i / 3))
    return dedupe(pts)


def polar(r, a):
    return (r * math.cos(a), r * math.sin(a))


def dedupe(pts):
    out = []
    for p in pts:
        if not out or dist2(out[-1], p) > 1e-14:
            out.append(p)
    if len(out) > 1 and dist2(out[0], out[-1]) <= 1e-14:
        out.pop()
    return out


def dist2(a, b):
    return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2


def signed_area(poly):
    s = 0.0
    for i, (x0, y0) in enumerate(poly):
        x1, y1 = poly[(i + 1) % len(poly)]
        s += x0 * y1 - x1 * y0
    return s / 2


# ------------------------------------------------------- polygon triangulation


def point_in_ring(p, ring):
    """Even-odd crossing test against one closed ring."""
    inside = False
    x, y = p
    for i, (x0, y0) in enumerate(ring):
        x1, y1 = ring[(i + 1) % len(ring)]
        if (y0 > y) != (y1 > y):
            t = x0 + (y - y0) * (x1 - x0) / (y1 - y0)
            if t > x:
                inside = not inside
    return inside


def segments_cross(p1, p2, p3, p4):
    """Strict crossing: shared endpoints and touching do not count."""
    def orient(a, b, c):
        v = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        return 0 if abs(v) < 1e-12 else (1 if v > 0 else -1)

    d1, d2 = orient(p3, p4, p1), orient(p3, p4, p2)
    d3, d4 = orient(p1, p2, p3), orient(p1, p2, p4)
    return d1 * d2 < 0 and d3 * d4 < 0


def bridge_hole(outer, hole, other_rings):
    """Splice `hole` (CW) into `outer` (CCW) with a doubled bridge edge.

    Picks the nearest outer vertex visible from the hole's rightmost vertex, by brute
    force. Ray casting is the textbook choice, but it degenerates exactly where circular
    holes sit level with each other -- which is the normal case for a bolt circle -- so
    this tests candidate segments directly instead.
    """
    m = max(range(len(hole)), key=lambda i: hole[i][0])
    origin = hole[m]
    rings = [outer, hole] + other_rings

    best = None
    for i, v in enumerate(outer):
        mid = ((origin[0] + v[0]) / 2, (origin[1] + v[1]) / 2)
        if not point_in_ring(mid, outer):
            continue
        if any(point_in_ring(mid, r) for r in [hole] + other_rings):
            continue
        blocked = False
        for ring in rings:
            for j, a in enumerate(ring):
                b = ring[(j + 1) % len(ring)]
                if segments_cross(origin, v, a, b):
                    blocked = True
                    break
            if blocked:
                break
        if blocked:
            continue
        d = dist2(origin, v)
        if best is None or d < best[0]:
            best = (d, i)
    if best is None:
        raise RuntimeError("no visible bridge vertex for hole at %r" % (origin,))

    i = best[1]
    rotated = hole[m:] + hole[:m]
    return outer[: i + 1] + rotated + [rotated[0]] + outer[i:]


def ear_clip(poly):
    """Ear clipping over a CCW, weakly simple polygon. Returns (a, b, c) point triples."""
    idx = list(range(len(poly)))
    tris = []
    guard = 0
    while len(idx) > 3:
        guard += 1
        if guard > 4 * len(poly) + 64:
            raise RuntimeError("ear clipping stalled with %d vertices left" % len(idx))
        clipped = False
        for k in range(len(idx)):
            ia, ib, ic = idx[k - 1], idx[k], idx[(k + 1) % len(idx)]
            a, b, c = poly[ia], poly[ib], poly[ic]
            cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
            if cross <= EPS:                       # reflex or degenerate
                continue
            if any(
                point_strictly_in_triangle(poly[i], a, b, c)
                for i in idx
                if i not in (ia, ib, ic)
            ):
                continue
            tris.append((a, b, c))
            idx.pop(k)
            clipped = True
            break
        if not clipped:
            raise RuntimeError("no ear found with %d vertices left" % len(idx))
    a, b, c = (poly[i] for i in idx)
    tris.append((a, b, c))
    return tris


def point_strictly_in_triangle(p, a, b, c):
    def side(u, v):
        return (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])

    s1, s2, s3 = side(a, b), side(b, c), side(c, a)
    return s1 > EPS and s2 > EPS and s3 > EPS


# --------------------------------------------------------------- solid builders


def extrude(outer, holes, z0, z1):
    """A prismatic solid: CCW `outer` outline, CCW `holes` (reversed here), z0..z1."""
    assert signed_area(outer) > 0, "outer loop must be CCW"
    merged = list(outer)
    cw_holes = [h[::-1] for h in holes]           # CW inside a CCW outer loop
    order = sorted(range(len(cw_holes)), key=lambda i: -max(p[0] for p in cw_holes[i]))
    pending = [cw_holes[i] for i in order]
    for n, hole in enumerate(pending):
        merged = bridge_hole(merged, hole, pending[n + 1:])
    cap = ear_clip(merged)

    tris = []
    for a, b, c in cap:                            # top, normals +z
        tris.append(((a[0], a[1], z1), (b[0], b[1], z1), (c[0], c[1], z1)))
    for a, b, c in cap:                            # bottom, wound the other way
        tris.append(((c[0], c[1], z0), (b[0], b[1], z0), (a[0], a[1], z0)))
    for loop, outward in [(outer, True)] + [(h, False) for h in holes]:
        for i, p in enumerate(loop):
            q = loop[(i + 1) % len(loop)]
            if outward:
                tris.append(((p[0], p[1], z0), (q[0], q[1], z0), (q[0], q[1], z1)))
                tris.append(((p[0], p[1], z0), (q[0], q[1], z1), (p[0], p[1], z1)))
            else:
                tris.append(((q[0], q[1], z0), (p[0], p[1], z0), (p[0], p[1], z1)))
                tris.append(((q[0], q[1], z0), (p[0], p[1], z1), (q[0], q[1], z1)))
    return tris


def rotate_x90(tris):
    """(x, y, z) -> (x, -z, y): stands an XY-extruded profile up so its features face +Z."""
    return [tuple((p[0], -p[2], p[1]) for p in t) for t in tris]


def revolve(profile, segments):
    """A solid of revolution about the Z axis from a closed (r, z) profile.

    The profile is given CCW in the r-z half plane with r > 0 throughout, so the swept
    surface closes on itself and no end caps are needed.
    """
    assert all(r > 0 for r, _ in profile), "profile must not touch the axis"
    assert signed_area(profile) > 0, "profile must be CCW in r-z"
    tris = []
    for i in range(segments):
        a0 = 2 * math.pi * i / segments
        a1 = 2 * math.pi * (i + 1) / segments
        for j, (r0, z0) in enumerate(profile):
            r1, z1 = profile[(j + 1) % len(profile)]
            p00 = (r0 * math.cos(a0), r0 * math.sin(a0), z0)
            p01 = (r0 * math.cos(a1), r0 * math.sin(a1), z0)
            p10 = (r1 * math.cos(a0), r1 * math.sin(a0), z1)
            p11 = (r1 * math.cos(a1), r1 * math.sin(a1), z1)
            # Wound so the normal points away from the axis: sweeping CCW about +Z
            # while walking a CCW (r, z) profile gives an inward normal otherwise.
            tris.append((p00, p11, p10))
            tris.append((p00, p01, p11))
    return tris


# --------------------------------------------------------------- checks and IO


def check_watertight(tris):
    edges = {}
    for t in tris:
        for i in range(3):
            a = quant(t[i])
            b = quant(t[(i + 1) % 3])
            edges[(a, b)] = edges.get((a, b), 0) + 1
    bad = [e for e, n in edges.items() if n != 1]
    unpaired = [e for e in edges if (e[1], e[0]) not in edges]
    return len(bad), len(unpaired)


def quant(p):
    return tuple(round(c, 6) for c in p)


def volume(tris):
    v = 0.0
    for a, b, c in tris:
        v += (
            a[0] * (b[1] * c[2] - c[1] * b[2])
            - a[1] * (b[0] * c[2] - c[0] * b[2])
            + a[2] * (b[0] * c[1] - c[0] * b[1])
        ) / 6.0
    return v


def write_stl(path, tris, header):
    buf = bytearray()
    buf += header.encode("ascii")[:80].ljust(80, b"\0")
    buf += struct.pack("<I", len(tris))
    for a, b, c in tris:
        u = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
        v = (c[0] - a[0], c[1] - a[1], c[2] - a[2])
        n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        ln = math.sqrt(sum(x * x for x in n)) or 1.0
        buf += struct.pack("<3f", *(x / ln for x in n))
        for p in (a, b, c):
            buf += struct.pack("<3f", *p)
        buf += struct.pack("<H", 0)
    path.write_bytes(bytes(buf))
    return len(tris), len(buf)


# ------------------------------------------------------------------- the parts


def part_mounting_plate():
    """80 x 60 x 6 plate, R6 corners, four M5 clearance holes, 25 mm centre bore."""
    outer = rounded_rect(80, 60, 6, 6)
    holes = [circle(sx * 30, sy * 20, 2.75, 20, phase=0.07)
             for sx in (1, -1) for sy in (1, -1)]
    holes.append(circle(0, 0, 12.5, 40, phase=0.07))
    return extrude(outer, holes, 0.0, 6.0)


def part_flange():
    """DN40 PN16 plate flange: 150 OD, 110 PCD, 4 x 18 bolt holes, 43 bore, 16 thick."""
    outer = circle(0, 0, 75, 64, phase=0.05)
    holes = [circle(55 * math.cos(a), 55 * math.sin(a), 9.0, 20, phase=0.07)
             for a in (math.pi / 4, 3 * math.pi / 4, 5 * math.pi / 4, 7 * math.pi / 4)]
    holes.append(circle(0, 0, 21.5, 48, phase=0.05))
    return extrude(outer, holes, 0.0, 16.0)


def part_hex_spacer():
    """M4 hex spacer, 7 mm across flats, 20 mm long, 4.2 mm through bore."""
    outer = hexagon(7.0, phase=0.09)
    return extrude(outer, [circle(0, 0, 2.1, 24, phase=0.05)], 0.0, 20.0)


def part_vee_block():
    """60 x 40 x 50 vee block with a 90 degree vee, 16 mm deep."""
    outer = [
        (-30.0, -20.0), (30.0, -20.0), (30.0, 20.0),
        (14.0, 20.0), (0.0, 6.0), (-14.0, 20.0), (-30.0, 20.0),
    ]
    holes = [circle(sx * 20, -10.0, 3.3, 20, phase=0.07) for sx in (1, -1)]
    # Extruded in XY, then stood up so the vee opens upward the way the block is used.
    return rotate_x90(extrude(outer, holes, 0.0, 50.0))


def part_spur_gear():
    """Module 2, 20 teeth, 20 degree pressure angle, 8 mm face, 10 mm bore."""
    outer = spur_gear(2.0, 20, 20.0, phase=0.087)
    return extrude(outer, [circle(0, 0, 5.0, 28, phase=0.05)], 0.0, 8.0)


def part_idler_pulley():
    """24 mm OD vee-groove idler, 6 mm bore, 9 mm wide, 90 degree groove."""
    profile = [
        (3.0, 0.0), (12.0, 0.0), (12.0, 1.5), (7.5, 4.5), (12.0, 7.5),
        (12.0, 9.0), (3.0, 9.0),
    ]
    return revolve(profile, 56)


PARTS = [
    ("mounting-plate-lp-1180-01", "lapidary mounting plate LP-1180-01", part_mounting_plate),
    ("flange-dn40-lp-3310-02", "lapidary DN40 flange LP-3310-02", part_flange),
    ("hex-spacer-m4x20-lp-2145-01", "lapidary hex spacer LP-2145-01", part_hex_spacer),
    ("vee-block-lp-3072-02", "lapidary vee block LP-3072-02", part_vee_block),
    ("spur-gear-m2-20t-lp-5140-00", "lapidary spur gear LP-5140-00", part_spur_gear),
    ("idler-pulley-lp-4820-00", "lapidary idler pulley LP-4820-00", part_idler_pulley),
]


def main(out_dir):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    failures = 0
    for stem, header, build in PARTS:
        tris = build()
        dup, unpaired = check_watertight(tris)
        vol = volume(tris)
        n, size = write_stl(out / (stem + ".stl"), tris, header)
        status = "ok"
        if dup or unpaired or vol <= 0:
            status = "NOT WATERTIGHT (dup=%d unpaired=%d)" % (dup, unpaired)
            failures += 1
        print("%-32s %5d tris %8d B  %10.1f mm3  %s" % (stem, n, size, vol, status))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "example/parts"))
