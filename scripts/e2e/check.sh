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
# There are none left — `exit2.py` and `skew-stl.py` went with Phase 6's second exit and the duplicate
# fixtures — and the guard is what keeps an unmatched glob from reading as a failing file. The python
# this rig still runs is embedded in `stack.sh`'s heredocs, which `code_of` below reads as strings and
# nothing parses; that is a gap, and it is the same gap `check.sh` exists inside rather than a new one.
for py in "$HERE"/*.py; do
  [ -e "$py" ] || continue
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
  build_ingest_tree check_worker_sees_ingest library_named upload_file seed_picture \
  tag_borrowed_images untag_borrowed_images \
  cmd_build cmd_up cmd_seed cmd_drive cmd_down cmd_status; do
  if grep -qE "^$fn\(\) \{" "$HERE/stack.sh"; then
    pass=$((pass + 1))
  else
    no "stack.sh no longer defines $fn()"
  fi
done
# And the list is the whole list: a function defined and not named here would be one this check cannot
# protect, which is how the four helpers it was written for came to be deleted in the first place.
defined=$(grep -cE '^[a-z_]+\(\) \{' "$HERE/stack.sh")
if [ "$defined" = 36 ]; then ok "all 36 of stack.sh's functions are defined, and 36 is all there are"
else no "stack.sh defines $defined functions and this check names 36 — add the new one to the list above"; fi

# `skew-stl.py` is gone, and with it the block that stood here.
#
# It asserted the geometry of the two duplicate fixtures — that `--rotate` preserved volume to 1.000000
# and swapped the Y and Z bounding-box sides, and that `--scale 1.15` multiplied volume by 1.520875 —
# because `duplicates` rested on exactly that and `docs/phase-6.md` claimed the descriptor was
# rotation-invariant. R3 deleted `part_shape`, `part_link` and every likeness route, so both fixtures
# were fixtures for nothing and the script had no other caller. What is left in `alike/` is one pair
# with identical bytes, which needs no geometry asserted: `cp` is the whole of it.

echo "== every string the flows match still exists in strings.ts"
# The flows have no test ids to work with — this application has none, and T1 owns none of `web/` — so a
# dozen of them match rendered text. A copy change should therefore break *this*, at commit time, rather
# than a flow twenty minutes into a run against a live stack. Every literal below is one a flow matches; if
# one moves, either the flow follows it or the rename was not meant.
#
# **Ten came out with the pivot and two went in.** Out: `Point to point`, `Show in the 3D view`,
# `Explode` and `Loading the full-detail mesh` with measuring, the section plane and the L2 wait; `Looks
# alike`, `Possible duplicates`, `Identical`, `Near-duplicates`, `Fold into this` and `Not the same` with
# shape similarity. `Read from an analytic CAD entity` is still IN `strings.ts` — rows a STEP file wrote
# before the pivot carry it — but no flow matches it any more, and this list is the flows' list, not the
# file's. In: `View in 3D`, which is the whole of `view-original`, and `Picture of`, which is the alt text
# `pictures` finds a card's own image by.
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
Remove from library
Restore
Scan the ingest folder
Storage
Upload complete —
View in 3D
Picture of
Derived from tessellated
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
LITERALS
  if [ -n "$gone" ]; then
    printf '    these are matched by a flow and are no longer in strings.ts:%s\n' "$gone"
    no "a string a flow depends on has been renamed"
  else ok "all 24 strings the flows match are still in strings.ts"; fi
fi

echo "== the override says nothing check-deploy cannot see"
# `check-deploy` reads deploy/ only, so a build stage, a feature set or a role in this file would be a
# deployment change the gate is blind to.
if grep -nE 'target:|SERVER_FEATURES|LAPIDARY_ROLE|build:|image:' "$HERE/e2e.override.yaml"; then
  no "e2e.override.yaml names a build stage, feature set, role or image"
else ok "e2e.override.yaml carries ports and extra_hosts only"; fi
for svc in app; do
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

echo "== three installations on one lane: AS is a, b or c and nothing else"
# The suffix widened the project pattern to `lapidary-e2e-<lane>[abc]`, so two things have to hold that did
# not need saying before: only those three letters, and the lane itself still a single digit — otherwise
# `LAPIDARY_LANE=4a` would spell a project this rig accepts and nobody owns.
for bad in d aa A 1 ab; do
  code=$(run_stack LAPIDARY_LANE=4 "AS=$bad" LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
    LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
  if [ "$code" = 3 ] && ! docker_touched; then ok "AS=$bad refused, no docker call"; else
    no "AS=$bad: exit $code, docker touched: $(docker_touched && echo yes || echo no)"; fi
done
code=$(run_stack LAPIDARY_LANE=4a LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
  LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
if [ "$code" = 3 ] && ! docker_touched; then ok "a lane spelled \`4a\` refused, no docker call"; else
  no "lane 4a: exit $code — the suffix has widened what counts as a lane"; fi
# And the three that must be allowed through, each on its own ports, or the refusals prove nothing.
for pair in 'a 34000 34080 34081 34082' 'b 34100 34180 34181 34182' 'c 34200 34280 34281 34282'; do
  set -- $pair
  code=$(run_stack LAPIDARY_LANE=4 "AS=$1" LAPIDARY_PORT_WEB=34000 LAPIDARY_PORT_API=34080 \
    LAPIDARY_PORT_WORKER=34081 LAPIDARY_PORT_PEER=34082 -- status)
  if [ "$code" != 3 ] && grep -q "lapidary-e2e-4$1" "$WORK/out.txt" &&
    grep -q "app $2" "$WORK/out.txt"; then
    ok "AS=$1 is project lapidary-e2e-4$1 on $2/$5"
  else
    no "AS=$1: exit $code, and status said $(grep -m1 '^ports' "$WORK/out.txt" || echo nothing)"
  fi
done

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

# The exception must not quietly become the rule. `--min-free` exists because the owner weighed it on
# 2026-10-04 (a build of the four cheap services costs ~0.1 GB and `peer` costs nothing at all), and the
# bargain was that it is passed by hand and recorded — not that the floor moves. R6 reached for the
# override under time pressure and did not ask; a lowered *default* is how that happens without anybody
# noticing at all.
if grep -qE '^ *local min_free=8$' "$HERE/stack.sh"; then ok "the build floor defaults to the protocol's 8 GB"
else no "the build floor's default is not 8 GB — \`--min-free\` is the exception, not a new default"; fi
if grep -q 'built under the 8 GB default on purpose' "$HERE/stack.sh"; then
  ok "a build under the floor says so in its own log"
else no "\`--min-free\` no longer announces itself; an exception nobody can see is not recorded"; fi

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
