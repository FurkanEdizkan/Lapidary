#!/usr/bin/env bash
# Three installations of this application on one lane's port block, sharing with each other, and a capture
# of every sharing screen at the moment it exists.
#
#   scripts/e2e/group.sh                 # tear down, bring three up, run the scenario, capture, tear down
#   scripts/e2e/group.sh --keep          # leave the three up afterwards
#   scripts/e2e/group.sh --min-ram 3     # `up`'s RAM floor, for three installations nobody seeds
#   scripts/e2e/group.sh dist [<dir>]    # serve a freshly built web/dist from all three web containers
#   scripts/e2e/group.sh down            # tear all three down
#
# WHY THIS EXISTS. Sharing takes two installations and the introductions take three, and a dozen of these
# screens exist only while three are in one particular state: an introduction disappears when it is
# accepted, "as somebody read it" holds only while the folder's owner is away, a queue position needs one
# pull waiting behind another, and "no folder in common" appears only after somebody is taken off. So the
# captures are taken inside the scenario, at each of those moments, and not in one pass at the end.
#
# It is goal 9's `target/docker-check/group/run-group.sh` — three compose projects, ports 28080/28180/28280
# — moved onto the committed rig and given screens: every installation is `stack.sh` with `AS=a|b|c`, so it
# inherits both refusals, the resolved-port verification, the reverse chown before teardown and the
# assertion that nothing is left owned by another user. Nothing here runs `docker compose`.
#
# It never builds an image. The three installations borrow the lane's five, which `stack.sh up` tags under
# each project's name — so what these screens show is whatever `stack.sh build` last built, and `dist`
# above is how a web change is seen without building anything at all.
set -uo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
STAMP=$(date +%Y%m%d-%H%M%S)
OUT=$ROOT/target/e2e/group/$STAMP
SHOTS=$OUT/shots

die() { printf 'STOP: %s\n' "$*" >&2; exit 1; }
say() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*"; }

# The lane's block, read the way stack.sh reads it. Each installation sits 100 ports above the last.
lane_var() {
  [ -f "$ROOT/.lane.env" ] || return 0
  sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*\"\{0,1\}\([^\"]*\)\"\{0,1\}[[:space:]]*\$/\1/p" \
    "$ROOT/.lane.env" | tail -1
}
PORT_WEB=${LAPIDARY_PORT_WEB:-$(lane_var LAPIDARY_PORT_WEB)}
PORT_API=${LAPIDARY_PORT_API:-$(lane_var LAPIDARY_PORT_API)}
PORT_PEER=${LAPIDARY_PORT_PEER:-$(lane_var LAPIDARY_PORT_PEER)}
LANE=${LAPIDARY_LANE:-$(lane_var LAPIDARY_LANE)}
[ -n "${PORT_WEB:-}" ] && [ -n "${PORT_API:-}" ] && [ -n "${PORT_PEER:-}" ] && [ -n "${LANE:-}" ] ||
  die "this lane's LAPIDARY_PORT_* and LAPIDARY_LANE are not in $ROOT/.lane.env. scripts/claim-goal.sh
  writes them; stack.sh refuses without them and so does this."

offset() { case "$1" in a) echo 0 ;; b) echo 100 ;; c) echo 200 ;; *) die "no installation $1" ;; esac; }
api() { echo "http://127.0.0.1:$((PORT_API + $(offset "$1")))"; }
web_of() { echo "http://127.0.0.1:$((PORT_WEB + $(offset "$1")))"; }
# Resolved inside the peer's own namespace, where 127.0.0.1 is the peer itself; the override gives it the
# host alias, which is how two installations on one machine reach each other.
peer_addr() { echo "host.docker.internal:$((PORT_PEER + $(offset "$1")))"; }
# One installation's stack.sh. `AS` is the only thing that differs between the three.
inst() { local as=$1; shift; AS=$as "$ROOT/scripts/e2e/stack.sh" "$@"; }

field() { python3 -c "import json,sys; value=json.load(sys.stdin); print($1)"; }
json() { curl -sf -X "$1" -H 'content-type: application/json' ${3:+-d "$3"} "$2"; }
# Until `test` reads true of the json at `url`, or the time is up.
until_true() { # url, python test on `value`, what, seconds
  local url=$1 test=$2 what=$3 secs=${4:-120} began
  began=$(date +%s)
  while [ $(($(date +%s) - began)) -lt "$secs" ]; do
    [ "$(curl -sf "$url" 2>/dev/null | field "$test" 2>/dev/null)" = True ] &&
      { say "  $what after $(($(date +%s) - began)) s"; return 0; }
    sleep 2
  done
  say "STOP: $what did not happen within ${secs} s ($url)"
  return 1
}
settle() { # api, library, batch
  local line
  while true; do
    line=$(curl -sf "$1/api/libraries/$2/jobs/$3" |
      field '" ".join(f"{k}={value[k]}" for k in ["total","pending","running","ingested","skipped","failedTotal"])' 2>/dev/null)
    [[ "$line" == *" pending=0 running=0 "* ]] && { echo "$line"; return; }
    sleep 2
  done
}

shots=0
shot_failures=0
# One capture, named for what it is meant to show. A failed capture is recorded and the scenario carries
# on: a screen review that abandons the run at the first missing selector brings back nothing at all.
shoot() { # who, tag, --view … extra args
  local who=$1 tag=$2
  shift 2
  say "  shot $tag (installation $who)"
  if node "$ROOT/scripts/e2e/sharing-shots.mjs" --base "$(web_of "$who")" --out "$SHOTS" --tag "$tag" "$@"; then
    # Two widths unless the call named its own; this is a count for the log, not an assertion.
    case " $* " in *' --widths '*) shots=$((shots + 1)) ;; *) shots=$((shots + 2)) ;; esac
  else
    shot_failures=$((shot_failures + 1))
  fi
}

# ---------------------------------------------------------------------------------------------------
# A's ingest tree: two categories, real parts, small enough that the worker's share of the machine is
# seconds. Contents cleared rather than the directory removed — `rm -rf` on a bind-mount source replaces
# the inode and the running container keeps the old one, which reads exactly like a broken scan route.
build_a_tree() {
  local ingest=$ROOT/target/e2e/${LANE}a/ingest
  mkdir -p "$ingest/Terrain" "$ingest/Fasteners"
  find "$ingest" -mindepth 2 -type f -delete
  # Mostly fixtures, and deliberately not `example/parts`: every installation's worker seeds those six into
  # its own default library on first start, so a folder made of them arrives at the other two already held,
  # every card reads *In your library*, and there is nothing left to press Download on. One example part is
  # kept for exactly the opposite reason — it is the card that honestly says the file is already here.
  cp "$ROOT/fixtures/bracket-lp-1042-03.stl" "$ingest/Terrain/"
  cp "$ROOT/fixtures/planetary-carrier-lp-3480-02.3mf" "$ingest/Terrain/"
  cp "$ROOT/example/parts/flange-dn40-lp-3310-02.stl" "$ingest/Terrain/"
  cp "$ROOT/fixtures/spacer-lp-2001-00.stl" "$ingest/Fasteners/"
  cp "$ROOT/fixtures/step/ball-knob-d20-lp-9020-00.step" "$ingest/Fasteners/"
  chmod -R a+rX "$ingest"
  say "  A's ingest tree: $(find "$ingest" -type f | wc -l) parts in $(find "$ingest" -mindepth 1 -type d | wc -l) categories"
}

cmd_down() {
  for who in c b a; do
    say "down $who"
    inst "$who" down 2>&1 | sed 's/^/  /'
  done
}

# A freshly built bundle, served by all three. Caddy serves `/srv` inside the web image, so a `docker cp`
# there is the whole trick: `npm --prefix web run build` takes no compile lock and no image is rebuilt.
cmd_dist() {
  local dist=${1:-$ROOT/web/dist}
  [ -d "$dist" ] || die "$dist does not exist. Run \`npm --prefix web run build\` first."
  [ -f "$dist/index.html" ] || die "$dist has no index.html; that is not a built bundle."
  for who in a b c; do
    local container=lapidary-e2e-$LANE$who-web-1
    docker cp "$dist/." "$container:/srv/" > /dev/null ||
      die "could not copy the bundle into $container. Is that installation up?"
    say "  $container now serves $(find "$dist" -type f | wc -l) files from $dist"
  done
}

case "${1:-}" in
  down) cmd_down; exit $? ;;
  dist) shift; cmd_dist "$@"; exit $? ;;
esac

KEEP=0
DIST=''
UP_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) KEEP=1; shift ;;
    --min-ram) UP_ARGS+=(--min-ram "${2:?--min-ram takes a number of GiB}"); shift 2 ;;
    # A bundle to serve instead of the one in the image, copied in before a single capture is taken.
    --dist) DIST=${2:-$ROOT/web/dist}; shift 2 ;;
    *) die "usage: group.sh [--keep] [--min-ram <gib>] [--dist <dir>] | dist [<dir>] | down" ;;
  esac
done

mkdir -p "$SHOTS"
say "== three installations, captures in ${OUT#"$ROOT"/}"

# From nothing, every time: `down -v` takes the peer key volume with it, so a device id and a pairing
# cannot survive a teardown — and a run that started from somebody else's leftovers would not be the run
# whose screens are being reviewed.
cmd_down
build_a_tree
for who in a b c; do
  say "up $who"
  inst "$who" up "${UP_ARGS[@]}" 2>&1 | sed 's/^/  /' ||
    die "installation $who would not come up. \`AS=$who scripts/e2e/stack.sh status\` says why."
done

[ -n "$DIST" ] && cmd_dist "$DIST"

A=$(api a) B=$(api b) C=$(api c)
A_ID=$(curl -sf "$A/api/sharing/identity" | field 'value["deviceId"]')
B_ID=$(curl -sf "$B/api/sharing/identity" | field 'value["deviceId"]')
C_ID=$(curl -sf "$C/api/sharing/identity" | field 'value["deviceId"]')
[ -n "$A_ID" ] && [ -n "$B_ID" ] && [ -n "$C_ID" ] || die "one of the three has no device id."
json PUT "$A/api/sharing/identity" '{"name":"Ayşe’s workshop"}' > /dev/null
json PUT "$B/api/sharing/identity" '{"name":"Burak’s bench"}' > /dev/null
json PUT "$C/api/sharing/identity" '{"name":"Cem’s studio"}' > /dev/null
say "  A ${A_ID:0:11}…  B ${B_ID:0:11}…  C ${C_ID:0:11}…"

say "== A ingests its workshop and pairs with both"
A_LIB=$(json POST "$A/api/libraries" '{"name":"Workshop","mode":"hobby"}' | field 'value["id"]')
BATCH=$(json POST "$A/api/libraries/$A_LIB/scan" | field 'value["batchId"]')
say "  scanned: $(settle "$A" "$A_LIB" "$BATCH")"
until_true "$A/api/libraries/$A_LIB/folders" '"Terrain" in [f["name"] for f in value]' \
  "Terrain is a category" 120 || die "A's scan made no Terrain category."
TERRAIN=$(curl -sf "$A/api/libraries/$A_LIB/folders" | field '[f["id"] for f in value if f["name"] == "Terrain"][0]')
json POST "$A/api/sharing/peers" "{\"deviceId\":\"$B_ID\",\"address\":\"$(peer_addr b)\"}" > /dev/null
json POST "$A/api/sharing/peers" "{\"deviceId\":\"$C_ID\",\"address\":\"$(peer_addr c)\"}" > /dev/null
json POST "$B/api/sharing/peers" "{\"deviceId\":\"$A_ID\",\"address\":\"$(peer_addr a)\"}" > /dev/null
json POST "$C/api/sharing/peers" "{\"deviceId\":\"$A_ID\",\"address\":\"$(peer_addr a)\"}" > /dev/null

say "== A shares Terrain with B and C"
SHARE=$(json POST "$A/api/libraries/$A_LIB/shares" \
  "{\"folderId\":\"$TERRAIN\",\"memberDeviceIds\":[\"$B_ID\",\"$C_ID\"]}" | field 'value["id"]')
[ -n "$SHARE" ] || die "A could not share Terrain."
until_true "$B/api/sharing/peers/$A_ID/shares" 'len(value) == 1 and value[0]["syncedAt"] is not None' \
  "B read Terrain" 180 || die "B never read Terrain."
until_true "$C/api/sharing/peers/$A_ID/shares" 'len(value) == 1 and value[0]["syncedAt"] is not None' \
  "C read Terrain" 180 || die "C never read Terrain."
# Who it goes to, with a list somebody picked rather than "everyone paired".
shoot a a-own-shares --view sharing

say "== B and C are introduced to each other, and C answers"
until_true "$C/api/sharing/introductions" 'len(value) == 1' "C is offered an introduction to B" 180 ||
  die "C was never offered an introduction."
# Before the answer: accepting is what makes the card disappear, so this is the only moment it exists.
shoot c c-introductions --view sharing
C_INTRO=$(curl -sf "$C/api/sharing/introductions" | field 'value[0]["shareId"]')
json POST "$C/api/sharing/introductions/$C_INTRO/$B_ID" '{"accept":true}' > /dev/null
until_true "$B/api/sharing/introductions" 'len(value) == 1' "B is offered an introduction to C" 180 || true
B_INTRO=$(curl -sf "$B/api/sharing/introductions" | field 'value[0]["shareId"]' 2> /dev/null)
[ -n "${B_INTRO:-}" ] && json POST "$B/api/sharing/introductions/$B_INTRO/$C_ID" '{"accept":true}' > /dev/null
# B is on C's list now, introduced by A, and the two of them share no folder at all.
until_true "$C/api/sharing/peers" 'len(value) == 2' "B is on C's list" 120 || true
shoot c c-people-introduced --view sharing

say "== A shares Fasteners through the dialog, asking first, with C unticked"
# Through the real member picker, not curl: this is the screen under review, and pressing its own Share is
# what proves the picker sends what it shows.
shoot a a-share-dialog --view share-dialog --library "$A_LIB" --folder Fasteners \
  --drop 'Cem’s studio' --ask --confirm
until_true "$A/api/shares" 'len([s for s in value if s["asksFirst"]]) == 1' \
  "Fasteners is shared, asking first" 60 || say "  (the dialog did not leave an ask-first share)"
# And the same dialog for a folder already shared, which is a different screen: no picker, because this
# dialog does not know who the folder goes to and everyone ticked here would widen a list somebody picked.
shoot a a-share-dialog-again --view share-dialog --library "$A_LIB" --folder Fasteners
FASTENERS=$(curl -sf "$A/api/shares" | field '([s["id"] for s in value if s["asksFirst"]] or [""])[0]')

say "== B asks for Fasteners, and queues a part of Terrain behind it"
B_LIB=$(curl -sf "$B/api/libraries" | field 'value[0]["id"]')
# A new share reaches B on the next hello round, which is fifteen seconds away — and a pull started before
# it arrives is a pull of nothing, which is how the first run of this scenario photographed an empty
# "Nobody has asked to pull" and called it the requests screen.
until_true "$B/api/sharing/peers/$A_ID/shares" '"Fasteners" in [s["name"] for s in value]' \
  "Fasteners reached B" 120 || say "  (B was never offered Fasteners)"
B_FASTENERS=$(curl -sf "$B/api/sharing/peers/$A_ID/shares" |
  field '([s["id"] for s in value if s["name"] == "Fasteners"] or [""])[0]')
B_TERRAIN=$(curl -sf "$B/api/sharing/peers/$A_ID/shares" |
  field '([s["id"] for s in value if s["name"] == "Terrain"] or [""])[0]')
if [ -n "$B_FASTENERS" ]; then
  json POST "$B/api/sharing/shares/$B_FASTENERS/pulls" "{\"libraryId\":\"$B_LIB\"}" > /dev/null
  until_true "$B/api/sharing/shares/$B_FASTENERS/pull" 'value["state"] == "waiting"' \
    "B's pull of Fasteners waits for A's answer" 120 || true
  # The ask is what A's page is for. Without it there is a request row to photograph and no request.
  until_true "$A/api/shares/requests" 'len(value) > 0' "B's ask reached A" 120 ||
    say "  (nobody is asking on A; the requests screen will be its empty state)"
fi
PART=$(curl -sf "$B/api/sharing/shares/$B_TERRAIN/parts?limit=50" |
  field '([p["sourcePath"] for p in value["parts"] if not p["held"]] or [""])[0]')
[ -n "$PART" ] && json POST "$B/api/sharing/shares/$B_TERRAIN/pulls" \
  "{\"libraryId\":\"$B_LIB\",\"sourcePath\":\"$PART\"}" > /dev/null
say "  B pulls ${PART:-nothing}; Terrain's pull is $(curl -sf "$B/api/sharing/shares/$B_TERRAIN/pull" |
  field '"%s, %s behind" % (value["state"], value["queuedBehind"])' 2> /dev/null || echo unknown)"
# One pull runs at a time, so this one says where it is in the queue.
shoot b b-queue-position --view shared-folder --share "$B_TERRAIN"
# And on A: somebody asking, with the answer still to give — and the switch that turns asking off.
shoot a a-requests --view sharing
shoot a a-members-dialog --view members-dialog

say "== A switches Fasteners off asking first, and back, from its own row"
# The owner-side gap this goal exists to close, driven through the switch itself rather than asserted in a
# test: the page had no way to change how an existing share is shared, and the web helper dropped
# `asksFirst: false`, so switching back was unreachable from anywhere in the application.
shoot a a-ask-first-off --view ask-first --folder Fasteners --widths 1440
until_true "$A/api/shares" 'not [s["asksFirst"] for s in value if s["name"] == "Fasteners"][0]' \
  "Fasteners is open again, said by the api" 60 || say "  (the switch did not reach the api)"
say "  A's requests now: $(curl -sf "$A/api/shares/requests" | field 'len(value)') (this api image predates \
the mode filter, so a stale ask here is expected; the db test is what proves it drops)"
shoot a a-ask-first-on --view ask-first --folder Fasteners --ask --widths 1440
until_true "$A/api/shares" '[s["asksFirst"] for s in value if s["name"] == "Fasteners"][0]' \
  "and asking first again" 60 || say "  (the switch did not reach the api)"

say "== A answers, and B's pulls run"
[ -n "$FASTENERS" ] && json PUT "$A/api/shares/$FASTENERS/grants/$B_ID" '{"granted":true}' > /dev/null
until_true "$B/api/sharing/shares/$B_TERRAIN/pull" 'value["state"] == "done"' "B's part arrived" 180 ||
  say "  Terrain's pull is $(curl -sf "$B/api/sharing/shares/$B_TERRAIN/pull" |
    field '"%s (%s of %s files): %s" % (value["state"], value["filesDone"], value["filesTotal"], value["error"])' 2> /dev/null)"
until_true "$B/api/sharing/shares/$B_TERRAIN/parts?limit=50" 'any(p["held"] for p in value["parts"])' \
  "one of Terrain's parts is in B's library" 120 || true
# Held and not held side by side: "In your library" where the file is here, Download where it is not.
shoot b b-shared-folder --view shared-folder --share "$B_TERRAIN"

say "== C sleeps, A's Terrain changes, and C takes it from B"
inst c peer stop > /dev/null 2>&1 || say "  (could not stop C's peer)"
cp "$ROOT/fixtures/idler-bracket-lp-2210-01.obj" "$ROOT/target/e2e/${LANE}a/ingest/Terrain/"
chmod -R a+rX "$ROOT/target/e2e/${LANE}a/ingest"
BATCH=$(json POST "$A/api/libraries/$A_LIB/scan" | field 'value["batchId"]')
say "  scanned again: $(settle "$A" "$A_LIB" "$BATCH")"
# By name, not value[0]: B is offered two of A's folders by now, and the first in the list is whichever
# the api returns first — a count asserted against the wrong folder waits three minutes and then lies.
until_true "$B/api/sharing/peers/$A_ID/shares" \
  '[s["partCount"] for s in value if s["name"] == "Terrain"] == [4]' "B read the change" 180 || true
inst a peer stop > /dev/null 2>&1 || say "  (could not stop A's peer)"
inst c peer start > /dev/null 2>&1 || say "  (could not start C's peer)"
C_TERRAIN=$(curl -sf "$C/api/sharing/peers/$A_ID/shares" |
  field '([s["id"] for s in value if s["name"] == "Terrain"] or [""])[0]')
until_true "$C/api/sharing/shares/$C_TERRAIN" 'value["readFrom"] is not None' \
  "C read Terrain through B while A was away" 240 || say "  (no relay; the page will say last read instead)"
shoot c c-relayed --view shared-folder --share "$C_TERRAIN"

say "== A comes back and takes C off Terrain"
inst a peer start > /dev/null 2>&1 || say "  (could not start A's peer)"
until_true "$A/api/sharing/peers" 'len(value) == 2' "A's peer role answers again" 180 || true
json PUT "$A/api/shares/$SHARE/members" "{\"deviceIds\":[\"$B_ID\"]}" > /dev/null
until_true "$C/api/sharing/peers/$A_ID/shares" 'len(value) == 0' "Terrain left C's list" 300 || true
# Both still paired, nothing in common with either of them, and what C pulled stays.
shoot c c-nothing-in-common --view sharing

say "== $shots captures in ${SHOTS#"$ROOT"/}, $shot_failures capture(s) failed"
python3 - "$OUT/group.json" <<EOF
import json, subprocess, sys
json.dump({
    "stamp": "$STAMP",
    "lane": "$LANE",
    "shots": $shots,
    "shotFailures": $shot_failures,
    "installations": {"a": "$(web_of a)", "b": "$(web_of b)", "c": "$(web_of c)"},
    "devices": {"a": "$A_ID", "b": "$B_ID", "c": "$C_ID"},
    "shares": {"terrain": "$SHARE", "fasteners": "$FASTENERS"},
    "sha": "$(git -C "$ROOT" rev-parse HEAD)",
    "images": "lapidary-e2e-$LANE-*",
}, open(sys.argv[1], "w"), indent=2)
EOF
if [ "$KEEP" = 1 ]; then
  say "--keep: the three are still up on $(web_of a), $(web_of b), $(web_of c)"
  say "  tear them down with \`scripts/e2e/group.sh down\`"
else
  cmd_down
fi
[ "$shot_failures" = 0 ]
