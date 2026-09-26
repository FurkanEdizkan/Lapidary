#!/usr/bin/env python3
"""The arithmetic behind Phase 6's exit 2, from `part_shape`'s own columns.

Called by `stack.sh exit2`, which hands it the two profiles through the environment. It exists as its own
file because the interesting part is a distance and two thresholds, and doing that in shell would be
unreadable — not because anything here is clever.

The thresholds are `crates/lapidary-core/src/shape.rs`'s, restated with their source so a drift between
this and the application is visible rather than silent:

    distance          Euclidean over the 35-float descriptor
    near-duplicate    distance <= NEAR_DUPLICATE_DISTANCE (0.04) **and** |ln(size_a/size_b)| <= ln(1.02)
    size band         ln(1.02), about 0.019803 — the +-2% band the candidate index returns

`phase-6.md` shows no score in the interface on purpose: "a figure like 0.038 invites a judgement nobody
can calibrate". Reading it here is legitimate precisely because this is a measurement for the record and
not a thing a person is asked to judge.
"""

import json
import math
import os
import sys

# crates/lapidary-core/src/shape.rs. Restated, not imported — if these ever disagree with the Rust, the
# report says so below rather than quietly using the wrong number.
NEAR_DUPLICATE_DISTANCE = 0.04
SIZE_BAND = math.log(1.02)
DESCRIPTOR_LEN = 35


def main():
    name = os.environ["PAIR_NAME"]
    degrees = os.environ["TURN_DEGREES"]
    turned_id = os.environ["TURNED_ID"]
    likeness = json.loads(os.environ["LIKENESS"] or "{}")

    profiles = {}
    for line in os.environ["ROWS"].splitlines():
        if not line.strip():
            continue
        path, size, descriptor = line.split("\t")
        values = [float(v) for v in descriptor.split(",") if v != ""]
        profiles[path] = {"size_mm": float(size), "descriptor": values}

    original = f"pair/{name}.stl"
    turned = f"pair/{name}-turn{degrees}.stl"
    report = {
        "part": name,
        "turnedDegrees": float(degrees),
        "settledSeconds": int(os.environ["SETTLED"]),
        "unprofiledInLibrary": os.environ["UNPROFILED"],
        "profileJobsLeftBehind": os.environ["STRAY"],
        "profiles": sorted(profiles),
    }

    if original not in profiles or turned not in profiles:
        report["verdict"] = "no profile"
        report["why"] = (
            "one of the pair has no row in part_shape, so nothing was compared. Profiling is meant to run "
            "in line inside the ingest job, so this is worth reporting."
        )
        finish(report, name)
        return 1

    a, b = profiles[original], profiles[turned]
    for path, p in ((original, a), (turned, b)):
        if len(p["descriptor"]) != DESCRIPTOR_LEN:
            report["verdict"] = "malformed descriptor"
            report["why"] = f"{path} has {len(p['descriptor'])} floats, not {DESCRIPTOR_LEN}"
            finish(report, name)
            return 1

    distance = math.sqrt(sum((x - y) ** 2 for x, y in zip(a["descriptor"], b["descriptor"])))
    band = abs(math.log(a["size_mm"] / b["size_mm"]))
    shape_ok = distance <= NEAR_DUPLICATE_DISTANCE
    size_ok = band <= SIZE_BAND
    # A part outside the band is never even a candidate — the index returns the +-2% neighbours — so
    # "outside the band" and "too far in shape" are different failures and the report keeps them apart.
    listed = {
        kind: [p["id"] for p in (likeness.get(kind) or [])]
        for kind in ("identical", "nearDuplicates", "similar", "variants")
    }
    where = [kind for kind, ids in listed.items() if turned_id in ids] or ["nowhere"]

    report.update(
        {
            "sizeMm": {"known": round(a["size_mm"], 4), "turned": round(b["size_mm"], 4)},
            "descriptorDistance": round(distance, 6),
            "nearDuplicateThreshold": NEAR_DUPLICATE_DISTANCE,
            "distanceHeadroom": round(NEAR_DUPLICATE_DISTANCE - distance, 6),
            "sizeBandPosition": round(band, 6),
            "sizeBandLimit": round(SIZE_BAND, 6),
            "insideSizeBand": size_ok,
            "withinDistance": shape_ok,
            "isNearDuplicateByTheRule": shape_ok and size_ok,
            "profiledFlag": likeness.get("profiled"),
            "surfacedAs": where,
            "verdict": "near-duplicate" if turned_id in listed["nearDuplicates"] else "not a near-duplicate",
        }
    )
    if report["isNearDuplicateByTheRule"] != (turned_id in listed["nearDuplicates"]):
        report["disagreement"] = (
            "the rule and the route disagree: the thresholds in this file may have drifted from "
            "crates/lapidary-core/src/shape.rs, or the candidate index never offered the pair."
        )
    finish(report, name)
    return 0


def finish(report, name):
    with open(os.path.join(os.environ["WORK"], f"exit2-{name}.json"), "w") as f:
        json.dump(report, f, indent=2)
    print(f"  profiled: {report.get('profiledFlag')}   unprofiled in library: {report['unprofiledInLibrary']}"
          f"   profile jobs left behind: {report['profileJobsLeftBehind']}")
    if "descriptorDistance" in report:
        print(f"  size {report['sizeMm']['known']} mm vs {report['sizeMm']['turned']} mm")
        print(f"  descriptor distance {report['descriptorDistance']} against a {NEAR_DUPLICATE_DISTANCE} "
              f"threshold  ->  {'within' if report['withinDistance'] else 'PAST IT'}"
              f" (headroom {report['distanceHeadroom']})")
        print(f"  size band position {report['sizeBandPosition']} against {report['sizeBandLimit']}"
              f"  ->  {'inside' if report['insideSizeBand'] else 'OUTSIDE, so never a candidate'}")
        print(f"  the route lists it under: {', '.join(report['surfacedAs'])}")
    print(f"  VERDICT: {report['verdict']}")
    if "disagreement" in report:
        print(f"  ! {report['disagreement']}")


if __name__ == "__main__":
    sys.exit(main())
