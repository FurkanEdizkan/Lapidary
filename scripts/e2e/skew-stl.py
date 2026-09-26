#!/usr/bin/env python3
"""Write a deliberately-altered copy of a binary STL, for the duplicate cases the rig seeds.

Two kinds, and the difference between them is the whole point:

  --rotate          the same solid stood on a different axis. `example/parts/generate.py`'s
                    `rotate_x90` is imported rather than reimplemented, because that exact
                    transform is the invariance `docs/phase-6.md` claims near-duplicate
                    detection has: different bytes, different hash, same shape, and a
                    descriptor distance inside the ln(1.02) band.
  --scale <factor>  the same shape at a different size. 1.15 is 15 % larger, which is far
                    outside that band by design: it is the "similar but not near" case, and a
                    detector that called it a near-duplicate would be wrong.

`write_stl` is imported too, so the bytes are laid out by the same writer that made the six
example parts — a second STL writer here could differ in normals or padding and turn a seeding
bug into a detection finding.

    scripts/e2e/skew-stl.py --rotate       <in.stl> <out.stl>
    scripts/e2e/skew-stl.py --scale 1.15   <in.stl> <out.stl>
"""

import argparse
import importlib.util
import struct
import sys
from pathlib import Path

# Importing `example/parts/generate.py` by path would otherwise leave a `__pycache__/` beside it, in a
# directory this goal does not own and which would turn up untracked in somebody's `git status`.
sys.dont_write_bytecode = True

REPO = Path(__file__).resolve().parents[2]


def load_generate():
    """`example/parts/generate.py` by path — it is a script beside its parts, not a package.

    It guards `main` behind `if __name__ == "__main__"`, so importing it runs no generation
    and writes no files; that guard is what makes these two functions reusable at all.
    """
    path = REPO / "example" / "parts" / "generate.py"
    spec = importlib.util.spec_from_file_location("lapidary_example_generate", path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"Could not load {path} — is this running from inside the repository?")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for name in ("rotate_x90", "write_stl"):
        if not hasattr(module, name):
            raise SystemExit(
                f"{path} no longer defines {name}(). skew-stl.py imports it on purpose rather "
                "than copying it; update both together."
            )
    return module


def read_binary_stl(path: Path):
    """The triangles of a binary STL, as `write_stl` wants them: a list of 3-vertex tuples.

    Normals are dropped, not read: `write_stl` recomputes each facet's normal from its winding,
    so carrying the old one across a rotation would be the one thing guaranteed to be wrong.
    """
    data = path.read_bytes()
    if len(data) < 84:
        raise SystemExit(f"{path} is {len(data)} bytes — too short to be a binary STL.")
    if data[:5].lower() == b"solid" and b"facet normal" in data[:512]:
        raise SystemExit(
            f"{path} looks like an ASCII STL. This rig's fixtures are binary; re-export it, or "
            "point at one of example/parts/*.stl."
        )
    (count,) = struct.unpack_from("<I", data, 80)
    expected = 84 + count * 50
    if len(data) != expected:
        raise SystemExit(
            f"{path} claims {count} triangles, which needs {expected} bytes, but the file is "
            f"{len(data)}. It may be truncated."
        )
    tris = []
    for i in range(count):
        base = 84 + i * 50 + 12  # past this facet's normal, which is recomputed on write
        vertices = struct.unpack_from("<9f", data, base)
        tris.append((vertices[0:3], vertices[3:6], vertices[6:9]))
    return tris


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--rotate", action="store_true", help="stand the solid up: (x, y, z) -> (x, -z, y)")
    mode.add_argument("--scale", type=float, metavar="FACTOR", help="scale every vertex about the origin")
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args(argv)

    if args.scale is not None and not 0.01 <= args.scale <= 100:
        raise SystemExit(f"--scale {args.scale} is not a plausible factor; 0.01 to 100 is the range.")
    if not args.source.is_file():
        raise SystemExit(f"No such file: {args.source}")

    generate = load_generate()
    tris = read_binary_stl(args.source)
    if args.rotate:
        out = generate.rotate_x90(tris)
        what = "rotated 90 degrees about X"
    else:
        k = args.scale
        out = [tuple(tuple(c * k for c in p) for p in t) for t in tris]
        what = f"scaled by {k:g}"

    args.destination.parent.mkdir(parents=True, exist_ok=True)
    # Not a header beginning "solid": a binary STL whose first bytes spell that word is read as
    # ASCII by some parsers, and the point of this file is to be ingested, not argued with.
    n, size = generate.write_stl(args.destination, out, f"Lapidary e2e: {args.source.name} {what}")
    print(f"{args.destination}: {n} triangles, {size} bytes, {what}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
