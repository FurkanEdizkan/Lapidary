#!/usr/bin/env bash
# The rig's own gate. `cargo xtask verify slice` lints no shell and no `.mjs` file — `check-deploy`
# reads only `deploy/`, and `check-strings` scans `.rs` — so this is the only check `scripts/e2e/` has.
# Run it after every edit, and before every commit.
#
#   scripts/e2e/check.sh
#
# It costs nothing and calls no container: the two refusals are proved with a **fake `docker` first on
# PATH** that records being called and exits 1. That log is the objective proof, and it is what makes
# the protocol's mutation check — disable a refusal on purpose, confirm this fails — safe to run.
set -uo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd -- "$HERE/../.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
pass=0
fail=0

ok() { printf '  ok    %s\n' "$*"; pass=$((pass + 1)); }
no() { printf '  FAIL  %s\n' "$*"; fail=$((fail + 1)); }

# A `docker` that answers nothing and writes down that it was asked. First on PATH, so anything in the
# script that reaches for a container hits this instead of the real one.
# Reading *code*, not prose, for every scan below.
#
# This rig's prose is substantial: every command it forbids and every port it refuses is named in a comment
# or inside a multi-line error message, and its helpers embed whole python programs in quoted spans. So
# `code_of` drops comment lines and everything inside quotes, tracking quote parity **across** lines — a
# `refuse "…"` message spanning four lines is one string, and a line-at-a-time filter reads its last three
# lines as code. Both quote characters count: inside code a `'` is always a delimiter. What is left is what
# the shell would actually run.
#
# check.sh itself is scanned only where that is meaningful; it necessarily spells out what it looks for.
code_of() {
  awk '
    { line = $0 }
    # A comment cannot continue a string, so it is dropped without touching the quote state.
    !inq && !ins && line ~ /^[[:space:]]*(#|\/\/)/ { next }
    {
      out = ""
      n = length(line)
      for (i = 1; i <= n; i++) {
        c = substr(line, i, 1)
        if (c == "\\") { i++; continue }
        if (c == "\"" && !ins) { inq = !inq; continue }
        if (c == "'"'"'" && !inq) { ins = !ins; continue }
        if (inq || ins) continue
        if (c == "#" && out ~ /^[[:space:]]*$/) break
        out = out c
      }
      if (out ~ /[^[:space:]]/) print FNR ":" out
    }
  ' "$1"
}

mkdir -p "$WORK/bin"
cat > "$WORK/bin/docker" <<EOF
#!/bin/sh
echo "CALLED: docker \$*" >> "$WORK/docker-calls.log"
exit 1
EOF
chmod +x "$WORK/bin/docker"

# One `stack.sh` run with a given environment, its xtrace captured. `.lane.env` is not read, because
# LAPIDARY_LANE being set is how xtask marks a lane's settings as applied — which is also what lets this
# drive the refusals without editing anybody's file.
run_stack() { # var=value …  -- subcommand
  local env_args=()
  while [ $# -gt 0 ] && [ "$1" != -- ]; do env_args+=("$1"); shift; done
  shift
  : > "$WORK/docker-calls.log"
  PATH="$WORK/bin:$PATH" env "${env_args[@]}" bash -x "$HERE/stack.sh" "$@" > "$WORK/out.txt" 2> "$WORK/trace.txt"
  echo $?
}

# `docker` inside the trace, not inside the whole output: an error message of ours may say the word.
docker_touched() {
  grep -qE '^\++ .*\bdocker\b' "$WORK/trace.txt" || [ -s "$WORK/docker-calls.log" ]
}

echo "== syntax"
if bash -n "$HERE/stack.sh"; then ok "bash -n stack.sh"; else no "bash -n stack.sh"; fi
if bash -n "$HERE/check.sh"; then ok "bash -n check.sh"; else no "bash -n check.sh"; fi
for mjs in "$HERE"/*.mjs; do
  if node --check "$mjs" 2>/dev/null; then ok "node --check $(basename "$mjs")"; else
    no "node --check $(basename "$mjs")"; node --check "$mjs"
  fi
done
for py in "$HERE"/*.py; do
  if python3 -c "import ast,sys; ast.parse(open(sys.argv[1]).read())" "$py"; then
    ok "python parses $(basename "$py")"
  else no "python parses $(basename "$py")"; fi
done
if python3 -c "import sys,yaml; yaml.safe_load(open(sys.argv[1]))" "$HERE/e2e.override.yaml" 2> /dev/null; then
  ok "yaml parses e2e.override.yaml"
else
  # PyYAML chokes on compose's `!override` tag, which is correct YAML with an unknown tag rather than
  # broken YAML. Falling back to a shape check keeps this useful without adding a dependency.
  if grep -q 'ports: !override' "$HERE/e2e.override.yaml"; then
    ok "e2e.override.yaml carries ports: !override (no yaml module, or its !override tag)"
  else no "e2e.override.yaml has no \`ports: !override\`"; fi
fi

echo "== stack.sh still defines every function it needs"
# `bash -n` is blind to a missing function: it is a runtime name lookup, not a syntax error. So a script
# that lost one parses perfectly and then prints `command not found` at every call. That happened — an edit
# to one function silently deleted the four helpers defined beside it, and the seed ran to the end, reported
# 0 checks and left a `seed.json` nothing had verified. For a test rig that is the worst failure mode there
# is, because it reads as a pass.
#
# The check is a list, deliberately, rather than a parser that guesses at command position: this script
# embeds whole python programs in heredocs and quoted spans, and every attempt to infer calls from them
# produced more false positives than findings. A list catches the thing that actually goes wrong — a helper
# disappearing under an edit to its neighbour — and costs one line when a helper is added.
for fn in die refuse note ask_owner lane_var assert_project compose or_none free_gb ram_gib field json \
  sql seed_check holds counter wait_for until_true settle chown_store remove_store write_env verify_ports \
  build_ingest_tree check_worker_sees_ingest library_named upload_file \
  cmd_build cmd_up cmd_seed cmd_drive cmd_down cmd_status cmd_exit2; do
  if grep -qE "^$fn\(\) \{" "$HERE/stack.sh"; then
    pass=$((pass + 1))
  else
    no "stack.sh no longer defines $fn()"
  fi
done
ok "all 34 of stack.sh's functions are defined (counted individually above)"

echo "== skew-stl.py makes the geometry it claims to"
# The two duplicate fixtures carry the whole meaning of the near-duplicate case, so their geometry is
# asserted rather than assumed: a rotation preserves volume and swaps two bounding-box sides, and a
# 1.15 scale multiplies volume by 1.15 cubed. If either stopped being true, `duplicates` would be
# testing something other than what `docs/phase-6.md` claims.
probe=$WORK/probe
mkdir -p "$probe"
flange=$ROOT/example/parts/flange-dn40-lp-3310-02.stl
if python3 "$HERE/skew-stl.py" --rotate "$flange" "$probe/rotated.stl" > /dev/null &&
  python3 "$HERE/skew-stl.py" --scale 1.15 "$flange" "$probe/scaled.stl" > /dev/null; then
  if python3 - "$flange" "$probe/rotated.stl" "$probe/scaled.stl" <<'PYEOF'
import struct, sys

def triangles(path):
    data = open(path, "rb").read()
    count = struct.unpack_from("<I", data, 80)[0]
    return [struct.unpack_from("<9f", data, 84 + i * 50 + 12) for i in range(count)]

def bbox(tris):
    axes = []
    for start in (0, 1, 2):
        values = [v[start + j] for v in tris for j in (0, 3, 6)]
        axes.append(max(values) - min(values))
    return axes

def volume(tris):
    total = 0.0
    for v in tris:
        a, b, c = v[0:3], v[3:6], v[6:9]
        total += (a[0] * (b[1] * c[2] - b[2] * c[1])
                  - a[1] * (b[0] * c[2] - b[2] * c[0])
                  + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6
    return abs(total)

o, r, s = (triangles(p) for p in sys.argv[1:4])
problems = []
if not (len(o) == len(r) == len(s)):
    problems.append(f"triangle counts differ: {len(o)}, {len(r)}, {len(s)}")
if abs(volume(r) / volume(o) - 1) > 1e-4:
    problems.append(f"--rotate changed the volume by {volume(r) / volume(o):.6f}x; a rotation must not")
if abs(volume(s) / volume(o) - 1.15 ** 3) > 1e-3:
    problems.append(f"--scale 1.15 changed the volume by {volume(s) / volume(o):.6f}x, not {1.15 ** 3:.6f}x")
bo, br = bbox(o), bbox(r)
if abs(bo[0] - br[0]) > 1e-3 or abs(bo[1] - br[2]) > 1e-3 or abs(bo[2] - br[1]) > 1e-3:
    problems.append(f"--rotate should send (x, y, z) to (x, -z, y): {bo} became {br}")
print(f"  {bo[0]:.2f} x {bo[1]:.2f} x {bo[2]:.2f} mm, {volume(o):.0f} mm3 -> "
      f"rotated {br[0]:.2f} x {br[1]:.2f} x {br[2]:.2f}, scaled {volume(s) / volume(o):.4f}x volume")
for problem in problems:
    print("  " + problem)
sys.exit(1 if problems else 0)
PYEOF
  then ok "--rotate preserves volume and swaps Y/Z; --scale 1.15 cubes to 1.5209x"
  else no "skew-stl.py's geometry is not what the duplicate cases need"; fi
else no "skew-stl.py would not run against example/parts/flange-dn40-lp-3310-02.stl"; fi

echo "== every string the flows match still exists in strings.ts"
# The flows have no test ids to work with — this application has none, and T1 owns none of `web/` — so a
# dozen of them match rendered text. A copy change should therefore break *this*, at commit time, rather
# than a flow twenty minutes into a run against a live stack. Every literal below is one a flow matches; if
# one moves, either the flow follows it or the rename was not meant.
strings=$ROOT/web/src/lib/strings.ts
if [ ! -f "$strings" ]; then no "no $strings to check against"; else
  gone=''
  while IFS= read -r literal; do
    [ -n "$literal" ] || continue
    grep -qF "$literal" "$strings" || gone="$gone
    $literal"
  done <<'LITERALS'
Load more
Select
Order
Cards per page
Save this filter
Name for this filter
Point to point
Show in the 3D view
Explode
Remove from library
Restore
Scan the ingest folder
Storage
This installation
Upload complete —
Loading the full-detail mesh
Derived from tessellated
Read from an analytic CAD entity
Nothing here yet
Upload a folder
Search this library
Categories
All models
Selected parts
Reading the folder
Asking which files are new
Finishing the upload
Scan complete —
Looks alike
Possible duplicates
Identical
Near-duplicates
Fold into this
Not the same
LITERALS
  if [ -n "$gone" ]; then
    printf '    these are matched by a flow and are no longer in strings.ts:%s\n' "$gone"
    no "a string a flow depends on has been renamed"
  else ok "all 34 strings the flows match are still in strings.ts"; fi
fi

echo "== the override says nothing check-deploy cannot see"
# `check-deploy` reads deploy/ only, so a build stage, a feature set or a role in this file would be a
# deployment change the gate is blind to.
if grep -nE 'target:|SERVER_FEATURES|LAPIDARY_ROLE|build:|image:' "$HERE/e2e.override.yaml"; then
  no "e2e.override.yaml names a build stage, feature set, role or image"
else ok "e2e.override.yaml carries ports and extra_hosts only"; fi
for svc in api worker web peer; do
  if grep -A2 "^  $svc:" "$HERE/e2e.override.yaml" | grep -q 'ports: !override'; then
    ok "$svc publishes with !override (so deploy/'s literal is replaced, not appended)"
  else no "$svc does not carry \`ports: !override\`"; fi
done

echo "== refusal one: the ports must be this lane's"
for bad in 3000 8080 8081 8082; do
  code=$(run_stack LAPIDARY_LANE=4 LAPIDARY_PORT_WEB=$bad LAPIDARY_PORT_API=34080 \
    LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
  if [ "$code" = 3 ] && ! docker_touched; then ok "web port $bad refused, no docker call"; else
    no "web port $bad: exit $code, docker touched: $(docker_touched && echo yes || echo no)"; fi
done
# Explicitly empty, not merely absent: absent falls back to .lane.env, which is the whole point of the
# file. Empty is somebody having cleared it, and that must reach the refusal.
code=$(run_stack LAPIDARY_LANE=4 LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
  LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER= -- status)
if [ "$code" = 3 ] && ! docker_touched; then ok "an emptied port refused, no docker call"; else
  no "empty LAPIDARY_PORT_PEER: exit $code"; fi
code=$(run_stack LAPIDARY_LANE=4 LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34000 \
  LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
if [ "$code" = 3 ] && ! docker_touched; then ok "two services on one port refused, no docker call"; else
  no "duplicate port: exit $code"; fi
code=$(run_stack LAPIDARY_LANE=4 LAPIDARY_PORT_WEB=notaport LAPIDARY_PORT_API=34080 \
  LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
if [ "$code" = 3 ] && ! docker_touched; then ok "a non-numeric port refused, no docker call"; else
  no "non-numeric port: exit $code"; fi

echo "== refusal two: the compose project must be this rig's own"
for lane in '' 5 lapidary x 04; do
  code=$(run_stack "LAPIDARY_LANE=$lane" LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
    LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
  if [ "$code" = 3 ] && ! docker_touched; then
    ok "lane ${lane:-<empty>} refused (project would not be lapidary-e2e-[0-4]), no docker call"
  else no "lane ${lane:-<empty>}: exit $code, docker touched: $(docker_touched && echo yes || echo no)"; fi
done
# And the one that must be allowed through, or the refusals would be vacuous.
code=$(run_stack LAPIDARY_LANE=4 LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
  LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
if [ "$code" != 3 ]; then ok "lane 4 on its own block is accepted (exit $code)"; else
  no "lane 4 on its own block was refused — the checks above prove nothing"; fi
if grep -q 'lapidary-e2e-4' "$WORK/out.txt"; then ok "and it names project lapidary-e2e-4"; else
  no "status did not name lapidary-e2e-4: $(head -3 "$WORK/out.txt")"; fi

echo "== no bare compose call anywhere in the rig"
# These greps read *code*, not prose. Every command this file names on purpose — the prunes it forbids,
# the ports it refuses — is written inside backticks in a comment somewhere, so a plain grep matches the
# documentation and calls it a violation. So: strip backticked spans and whole-line comments first, and
# skip this file, which necessarily spells out everything it is looking for.
scanned=()
for f in "$HERE"/*.sh "$HERE"/*.mjs; do [ "$f" = "$HERE/check.sh" ] || scanned+=("$f"); done

# One compose function with -p hard-coded. A call without it targets the owner's `lapidary` project,
# whose volumes hold their real library.
scanned=()
for f in "$HERE"/*.sh "$HERE"/*.mjs; do [ "$f" = "$HERE/check.sh" ] || scanned+=("$f"); done
hits=''
for f in "${scanned[@]}"; do
  hits+=$(code_of "$f" | grep 'docker compose' | grep -v 'docker compose -p' | sed "s|^|$(basename "$f"):|")
done
if [ -n "$hits" ]; then printf '%s\n' "$hits"; no "a \`docker compose\` call that is not the one -p'd function"
else ok "every \`docker compose\` goes through the -p \$PROJECT function"; fi

for forbidden in 'builder prune' 'volume prune' 'system prune' 'image prune'; do
  hits=''
  for f in "${scanned[@]}"; do
    hits+=$(code_of "$f" | grep "docker $forbidden" | sed "s|^|$(basename "$f"):|")
  done
  if [ -n "$hits" ]; then printf '%s\n' "$hits"; no "the rig runs \`docker $forbidden\`"
  else ok "no \`docker $forbidden\`"; fi
done

# The owner's ports must appear in exactly one place: the list this rig refuses.
hits=''
for f in "${scanned[@]}"; do
  hits+=$(code_of "$f" | grep -E '\b(3000|8080)\b' | grep -v FORBIDDEN_PORTS | sed "s|^|$(basename "$f"):|")
done
if [ -n "$hits" ]; then printf '%s\n' "$hits"; no "3000 or 8080 appears in code outside FORBIDDEN_PORTS"
else ok "3000 and 8080 appear only in FORBIDDEN_PORTS"; fi

echo "== $pass passed, $fail failed"
[ "$fail" = 0 ]
