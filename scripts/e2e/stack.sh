#!/usr/bin/env bash
# The whole application, up and driven: one compose stack on this lane's port block, a library worth
# testing seeded into it, and a real browser over the real api.
#
#   scripts/e2e/stack.sh build            # deploy/'s one image, one service at a time, disk-guarded
#   scripts/e2e/stack.sh up               # bring it up and wait until every role answers
#   scripts/e2e/stack.sh seed             # the ingest tree and the four libraries
#   scripts/e2e/stack.sh drive [--compare <report.json>] [--only <flow,flow>]
#   scripts/e2e/stack.sh down [--keep|--purge]   # reverse the chown, remove the project and the store
#   scripts/e2e/stack.sh status
#
# `AS=a|b|c` brings up a second or third installation beside the first, which sharing between
# installations needed until S3 removed it (2026-10-16). It suffixes the compose project, the work
# directory and the images, and shifts this lane's ports by 100 per letter, so the three sit inside one
# lane's block and cannot reach another lane's.
#
#   AS=b scripts/e2e/stack.sh up            # the second installation, on this lane's block + 100
#
# Everything heavy belongs inside `cargo xtask heavy -- …`, which takes the machine-wide compile lock,
# so a build here cannot coincide with another lane's `cargo build`:
#
#   cargo xtask heavy -- scripts/e2e/stack.sh build
#
# ---------------------------------------------------------------------------------------------------
# WHY THE TWO REFUSALS AT THE TOP EXIST
#
# `deploy/compose.yaml` declares `name: lapidary`, and the owner's `lapidary_lapidary-db` and
# `lapidary_lapidary-uploads` volumes hold their real library on this machine. A `docker compose` call
# against that file **without `-p`** targets those volumes, and one `down -v` would destroy them. Its
# host ports are literals too — web 3000, api 8080 — and 3000 carries the owner's browser storage, so
# a stack of ours on it rewrites their own grid preferences.
#
# So: exactly one `compose` function, with `-p` hard-coded and the project name asserted inside it; the
# four ports read from the lane's `.lane.env` and refused if any of them is unset or names one of
# `deploy/`'s literals. Both checks run before anything else happens — no `mkdir`, no `docker` — which
# is what makes them testable without a container (see `scripts/e2e/README` in this file's own tests,
# and the goal file's stage 0).
#
# Never `docker builder prune`, never `docker volume prune`, never `docker system prune`, and never
# remove an image this project did not tag. The pivot took the OCCT stage out, so a cold worker build is
# about 155 s rather than 916 s — but a prune here would still reach cache this rig did not create.
set -uo pipefail

# ---------------------------------------------------------------------------------------------------
# Where we are, and what the lane gave us.

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
LANE_FILE=$ROOT/.lane.env

die() { printf 'STOP: %s\n' "$*" >&2; exit 1; }
refuse() { printf 'REFUSED: %s\n' "$*" >&2; exit 3; }
note() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*"; }
ask_owner() { printf 'STOP, ask the owner: %s\n' "$*" >&2; exit 4; }

# One key out of `.lane.env`, by the same grammar `xtask/src/lane.rs` parses: `KEY=VALUE`, `#` comments,
# and a value that may be wrapped in one pair of quotes. Parsed rather than sourced — this file is
# written by a script but read by one that must not execute whatever ends up in it.
lane_var() {
  [ -f "$LANE_FILE" ] || return 0
  sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*\"\{0,1\}\([^\"]*\)\"\{0,1\}[[:space:]]*\$/\1/p" \
    "$LANE_FILE" | tail -1
}

# `cargo xtask heavy` exports the lane's settings into the environment before running us, and
# `LAPIDARY_LANE` being set is how xtask itself marks them as applied. Honour that: take what is in the
# environment, and fall back to the file only for what is missing. It is also what lets the refusal
# tests below drive this script with variables instead of editing a lane's file.
#
# The fallback fires only for a variable that is genuinely **unset**. Present-and-empty means somebody
# meant to clear it, and it must reach the refusals below rather than being quietly refilled from the
# file — `deploy/compose.yaml`'s worker knobs treat empty the same way, and for the same reason: a value
# that was silently not what the caller said is the failure nobody diagnoses.
LANE_KEYS=(LAPIDARY_LANE LAPIDARY_PORT_WEB LAPIDARY_PORT_API LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER)
for key in "${LANE_KEYS[@]}"; do
  if [ -z "${!key+set}" ]; then
    value=$(lane_var "$key")
    [ -n "$value" ] && export "$key=$value"
  fi
done

# ---------------------------------------------------------------------------------------------------
# ONE LANE, THREE INSTALLATIONS.
#
# A lane owns one port block and this rig brought up one stack in it. Sharing took two installations and
# the introductions three, so `AS` names which one this call is. It suffixes the compose project (and
# so the volumes, the containers and the images), it suffixes the work directory (and so the store, the
# ingest tree and the env file), and it shifts the four ports by 100 per letter. Everything else — both
# refusals, the port verification, the reverse chown, the teardown assertions — applies unchanged, which
# is the whole reason for extending this script rather than writing a fourth throwaway harness.
#
# Unset, nothing below changes and this is the single stack it has always been.
AS=${AS:-}
case "$AS" in
  '' | a | b | c) ;;
  *) refuse "AS is \`$AS\`, and this rig knows a, b and c — three installations inside one lane's port
  block. Anything else would name a compose project nobody can predict, and a port block that may be
  another lane's." ;;
esac
# Shifted here, before the refusals, so what they check is what compose will publish. A port that is not a
# number is left exactly as it is for the refusal below to name: shifting it would turn `notaport` into 100
# and blame the wrong thing.
if [ -n "$AS" ]; then
  case "$AS" in a) AS_OFFSET=0 ;; b) AS_OFFSET=100 ;; c) AS_OFFSET=200 ;; esac
  for key in LAPIDARY_PORT_WEB LAPIDARY_PORT_API LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER; do
    case "${!key:-}" in
      '' | *[!0-9]*) continue ;;
      *) export "$key=$((${!key} + AS_OFFSET))" ;;
    esac
  done
fi

# ---------------------------------------------------------------------------------------------------
# REFUSAL ONE: the ports must be this lane's, and must not be deploy/'s literals.

# 3000 and 8080 are `deploy/compose.yaml`'s own published ports — the owner's install, and the origin
# whose localStorage holds their grid preferences. 8081 and 8082 are the worker's and the peer's.
FORBIDDEN_PORTS='3000 8080 8081 8082'
PORT_KEYS=(LAPIDARY_PORT_WEB LAPIDARY_PORT_API LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER)
seen_ports=''
for key in "${PORT_KEYS[@]}"; do
  port=${!key:-}
  [ -n "$port" ] || refuse "$key is not set. \`scripts/claim-goal.sh <goal> <lane>\` writes the four
  LAPIDARY_PORT_* values into the lane's .lane.env; the lead's own checkout (lane 0) needs its block —
  30000/30080/30081/30082 — appended to $LANE_FILE by hand. This rig will not guess a port."
  case "$port" in
    '' | *[!0-9]*) refuse "$key is \`$port\`, which is not a port number." ;;
  esac
  [ "$port" -ge 1024 ] && [ "$port" -le 65535 ] ||
    refuse "$key is $port; an unprivileged published port is 1024-65535."
  for bad in $FORBIDDEN_PORTS; do
    [ "$port" = "$bad" ] && refuse "$key is $port, which is one of deploy/compose.yaml's own published
  ports (web 3000, api 8080, worker 8081, peer 8082). Binding it would collide with the owner's
  install, and on 3000 it would rewrite the grid preferences in their browser's storage for that
  origin. Use this lane's block from .lane.env."
  done
  case " $seen_ports " in
    *" $port "*) refuse "$key is $port, which another LAPIDARY_PORT_* already claims. The four
  services publish four different host ports." ;;
  esac
  seen_ports="$seen_ports $port"
done

# ---------------------------------------------------------------------------------------------------
# REFUSAL TWO: the compose project must be this rig's own, and no other.

# The lane itself, checked before the name is built out of it. Without this the suffix would have widened
# the project pattern: `LAPIDARY_LANE=4a` with no AS spells `lapidary-e2e-4a`, which the pattern below now
# allows and which is nobody's lane. A lane is one digit, 0 to 4, and `scripts/claim-goal.sh` writes it.
[[ ${LAPIDARY_LANE:-} =~ ^[0-4]$ ]] || refuse "LAPIDARY_LANE is \`${LAPIDARY_LANE:-}\`, and a lane is a
  single digit 0-4. It comes from $LANE_FILE, which scripts/claim-goal.sh writes; the lead's own checkout
  is lane 0. Which of three installations this is goes in AS, not in the lane."
PROJECT=lapidary-e2e-${LAPIDARY_LANE:-}$AS
[[ $PROJECT =~ ^lapidary-e2e-[0-4][abc]?$ ]] || refuse "the compose project would be \`$PROJECT\`, and this
  rig only ever speaks to \`lapidary-e2e-<lane 0-4>[a|b|c]\`. LAPIDARY_LANE is \`${LAPIDARY_LANE:-}\`; it comes
  from $LANE_FILE, which scripts/claim-goal.sh writes. Anything else risks naming the owner's own
  \`lapidary\` project, whose volumes hold their real library."

# Asserted again inside `compose` — so immediately before every `down`, every `up` and every `build` —
# because the cost of this one being wrong is not a failed test, it is the owner's library.
assert_project() {
  [[ $PROJECT =~ ^lapidary-e2e-[0-4][abc]?$ ]] ||
    die "compose project \`$PROJECT\` is not lapidary-e2e-<lane>[a|b|c]. Refusing to run docker compose."
}

# ---------------------------------------------------------------------------------------------------
# Paths, urls, and the one compose function.

LANE=$LAPIDARY_LANE
# One directory per installation: its own store, ingest tree and env file, since each is a whole
# installation and they must not share a database password or a blob store.
WORK=$ROOT/target/e2e/$LANE$AS
ENVFILE=$WORK/e2e.env
STORE=$WORK/store
INGEST=$WORK/ingest
# Since A2 the models folder is one directory: the person's folders, and Lapidary's own under it —
# uploads/, imports/, examples/ and _removed/, the only ones the app writes, and the only ones it owns.
INBOX=$INGEST
OWN_DIRS=(uploads imports examples _removed)
RUNS=$WORK/runs
# The rig's session (A1): written by `sign_in_rig`, sent by every `curl` below and handed to the flows.
COOKIES=$WORK/cookies.txt
RIG_USER=rig
RIG_PASSWORD=rig-e2e-password
# One process serves the app, its API and the worker since S1, so all three names are one origin. Kept
# as three names because the flows and the seed say which of them they mean.
WEB=http://127.0.0.1:$LAPIDARY_PORT_WEB
API=$WEB
WORKER_URL=$WEB
# The corpus lives in the **main checkout's** `target/`, not this worktree's.
#
# `target/` is per-worktree and gitignored — that is the whole point of the lane rules — so the 1,000
# symlinks an earlier goal made exist in exactly one place on this machine, and a lane looking for them
# beside its own build directory finds nothing. The main checkout is found the way
# `scripts/claim-goal.sh` finds it: the parent of the common git directory, which is the same answer from
# any worktree. `LAPIDARY_CORPUS` overrides it for a machine that keeps the corpus somewhere else.
MAIN_ROOT=$(cd -- "$(git -C "$ROOT" rev-parse --git-common-dir)/.." 2>/dev/null && pwd) || MAIN_ROOT=$ROOT
CORPUS=${LAPIDARY_CORPUS:-$MAIN_ROOT/target/sharing-check/corpus-1000/STL Files}
CORPUS_TSV=${LAPIDARY_CORPUS_TSV:-$MAIN_ROOT/target/sharing-check/corpus-1000.tsv}
CORPUS_SLICE=${CORPUS_SLICE:-400}

# Every variable the three compose files interpolate. Unset for the child, so `e2e.env` is the only
# source: under `cargo xtask heavy` the lane's variables are already exported, and anything else the
# shell happens to carry (a stray LAPIDARY_STORAGE_ROOT, a POSTGRES_PASSWORD from another harness)
# would silently win over --env-file and could point the bind mounts somewhere else — which teardown
# would then chown and delete.
COMPOSE_UNSET=()
for var in POSTGRES_USER POSTGRES_PASSWORD POSTGRES_DB LAPIDARY_LOG \
  LAPIDARY_STORAGE_ROOT LAPIDARY_INGEST_DIR LAPIDARY_INBOX_DIR LAPIDARY_MODELS_DIR LAPIDARY_BROWSE_ROOT LAPIDARY_DB_PORT LAPIDARY_PORT \
  LAPIDARY_PORT_WEB LAPIDARY_PORT_API LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER \
  LAPIDARY_WORKER_CONCURRENCY LAPIDARY_JOB_LEASE_SECS LAPIDARY_KERNEL_TIMEOUT \
  LAPIDARY_JOB_POLL_SECS LAPIDARY_WORKER_ID LAPIDARY_SETUP_CODE LAPIDARY_COOKIE_SECURE; do
  COMPOSE_UNSET+=(-u "$var")
done

# The only place in this rig that runs `docker compose`. `-p` is not a parameter.
compose() {
  assert_project
  [ -f "$ENVFILE" ] || die "$ENVFILE does not exist yet. Run \`stack.sh up\` (it writes it) first."
  env "${COMPOSE_UNSET[@]}" docker compose -p "$PROJECT" \
    -f "$ROOT/deploy/compose.yaml" \
    -f "$ROOT/scripts/e2e/e2e.override.yaml" \
    --env-file "$ENVFILE" "$@"
}

# ---------------------------------------------------------------------------------------------------
# Small helpers, most of them lifted from target/docker-check/check-plain.sh.

# A one-line summary of a pipeline's output, or "none". Its own function because `set -o pipefail`
# makes `cmd | grep | paste || echo none` print an empty line *and* "none" when grep matches nothing.
or_none() { local out; out=$("$@" 2>/dev/null | paste -sd', ' -); echo "${out:-none}"; }

free_gb() { df --output=avail -BG "$1" | tail -1 | tr -dc '0-9'; }
ram_gib() { awk '/^MemAvailable:/ { printf "%d", $2 / 1048576 }' /proc/meminfo; }
field() { python3 -c "import json,sys; value=json.load(sys.stdin); print($1)"; }
# Every curl in this script carries the rig's session. `command`, so this is not itself.
curl() { command curl -b "$COOKIES" "$@"; }
json() { curl -sf -X "$1" -H 'content-type: application/json' ${3:+-d "$3"} "$2"; }
sql() { compose exec -T db psql -U lapidary -d lapidary -Atc "$1"; }

# Stage 3's thresholds, asserted rather than printed. `seed` exits non-zero on a miss and the whole list
# lands in `seed.json`, because stage 6's pass condition is "the seed counts match" and a seed that only
# echoed them gave nothing to match against.
#
# These live here, beside the other helpers, rather than above `cmd_seed`: they were once glued to it, and
# rewriting the function next door deleted all four. `bash -n` cannot see that — a missing function is a
# runtime name lookup, not a syntax error — so the seed ran to the end printing `command not found` and
# reporting 0 checks. `check.sh` grew a called-but-never-defined check off the back of it.
seed_fail=0
seed_check() { # name, 1|0, detail
  # **The detail is flattened to one line, and that is not tidiness.** This file is
  # `name<TAB>ok<TAB>detail` and the python below splits each line on tabs with `maxsplit=2`; a detail
  # written across two source lines carries a real newline into it, and the second half is then a line
  # with no tabs at all. That happened: `seed.json` was never written, the traceback went to stdout,
  # and the seed exited 0 all the same.
  local flat
  flat=$(printf '%s' "$3" | tr '\n\t' '  ' | tr -s ' ')
  printf '%s\t%s\t%s\n' "$1" "$2" "$flat" >> "$WORK/seed-checks.tsv"
  if [ "$2" = 1 ]; then
    echo "  ok    $1 — $3"
  else
    echo "  FAIL  $1 — $3"
    seed_fail=$((seed_fail + 1))
  fi
}
# `1` when the arithmetic holds, `0` when it does not: a bare `[ ... ]` in a `$( )` would abort under -e.
holds() { if eval "[ $* ]" 2> /dev/null; then echo 1; else echo 0; fi; }
# One counter out of a settle line like `total=412 pending=0 … ingested=412 …`.
counter() { printf '%s\n' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p" | tail -1; }

wait_for() { # url, what, seconds
  timeout "${3:-180}" bash -c "until curl -sf '$1' >/dev/null 2>&1; do sleep 1; done" ||
    { echo "  $2 never answered at $1"; return 1; }
}

# Until `test` reads true from `url`, or the time is up. run-group.sh's, unchanged in behaviour.
until_true() { # url, python test on `value`, what, seconds
  local began; began=$(date +%s)
  while [ $(($(date +%s) - began)) -lt "${4:-180}" ]; do
    [ "$(curl -sf "$1" 2>/dev/null | field "$2" 2>/dev/null)" = True ] &&
      { echo "  $3 after $(($(date +%s) - began)) s"; return 0; }
    sleep 2
  done
  echo "  $3 did not happen within ${4:-180} s ($1)"
  return 1
}

# A batch's counters until nothing is pending or running. check-plain.sh's settle, with `revised` kept:
# it is how a part's second revision shows.
settle() { # library, batch, seconds
  local line began; began=$(date +%s)
  [ -n "${2:-}" ] || { echo "no batch id to follow"; return 1; }
  while [ $(($(date +%s) - began)) -lt "${3:-3600}" ]; do
    line=$(curl -sf "$API/api/libraries/$1/jobs/$2" |
      field '" ".join(f"{k}={value[k]}" for k in ["total","pending","running","ingested","skipped","revised","failedTotal"])' 2>/dev/null)
    [[ "$line" == *" pending=0 running=0 "* ]] && { echo "$line"; return 0; }
    sleep 2
  done
  echo "did not settle within ${3:-3600} s: $line"
  return 1
}

# The store is bind-mounted, and everything in the container writes it as uid 10001. A host directory
# comes up owned by whoever made it, so it has to be handed over before `up` — and handed back before
# `down` removes it, or the host user cannot delete it and `scripts/release-goal.sh` cannot remove the
# worktree. `target/docker-check/group/a/store` is still root-owned proof of forgetting.
# The one image belong to the lane, built once by `build` from the lane's own checkout. A suffixed
# installation borrows them under its own project's names, because compose looks for `<project>-<service>`
# when it is told not to build: `docker tag` names the same layers a second time, so three installations
# cost one build and not a byte of disk. Never the other way round — `build` refuses a suffix.
IMAGE_PROJECT=lapidary-e2e-$LANE

tag_borrowed_images() {
  [ -n "$AS" ] || return 0
  local svc
  for svc in app; do
    docker image inspect "$IMAGE_PROJECT-$svc" > /dev/null 2>&1 || die "there is no \`$IMAGE_PROJECT-$svc\`
  image for $PROJECT to borrow. Build this lane's one image first, with no AS set:
  \`cargo xtask heavy -- scripts/e2e/stack.sh build\`."
    docker tag "$IMAGE_PROJECT-$svc" "$PROJECT-$svc" ||
      die "could not tag $IMAGE_PROJECT-$svc as $PROJECT-$svc."
  done
  echo "  $PROJECT's one image are tags of $IMAGE_PROJECT-*: same layers, nothing built"
}

# Only ever the tags this project put on, and only when there is a suffix — the lane's own one image are
# what every installation is made of, and a `down` that removed them would cost the next one a rebuild.
untag_borrowed_images() {
  [ -n "$AS" ] || return 0
  local svc removed=0
  for svc in app; do
    if docker image inspect "$PROJECT-$svc" > /dev/null 2>&1; then
      docker rmi "$PROJECT-$svc" > /dev/null 2>&1 && removed=$((removed + 1))
    fi
  done
  echo "  removed $removed of $PROJECT's borrowed image tags; $IMAGE_PROJECT-* kept"
}

chown_store() { # owner, e.g. 10001:10001
  local image=''
  for candidate in "$PROJECT-app" docker.io/library/postgres:18; do
    docker image inspect "$candidate" > /dev/null 2>&1 && { image=$candidate; break; }
  done
  [ -n "$image" ] || { echo "  no local image to chown with; leaving $STORE and Lapidary's folders as they are"; return 1; }
  mkdir -p "$STORE"
  local own=() dir
  for dir in "${OWN_DIRS[@]}"; do mkdir -p "$INGEST/$dir"; own+=("/models/$dir"); done
  # Lapidary's own folders in the models folder too (A2): it writes uploads, imports and examples there,
  # and moves a removed model's file into _removed, as uid 10001. The person's folders stay theirs.
  docker run --rm --pull never --user 0 --entrypoint chown \
    -v "$STORE:/var/lib/lapidary" -v "$INGEST:/models" "$image" \
    -R "$1" /var/lib/lapidary "${own[@]}" > /dev/null &&
    echo "  $STORE and ${OWN_DIRS[*]} under $INGEST now owned by $1 (through $image)"
}

# `rm -rf` on a path built from variables gets its own gate. `set -u` is not a safety net for a
# variable that is set and wrong.
remove_store() {
  case "$STORE" in
    "$ROOT"/target/e2e/[0-4]/store | "$ROOT"/target/e2e/[0-4][abc]/store) ;;
    *) die "refusing to remove \`$STORE\`: that is not target/e2e/<lane>[a|b|c]/store." ;;
  esac
  local dir
  for dir in "$STORE"; do
    [ -e "$dir" ] || continue
    rm -rf "$dir" || die "could not remove $dir. If it is still root-owned the reverse chown above
  failed — fix that before the lead runs scripts/release-goal.sh, or the worktree cannot be deleted."
  done
}

# Every path under the ingest tree with its size and modification time, hashed (M4). The app indexes
# the owner's folders where they are and must never copy, rename, move, write or delete anything under
# them; this is how the rig checks that from the host, before and after a scan, a move, a revision and
# an upload.
ingest_fingerprint() {
  # The person's folders only: Lapidary's own (uploads/, imports/, examples/, _removed/) are where it
  # writes, by design (A2).
  (cd "$INGEST" && find . \( -path ./uploads -o -path ./imports -o -path ./examples -o -path ./_removed \) -prune \
    -o -printf '%P %s %T@\n' | sort | sha256sum | cut -d' ' -f1)
}

# `e2e.env`. The password is generated **only when the file is absent**: `POSTGRES_PASSWORD` is applied
# when the db volume is initialised, so a new password against a kept volume fails authentication with
# a message that blames the wrong thing. The ports are rewritten every time, because they belong to the
# lane and a lane's block does not change under it.
write_env() {
  mkdir -p "$WORK" "$RUNS"
  local password=''
  [ -f "$ENVFILE" ] && password=$(sed -n 's/^POSTGRES_PASSWORD=//p' "$ENVFILE" | tail -1)
  if [ -z "$password" ]; then
    password=$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 24)
    note "generated a new database password for $PROJECT (there was none in $ENVFILE)"
  fi
  cat > "$ENVFILE" <<EOF
# Lane $LANE's end-to-end stack, project $PROJECT. Written by scripts/e2e/stack.sh; untracked, under
# target/. deploy/.env is deliberately not used: it is the owner's, and local-only.
#
# The password is kept across rewrites on purpose. POSTGRES_PASSWORD is applied when the lapidary-db
# volume is initialised, so regenerating it while that volume survives breaks authentication.
POSTGRES_USER=lapidary
POSTGRES_PASSWORD=$password
POSTGRES_DB=lapidary
LAPIDARY_LOG=info
# Absolute, both of them: compose resolves a relative path against deploy/, and the api additionally
# shows LAPIDARY_STORAGE_ROOT to the person as where their model is, which only means anything absolute.
LAPIDARY_STORAGE_ROOT=$STORE
# The one models folder (A2).
LAPIDARY_MODELS_DIR=$INGEST
# The browse root (A6): the models folder itself, so no mount leaves this lane. Fixed by the line above,
# so the rig drives the locked picker.
LAPIDARY_BROWSE_ROOT=$INGEST
# The database's loopback port (A5): the lane's spare api port, since the api is the app's own port now.
LAPIDARY_DB_PORT=$LAPIDARY_PORT_API
# Interpolated into scripts/e2e/e2e.override.yaml, which is the only thing that moves the published
# ports off deploy/compose.yaml's literals.
LAPIDARY_PORT_WEB=$LAPIDARY_PORT_WEB
LAPIDARY_PORT_API=$LAPIDARY_PORT_API
LAPIDARY_PORT_WORKER=$LAPIDARY_PORT_WORKER
LAPIDARY_PORT_PEER=$LAPIDARY_PORT_PEER
# The first-run wizard's code (A1), fixed so the seed can run setup without reading the log.
LAPIDARY_SETUP_CODE=E2E-RIG-SETUP
EOF
}

# The rig's account (A1): setup on a fresh stack, sign-in on one already set up. The cookie lands in
# $COOKIES for every curl, and `rig_cookie` hands its value to the flows' browsers.
sign_in_rig() {
  local status
  status=$(command curl -s -o /dev/null -w '%{http_code}' -b "$COOKIES" "$API/api/session")
  [ "$status" = 200 ] && return 0
  if [ "$status" = 409 ]; then
    command curl -sf -c "$COOKIES" -H 'content-type: application/json' \
      -d "{\"code\":\"E2E-RIG-SETUP\",\"username\":\"$RIG_USER\",\"password\":\"$RIG_PASSWORD\"}" \
      "$API/api/setup" > /dev/null || die "setup refused the rig's account; see \`compose logs app\`"
    note "ran first-run setup as $RIG_USER"
  else
    command curl -sf -c "$COOKIES" -H 'content-type: application/json' \
      -d "{\"username\":\"$RIG_USER\",\"password\":\"$RIG_PASSWORD\"}" \
      "$API/api/session" > /dev/null || die "the rig could not sign in as $RIG_USER (status $status)"
  fi
}
rig_cookie() { awk '$6 == "lapidary_session" { print $7 }' "$COOKIES" | tail -1; }

# ---------------------------------------------------------------------------------------------------
# build

# What the three files actually resolve to, asserted before anything is built or started.
#
# This is the one mistake with a cost outside this lane: `deploy/compose.yaml` publishes web on 3000 and
# api on 8080, and if `ports: !override` failed to replace them — a typo in the tag, a service the
# override forgot — compose would publish both and the first lane up would take the owner's ports and
# the browser storage on that origin with them. `compose config` resolves everything and needs no
# daemon, so this costs nothing and runs every time.
verify_ports() {
  local resolved
  resolved=$(compose config --format json 2>/dev/null) || die "\`compose config\` would not resolve. Run
  \`scripts/e2e/stack.sh status\` and read what compose says — a missing LAPIDARY_PORT_* in $ENVFILE is
  the usual cause."
  LAPIDARY_PORT_WEB=$LAPIDARY_PORT_WEB LAPIDARY_PORT_API=$LAPIDARY_PORT_API \
    LAPIDARY_PORT_WORKER=$LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER=$LAPIDARY_PORT_PEER \
    STORE=$STORE INGEST=$INGEST INBOX=$INBOX python3 -c '
import json, os, sys

resolved = json.load(sys.stdin)
# Since A5 the app runs on host networking and binds the lane web port itself; the only port
# published is the database one, on loopback, and it is the lane own.
want = {os.environ["LAPIDARY_PORT_API"]}
published = set()
mounts = {}
app = resolved.get("services", {}).get("app", {})
app_port = str((app.get("environment") or {}).get("LAPIDARY_PORT", ""))
for name, service in resolved.get("services", {}).items():
    for port in service.get("ports") or []:
        published.add(str(port.get("published")))
    for volume in service.get("volumes") or []:
        if volume.get("type") == "bind":
            mounts.setdefault(volume["source"], set()).add(name)

problems = []
if published != want:
    problems.append(f"published host ports are {sorted(published)}; this lane owns {sorted(want)}")
if app_port != os.environ["LAPIDARY_PORT_WEB"]:
    problems.append("the app would bind " + (app_port or "its own default") + "; this lane owns " + os.environ["LAPIDARY_PORT_WEB"])
if app.get("network_mode") != "host":
    problems.append("the app is not on host networking, so nothing would bind the lane port")
for forbidden in ("3000", "8080", "8081", "8082"):
    if forbidden in published:
        problems.append(f"{forbidden} is published — that is deploy/compose.yaml`s literal, and the owner`s")
for path in (os.environ["STORE"], os.environ["INGEST"], os.environ["INBOX"]):
    if path not in mounts:
        problems.append(f"nothing bind-mounts {path}; the env file is not the one compose read")
for path in mounts:
    if not path.startswith(os.path.dirname(os.environ["STORE"])):
        problems.append(f"{path} is bind-mounted from outside this lane`s target/e2e directory")
print("  app on " + app_port + ", database on " + ", ".join(sorted(published)) + "; bind mounts " + ", ".join(sorted(mounts)))
for problem in problems:
    print("  " + problem, file=sys.stderr)
sys.exit(1 if problems else 0)
' <<< "$resolved" || die "the resolved compose configuration is not this lane's. Nothing was started."
}

cmd_build() {
  [ -z "$AS" ] || die "an installation with AS=$AS borrows the lane's images; it never builds its own.
  Build once with no AS set (\`cargo xtask heavy -- scripts/e2e/stack.sh build\`), then bring each
  installation up — \`up\` tags the lane's one image under this project's names."
  # **The 8 GB floor below is the protocol's and this rig does not let anybody past it.**
  #
  # R6 (2026-10-04) measured what a build of all five images then cost on this machine and it was 7.5 GB,
  # 4 GB of it the shared release stage. Since S1 there is one image, built cold in 130 s.
  #
  # **The owner settled this on 2026-10-04: `--min-free <gb>` exists, and using it is recorded.** R6
  # wrote the override, used it at 4 GB and then 2 GB to finish, took it back out, and said so at the top
  # of its hand-back — loosening a guard `docs/goals/PROTOCOL.md` states is the owner's call, and it had
  # not asked. The owner's answer was to keep the flag rather than the round trip, on the condition that
  # it is deliberate and visible: it must be passed by hand, it says so in the log of the run that used
  # it, and the goal file says why. That is the same bargain `up --min-ram` already makes. `ask_owner`
  # below is still what happens when nobody passes it.
  local min_free=8
  while [ $# -gt 0 ]; do
    case $1 in
      --min-free)
        min_free=${2:-}
        case "$min_free" in
          '' | *[!0-9]*) die "--min-free takes a whole number of GB; got \`$min_free\`." ;;
        esac
        shift 2
        ;;
      *) die "\`build\` takes --min-free <gb> and nothing else; got \`$1\`." ;;
    esac
  done
  [ "$min_free" = 8 ] ||
    note "--min-free $min_free: built under the 8 GB default on purpose. Say why in the goal file."
  local services=${SERVICES:-app}
  write_env
  verify_ports
  local summary=$WORK/build-summary.txt
  echo "== $(date -Is) build of $PROJECT from $(git -C "$ROOT" rev-parse --short HEAD); / $(free_gb /) GB free; /mnt/Storage $(free_gb "$ROOT") GB free${min_free:+; floor ${min_free} GB}" > "$summary"
  git -C "$ROOT" rev-parse HEAD > "$WORK/built-from.sha"
  for svc in $services; do
    # The guard, before each service and not once at the top: a release build layer alone can move
    # root free space by a gigabyte or more, and the api and worker stages each build one. Under it we
    # stop and ask — we never prune, because this rig's cache is not the only thing in that cache.
    [ "$(free_gb /)" -ge "$min_free" ] || ask_owner "/ has $(free_gb /) GB free, under the $min_free GB this build needs,
  before service \`$svc\`. This rig never prunes: it does not own the builder cache and cannot tell
  which of it is somebody else's. Free space elsewhere, or say to go ahead — and if what is left to
  build is a runtime layer and one binary each, say so, because that is a different argument from a
  cold build and the floor cannot tell them apart."
    [ "$(free_gb "$ROOT")" -ge 20 ] || ask_owner "$ROOT has $(free_gb "$ROOT") GB free, under 20 GB,
  before service \`$svc\`."

    local before after took result size
    before=$(docker image inspect "$PROJECT-$svc" --format '{{.Id}}' 2>/dev/null)
    note "building $svc"
    local began=$SECONDS
    if compose --progress plain build "$svc" > "$WORK/build-$svc.log" 2>&1; then result=ok; else result=FAILED; fi
    took=$((SECONDS - began))
    after=$(docker image inspect "$PROJECT-$svc" --format '{{.Id}}' 2>/dev/null)
    size=$(docker image inspect "$PROJECT-$svc" --format '{{.Size}}' 2>/dev/null || echo none)
    printf '%s: %s in %s s; image %s %s bytes; / %s GB free\n' \
      "$svc" "$result" "$took" "$PROJECT-$svc" "$size" "$(free_gb /)" | tee -a "$summary"
    # The image this build replaced is now untagged and holds whole layers. Removed by its id, and only
    # when it carries no tag at all — so nothing that another project still names can be caught by it.
    # Never `docker image prune`: that would reach dangling images this rig did not make.
    if [ -n "$before" ] && [ -n "$after" ] && [ "$before" != "$after" ] &&
      [ "$(docker image inspect "$before" --format '{{len .RepoTags}}' 2>/dev/null)" = 0 ]; then
      docker rmi "$before" > /dev/null 2>&1 &&
        echo "  removed the untagged image $svc replaced (${before:7:12})" | tee -a "$summary"
    fi
    [ "$result" = ok ] || die "$svc failed to build; see $WORK/build-$svc.log"
  done
  echo "== $(date -Is) build done" | tee -a "$summary"
  note "summary in $summary"
}

# ---------------------------------------------------------------------------------------------------
# up

cmd_up() {
  # The RAM floor is a default, not a constant, and going under it is an argument somebody has to make out
  # loud: `--min-ram <gib>` says so in the log of the run that used it. The default's premise is one stack
  # whose declared ceilings total 4.3 GB; three installations that are never seeded are a different case,
  # and the number they were brought up on belongs in whichever goal file quotes their captures.
  # **6 GB is more than this stack costs on `/` and it stays all the same.** R6 measured it: the store
  # and the ingest tree are bind mounts under `target/e2e/`, which is on the big filesystem, so what
  # lands on the root is the database volume, the containers' writable layers and the headless profiles
  # in /tmp — well under 500 MB for a 404-part library. A `--min-free` override was written here and
  # taken out again for the reason above `cmd_build`'s own floor: a guard the protocol states is the
  # owner's to lower.
  local min_ram=5
  while [ $# -gt 0 ]; do
    case "$1" in
      --min-ram)
        min_ram=${2:-}
        case "$min_ram" in
          '' | *[!0-9]*) die "--min-ram takes a whole number of GiB; got \`$min_ram\`." ;;
        esac
        shift 2
        ;;
      *) die "\`up\` takes --min-ram <gib> and nothing else; got \`$1\`." ;;
    esac
  done
  [ "$(free_gb /)" -ge 6 ] || ask_owner "/ has $(free_gb /) GB free, under the 6 GB this stack needs.
  What this actually puts on the root is the database volume and the containers' own layers — the store
  and the ingest tree are bind mounts on the big filesystem — so if that is all that is left to come,
  that is the argument to put to the owner rather than a floor to lower."
  [ "$min_ram" = 5 ] ||
    note "--min-ram $min_ram: brought up under the 5 GiB default on purpose. Say why in the goal file."
  # The declared ceilings in deploy/ total 4 GB. A stack brought up beside a cargo build is how a
  # session gets its processes killed; 5 GiB available is the floor that leaves the stack room.
  [ "$(ram_gib)" -ge "$min_ram" ] || ask_owner "$(ram_gib) GiB of RAM is available, under the $min_ram GiB
  floor. This stack's declared ceilings total 4 GB (db 1g, app 3g).
  Wait until the other lanes have stopped compiling, then run this again — or, for installations that are
  never seeded, say \`--min-ram <gib>\` and record the number you chose."

  write_env
  # Before the directories exist, so a wrong bind source is caught before docker creates it as root.
  verify_ports
  mkdir -p "$STORE" "$INGEST" "$INBOX" "$RUNS"
  tag_borrowed_images
  chown_store 10001:10001

  note "up"
  local began=$SECONDS
  compose up -d --no-build 2>&1 | tail -8
  wait_for "$API/api/healthz" "the app" || die "the app never became healthy. \`stack.sh status\` and
  \`docker compose -p $PROJECT logs app\` say why."
  wait_for "$WEB/" "the browser app" || die "the browser app never answered on $LAPIDARY_PORT_WEB."

  local secs=$((SECONDS - began))
  local extensions migration
  extensions=$(sql "SELECT string_agg(extname || ' ' || extversion, ', ' ORDER BY extname) FROM pg_extension")
  migration=$(sql "SELECT max(version) || ' of ' || count(*) FROM _sqlx_migrations")
  note "healthy after ${secs} s"
  echo "  services: $(compose ps --format '{{.Service}} {{.Status}}' | paste -sd'; ' -)"
  echo "  extensions: $extensions"
  echo "  newest migration: $migration"
  echo "  port: app $LAPIDARY_PORT_WEB"

  python3 - "$WORK/stack.json" <<EOF
import json, sys
json.dump({
    "project": "$PROJECT", "lane": $LANE, "upSeconds": $secs,
    "ports": {"web": $LAPIDARY_PORT_WEB, "api": $LAPIDARY_PORT_WEB, "worker": $LAPIDARY_PORT_WEB},
    "extensions": """$extensions""", "migration": """$migration""",
    # Which tree these images were built from, and which tree drove them. A comparison is only meaningful
    # (see drive --compare)
    # between reports that say so: a flow status that changed between two shas is a regression, and the
    # same change between two builds of one sha is flakiness. They differ whenever a stack is kept across
    # a merge, which is exactly when somebody would misread the comparison.
    "builtFromSha": "$(cat "$WORK/built-from.sha" 2>/dev/null || echo unknown)",
    "sha": "$(git -C "$ROOT" rev-parse HEAD)",
    "branch": "$(git -C "$ROOT" rev-parse --abbrev-ref HEAD)",
}, open(sys.argv[1], "w"), indent=2)
EOF
}

# ---------------------------------------------------------------------------------------------------
# seed: one ingest tree, four libraries

# The ingest tree, whose subdirectories become this library's categories.
build_ingest_tree() {
  if [ -f "$INGEST/.seeded" ] && [ "${FRESH_INGEST:-}" != 1 ]; then
    echo "  ingest tree already built ($(find "$INGEST" -type f ! -name .seeded | wc -l) files); FRESH_INGEST=1 to rebuild"
    # Still checked, and this is the path that most needs it: a tree kept across a `down` is reattached
    # by the next `up`, and if that did not happen the mount is stale and every scan finds nothing.
    check_worker_sees_ingest
    return 0
  fi
  # Clear the CONTENTS, never the directory. `rm -rf "$INGEST"` replaces the inode, and the worker's
  # bind mount follows the inode rather than the path — so the container goes on seeing the old, deleted,
  # empty directory and every scan finds nothing, with no error anywhere. That cost a run: the walk
  # reported `total=1 ingested=0` and looked like a broken scan route. Measured proof, for anyone who
  # doubts it: host inode 16909401, container /ingest inode 16908512, after one `rm -rf`.
  mkdir -p "$INGEST"
  # Never Lapidary's own folders: they are the app's (uid 10001), and the examples in them are seeded.
  find "$INGEST" -mindepth 1 -maxdepth 1 ! -name uploads ! -name imports ! -name examples ! -name _removed \
    -exec rm -rf {} +
  mkdir -p "$INGEST/step" "$INGEST/misc" "$INGEST/alike"

  # step/: the six B-rep fixtures, kept on purpose and for the opposite reason they were put here.
  #
  # They used to be what sent work through the OCCT worker. Since R2 a STEP or IGES file is **refused at
  # ingest, by name**, so these six are now the natural fixture for that refusal — `fixtures/step/` was
  # kept for exactly this. The seed asserts six refused jobs, each naming its own format and what this
  # library takes; a scan that *skipped* them in silence is the failure this directory now exists to
  # catch, and it is the one `is_model_file` still chains `CAD_FORMATS` in to prevent.
  cp "$ROOT"/fixtures/step/* "$INGEST/step/" || die "fixtures/step is not where it was."
  # misc/: the two non-STL mesh formats, so the format facet has three values and not one — and the
  # bracket, which is the one part in this tree we have a real picture of. `pictures` attaches
  # `fixtures/bracket-lp-1042-03.thumb.webp` to it after the scan, so the flow can assert a card drawing
  # the part's OWN image and not only the format placeholder every other card shows.
  cp "$ROOT"/fixtures/*.obj "$ROOT"/fixtures/*.3mf "$INGEST/misc/" || die "the .obj/.3mf fixtures moved."
  cp "$ROOT/fixtures/bracket-lp-1042-03.stl" "$INGEST/misc/" || die "the bracket fixture moved."

  # The corpus slice: real creator/set/part paths, kept as they are. `corpus-1000/` holds symlinks into
  # /mnt/Storage2 and a container cannot follow one out of its own mount, so `-L` — dereference — is
  # what turns them into files. `--parents` keeps the real tree, which is what makes the folder facet
  # worth looking at. IFS is a tab and nothing else: these paths have spaces and parentheses in them.
  [ -d "$CORPUS" ] || die "the corpus is not at \`$CORPUS\`. It is 1,000 symlinks into
  /mnt/Storage2/All/STL Files, made by an earlier goal, and the tsv's paths are relative to it — note
  the \`STL Files\` level. It lives under the MAIN checkout's target/ ($MAIN_ROOT), because target/ is
  per-worktree; set LAPIDARY_CORPUS and LAPIDARY_CORPUS_TSV if it is somewhere else on this machine. If
  /mnt/Storage2 is not mounted, mount it; the slice needs the real bytes."
  [ -f "$CORPUS_TSV" ] || die "no corpus index at $CORPUS_TSV."
  local first
  first=$(head -1 "$CORPUS_TSV" | cut -f2)
  [ -e "$CORPUS/$first" ] || die "the first row of $CORPUS_TSV names \`$first\`, which does not exist
  under \`$CORPUS\`. Either the corpus root is wrong or /mnt/Storage2 is not mounted — copying would
  otherwise quietly produce an empty library."

  # Read *down* the sorted list until the slice is full, rather than taking the first N rows.
  #
  # 20 of the 1,000 symlinks dangle — one whole directory's targets have gone from /mnt/Storage2 since
  # the corpus was made, and all 20 are inside the smallest 400. `cp -L` on a dangling link fails, so
  # taking rows 1-400 blind would either abort the seed or quietly leave a 380-file library and break the
  # paging arithmetic that "400 is the number" rests on. Skipping them and reading on keeps the slice
  # "the smallest 400 that exist", and reports the rot as a number instead of hiding it.
  local copied=0 dangling=0 failed=0 rows=0
  while IFS=$'\t' read -r size path; do
    [ -n "$path" ] || continue
    [ "$copied" -lt "$CORPUS_SLICE" ] || break
    rows=$((rows + 1))
    if [ ! -e "$CORPUS/$path" ]; then
      dangling=$((dangling + 1))
      continue
    fi
    # -L dereferences: the corpus is symlinks and a container cannot follow one out of its own mount.
    # --parents keeps the real creator/set/part tree, which is what makes the category facet worth
    # looking at.
    if (cd "$CORPUS" && cp -L --parents -- "$path" "$INGEST/"); then
      copied=$((copied + 1))
    else
      failed=$((failed + 1))
      echo "    could not copy: $path"
    fi
  done < <(sort -n "$CORPUS_TSV")
  [ "$failed" = 0 ] || die "$failed corpus files exist but would not copy. That is not the dangling-link
  case (those are counted separately and skipped) — check permissions on $CORPUS."
  [ "$copied" = "$CORPUS_SLICE" ] || die "only $copied of $CORPUS_SLICE corpus files could be copied after
  reading all $rows rows of $CORPUS_TSV ($dangling of them dangling). The corpus has lost more than it
  can spare; remake it, or lower CORPUS_SLICE and say so in the goal's Record."
  echo "  corpus slice: $copied files from $rows rows ($dangling dangling links skipped), $(du -sh --apparent-size "$INGEST" | cut -f1) so far"

  # alike/: one pair, the same bytes at two paths.
  #
  # This directory held four files and three cases — identical, a rotation and a 15 % scale — because
  # shape similarity was a feature and `docs/phase-6.md` claimed the descriptor was rotation-invariant.
  # R3 deleted `part_shape`, `part_link` and every likeness route, so the rotation and the scale were
  # fixtures for nothing; they went, and `skew-stl.py` with them.
  #
  # **The identical pair stays, because the property it shows is the store's and not the detector's.**
  # `PgBlobs::library_holds` joins on `source_path` *and* hash, so the same bytes at a second path are a
  # second PART rather than a `Skipped` job — and the content-addressed store gives that second part a
  # row and transfers nothing. Two flanges here are also what `search` narrows to.
  local flange=$ROOT/example/parts/flange-dn40-lp-3310-02.stl
  cp "$flange" "$INGEST/alike/flange-dn40-lp-3310-02.stl"
  cp "$flange" "$INGEST/alike/flange-dn40-lp-3310-02-second-copy.stl"

  # Readable by the container's uid, which is 10001 and not this user.
  #
  # Four of the corpus files are mode 0600 at the source — a 2022 download that arrived that way — and
  # `cp` carries the source's mode across, so the worker got `Permission denied (os error 13)` on each.
  # This is our own throwaway copy and not the user's library, so widening it is right; `a+rX` adds read
  # everywhere and the execute bit only where it already exists, so directories become traversable and
  # files do not become executable.
  chmod -R a+rX "$INGEST"

  : > "$INGEST/.seeded"
  echo "  ingest tree: $(find "$INGEST" -type f ! -name .seeded | wc -l) files, $(du -sh --apparent-size "$INGEST" | cut -f1), $(find "$INGEST" -mindepth 1 -type d | wc -l) directories"
  check_worker_sees_ingest
}

# The worker must actually see the tree. This is the assertion the inode bug needed: without it a broken
# mount looks like a broken scan route, three checks downstream and a scan's worth of time later.
check_worker_sees_ingest() {
  local host_files seen
  host_files=$(find "$INGEST" -type f ! -name .seeded | wc -l)
  seen=$(compose exec -T app sh -c 'find /models -type f ! -name .seeded | wc -l' 2>/dev/null | tr -dc '0-9')
  [ "${seen:-0}" = "$host_files" ] || die "the host has $host_files files under $INGEST but the worker sees
  ${seen:-0} in /models, so the bind mount is not showing this directory and every scan will find nothing.
  A bind mount follows the inode, not the path: if anything replaced the directory rather than its
  contents, only \`stack.sh down\` then \`up\` reattaches it. Compare \`stat -c %i $INGEST\` with
  \`compose exec -T app stat -c %i /models\` to confirm that is what happened."
  echo "  the worker sees all $seen of them in /models"
}

# A library by name, made if it is not there. Names, not ids, so `seed` is re-runnable.
library_named() { # name
  local existing
  existing=$(curl -sf "$API/api/libraries" | field "next((l['id'] for l in value if l['name'] == $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1")), '')")
  if [ -n "$existing" ]; then echo "$existing"; return 0; fi
  json POST "$API/api/libraries" "{\"name\":$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1")}" | field 'value["id"]'
}

# The chunked upload, following the plan the server answers rather than sending regardless.
#
# `POST /uploads/probe` answers `UploadPlan { have, needBytes }`, and those two cases are the whole
# protocol: `have` means this library already holds these bytes at this path and there is nothing to do;
# `needBytes` means send them. (There was a third, `needRows`, for bytes the store already held; since M4
# an upload is its own file in the inbox and there is no stored copy to reuse.) check-plain.sh sent the chunks unconditionally
# because it only ever ran against an empty library — do that against a library that already holds the
# file and the PUT is refused, correctly, with nothing staged to write into. "Hash first, always" is what
# makes the plan possible; ignoring it is what made this rig look broken on its second run.
upload_file() { # library, local file, source path in the library
  local lib=$1 file=$2 path=$3 hash size offset=0 manifest plan commit
  hash=$(cd "$ROOT/web" && node -e "const {blake3}=require('hash-wasm'); blake3(require('fs').readFileSync(process.argv[1])).then(h=>console.log(h))" "$file") ||
    { echo "  could not hash $file (is web/node_modules linked?)"; return 1; }
  size=$(stat -c %s "$file")
  manifest="{\"files\":[{\"path\":\"$path\",\"blake3\":\"$hash\"}]}"
  plan=$(json POST "$API/api/libraries/$lib/uploads/probe" "$manifest") ||
    { echo "  probe refused for $path"; return 1; }

  case "$(printf '%s' "$plan" | field 'next((k for k in ("have", "needBytes") if value.get(k)), "?")')" in
    have)
      echo "already here (the library holds these bytes at this path)"
      return 0
      ;;
    needBytes)
      while [ $offset -lt "$size" ]; do
        dd if="$file" iflag=skip_bytes,count_bytes skip=$offset count=$((4 * 1048576)) status=none |
          curl -sf -X PUT -H 'content-type: application/octet-stream' --data-binary @- \
            "$API/api/libraries/$lib/uploads/$hash?offset=$offset" > /dev/null ||
          { echo "  chunk at $offset refused for $path"; return 1; }
        offset=$((offset + 4 * 1048576))
      done
      ;;
    *)
      echo "  the probe named neither have nor needBytes for $path: $plan"
      return 1
      ;;
  esac

  commit=$(json POST "$API/api/libraries/$lib/uploads/commit" "$manifest") ||
    { echo "  commit refused for $path"; return 1; }
  settle "$lib" "$(echo "$commit" | field 'value["batchId"]')" 600
}

# A part's own picture, attached the way a person attaches one.
#
# `POST /api/parts/{id}/images` takes the bytes and nothing else: the server normalizes, re-encodes to
# WebP and decides inline-or-blob at the 64 KB line itself (`images.rs`), so nothing here has to know
# that rule. **Nothing in this product produces a picture** — every one is something somebody added —
# so without this the whole grid shows format placeholders and the `pictures` flow could only ever test
# half of what it claims. Idempotent by checking the gallery first: `add_part_image` appends a position
# rather than replacing, so re-seeding one stack would otherwise grow a gallery per run.
seed_picture() { # part id, image file
  local have
  have=$(curl -sf "$API/api/parts/$1/images" | field 'len(value)')
  if [ "${have:-0}" -gt 0 ]; then echo "already has ${have} picture(s)"; return 0; fi
  curl -sf -X POST --data-binary "@$2" "$API/api/parts/$1/images" > /dev/null ||
    { echo "  the server would not take $2 for part $1"; return 1; }
  curl -sf "$API/api/parts/$1/images" |
    field '"%d picture, %s" % (len(value), "inline" if (value[0]["src"] or "").startswith("data:") else value[0]["src"][:40])'
}

cmd_seed() {
  wait_for "$API/api/healthz" "the api" 30 || die "nothing is up. Run \`stack.sh up\` first."
  sign_in_rig
  echo "== INGEST TREE"
  build_ingest_tree

  # The Owner library's folder (M4, below), made before Sweep's scan so it is part of the tree Sweep
  # sees on every run, with the same bytes every time: Owner's revision step puts them back after.
  mkdir -p "$INGEST/m4-owner"
  cp "$ROOT/example/parts/flange-dn40-lp-3310-02.stl" "$INGEST/m4-owner/flange-dn40-lp-3310-02.stl"
  local fingerprint
  fingerprint=$(ingest_fingerprint)

  echo "== SWEEP, the whole tree"
  local sweep began batch
  sweep=$(library_named Sweep)
  [ -n "$sweep" ] || die "could not create the Sweep library."
  # Sweep holds every folder of the tree, added as a person adds them with Import (A2) — and not
  # Lapidary's own, which hold what other libraries uploaded and imported.
  local top
  while IFS= read -r top; do
    json POST "$API/api/libraries/$sweep/sources" "{\"path\":$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$top")}" > /dev/null ||
      die "could not add $top to Sweep"
  done < <(find "$INGEST" -mindepth 1 -maxdepth 1 ! -name uploads ! -name imports ! -name examples ! -name _removed ! -name .seeded -printf '%f\n' | sort)
  # One scan is the whole ingest directory into one library: POST /scan takes no subpath.
  began=$SECONDS
  batch=$(json POST "$API/api/libraries/$sweep/scan" | field 'value["batchId"]')
  # The worker's memory while the STEP files go through the kernel. Sampled into a file, never into
  # this script's stdout: a background writer on a `tee` pipe keeps it open and the caller hangs.
  ( while true; do docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' "$PROJECT-app-1" 2>/dev/null; sleep 2; done ) > "$WORK/worker-stats.txt" 2>/dev/null &
  local stats=$!
  local scanned; scanned=$(settle "$sweep" "$batch" 5400)
  kill "$stats" 2>/dev/null
  echo "  scan: $scanned in $((SECONDS - began)) s"
  echo "  worker memory, peak sampled: $(awk '{print $2}' "$WORK/worker-stats.txt" | sort -h | tail -1) (ceiling 2GiB)"
  # `reason`, not `message` — `JobFailure` is `{job, path, reason, attempts}`, and asking for the wrong
  # key returned an empty string for every failure, which hid five real errors behind ''.
  local failures
  failures=$(curl -sf "$API/api/libraries/$sweep/jobs/$batch" |
    field '"; ".join(f["path"].split("/")[-1] + ": " + f["reason"][:150] for f in value.get("failed", []))')
  echo "  failures: ${failures:-none}"
  local unexplained
  unexplained=$(curl -sf "$API/api/libraries/$sweep/jobs/$batch" |
    field 'sum(1 for f in value.get("failed", []) if not (f.get("reason") or "").strip())')
  : > "$WORK/seed-checks.tsv"
  local ingested failed_total
  ingested=$(counter "$scanned" ingested)
  failed_total=$(counter "$scanned" failedTotal)
  echo "  this batch: ingested=${ingested:-none} of $(counter "$scanned" total) queued"

  # THE SIX B-REP FIXTURES ARE REFUSED BY NAME, and that is now the headline assertion of this seed.
  #
  # `is_model_file` still chains `CAD_FORMATS`, deliberately, so a `.step` file is QUEUED and then
  # refused out loud — the alternative is a scan that skips it in silence, which is the failure this
  # repository keeps finding. So all six appear in `failed`, each with `cad_refusal`'s own sentence, and
  # that sentence must NAME THE FORMAT of the file it refused: a `.igs` refused as "STEP" would send
  # somebody to re-export the wrong thing. Read per file rather than counted, because six refusals that
  # all said the same format would pass a count.
  local refused refused_mismatch
  refused=$(curl -sf "$API/api/libraries/$sweep/jobs/$batch" | field '
sum(1 for f in value.get("failed", [])
    if f["path"].startswith("step/")
    and "does not read" in (f.get("reason") or "")
    and f["path"].rsplit(".", 1)[-1].upper() in (f.get("reason") or ""))')
  refused_mismatch=$(curl -sf "$API/api/libraries/$sweep/jobs/$batch" | field '
"; ".join(f["path"] + " -> " + (f.get("reason") or "")[:90]
          for f in value.get("failed", [])
          if f["path"].startswith("step/")
          and f["path"].rsplit(".", 1)[-1].upper() not in (f.get("reason") or ""))')
  seed_check "the six B-rep fixtures are refused, each naming its own format" \
    "$(holds "${refused:-0}" -eq 6)" \
    "${refused:-0} of 6 refused by name${refused_mismatch:+ — mismatched: $refused_mismatch}"

  # Stage 3 said "no failures". That is not what a slice of 400 real downloaded files can promise: one of
  # them has a non-finite coordinate at triangle 49957 and the application says so, precisely and
  # actionably, which is the behaviour we want rather than a regression. Add the six refusals, which are
  # correct behaviour too, and the bound is seven. **And every failure must explain itself** — a failure
  # with an empty reason would break this repository's own rule that errors say what broke and what to do.
  seed_check "nothing fails but the six refusals and the one known-corrupt file" \
    "$(holds "${failed_total:-99}" -le 7)" \
    "failedTotal=${failed_total:-none} of $(counter "$scanned" total)"
  seed_check "every failure explains itself" "$(holds "${unexplained:-99}" -eq 0)" \
    "${unexplained:-?} of ${failed_total:-?} failures carry no reason"
  seed_check "the scan wrote nothing under the ingest tree" \
    "$(holds "$(ingest_fingerprint)" = "$fingerprint")" "fingerprint $fingerprint"

  local parts folders formats
  parts=$(sql "SELECT count(*) FROM part WHERE library_id = '$sweep'")
  # **The thumbnail wait loop is gone, and nothing replaces it.** It waited for a `thumbnail` derivative
  # per part, because thumbnails were their own jobs that landed after the ingest batch settled. Nothing
  # renders (R3) and `0055` deleted every such row (R4), so the loop would have waited out its full 600 s
  # and then failed — on a library that is exactly right.
  folders=$(curl -sf "$API/api/libraries/$sweep/folders" | python3 -c 'import json,sys
def count(nodes): return sum(1 + count(n.get("children") or []) for n in nodes)
print(count(json.load(sys.stdin)))')
  formats=$(curl -sf "$API/api/libraries/$sweep/facets" | field '", ".join(f["value"] for f in value["formats"])')
  echo "  parts $parts, folders $folders, formats: $formats"
  # 400 corpus + 3 misc + 2 alike = 405, less the one corrupt file = 404. The six `step/` files are
  # queued and refused, so they are in `total` and in `failed` and in no part count — which is the drop
  # this floor records. Asserted on what the library HOLDS, not on this batch's `ingested`: a second seed
  # against a library that already has the tree ingests nothing new and would fail a per-batch check
  # while being perfectly correct.
  seed_check "the library holds at least 404 parts" "$(holds "${parts:-0}" -ge 404)" "$parts parts"
  seed_check "at least 8 categories" "$(holds "${folders:-0}" -ge 8)" "$folders"
  # Three formats, and **step and igs must be ABSENT**. The facet is the one place the refusal is visible
  # from outside without reading a job: a B-rep file that had ingested anyway would show up here.
  local missing='' present=''
  for want in stl obj 3mf; do
    case ",${formats// /}," in *",$want,"*) ;; *) missing="$missing $want" ;; esac
  done
  for banned in step stp iges igs; do
    case ",${formats// /}," in *",$banned,"*) present="$present $banned" ;; esac
  done
  seed_check "the three mesh formats are present and no B-rep format is" \
    "$(holds -z "\"$missing$present\"")" \
    "${formats:-none}${missing:+ (missing$missing)}${present:+ (REFUSED FORMAT INGESTED:$present)}"

  echo "== ONE PART'S OWN PICTURE"
  local bracket
  bracket=$(sql "SELECT id FROM part WHERE library_id = '$sweep' AND source_path = 'misc/bracket-lp-1042-03.stl'")
  if [ -n "$bracket" ]; then
    echo "  bracket-lp-1042-03: $(seed_picture "$bracket" "$ROOT/fixtures/bracket-lp-1042-03.thumb.webp")"
  fi
  local pictured
  pictured=$(sql "SELECT count(*) FROM part_image i JOIN part p ON p.id = i.part_id WHERE p.library_id = '$sweep'")
  # One, and exactly one: the flow asserts that the bracket's card draws a picture AND that every other
  # card names its format instead, so a second picture arriving from somewhere would weaken both halves.
  seed_check "exactly one part in Sweep has a picture" "$(holds "${pictured:-0}" -eq 1)" \
    "${pictured:-?} part_image rows; the bracket's own photograph, inline under the 64 KB line"

  echo "  re-scan settles all-skipped:"
  local again rescan
  again=$(json POST "$API/api/libraries/$sweep/scan" | field 'value["batchId"]')
  rescan=$(settle "$sweep" "$again" 3600)
  echo "    $rescan"
  seed_check "the re-scan adds nothing" \
    "$(holds "$(counter "$rescan" ingested)" -eq 0 -a "$(counter "$rescan" skipped)" -ge 404)" \
    "$rescan (seven short of total: the six B-rep files are refused again — the refusal is Permanent for
    one job, not a memory across scans — and the corrupt file fails again)"

  # THE DERIVATIVE TABLE HOLDS ONLY WHAT SOMETHING CAN STILL BUILD.
  #
  # `DerivativeKind` is `export_stl` and `export_3mf` (R3) and `0055` deleted every row of the other
  # seven kinds (R4). This asserts it from outside, over a library built from nothing by this very run —
  # which is the case a migration test cannot make, because the migration tests seed the dead rows in raw
  # SQL and then check they are gone. Here nothing ever wrote one. The predicate is the migration's own,
  # `kind NOT IN (the two exports)`, so a kind nobody thought to list fails this too.
  local bad_kinds kinds
  bad_kinds=$(sql "SELECT count(*) FROM derivative WHERE kind NOT IN ('export_stl', 'export_3mf')")
  kinds=$(sql "SELECT coalesce(string_agg(DISTINCT kind, ', ' ORDER BY kind), 'none') FROM derivative")
  seed_check "derivative holds no render kinds" "$(holds "${bad_kinds:-99}" -eq 0)" \
    "${bad_kinds:-?} rows of a kind nothing builds; kinds present: ${kinds:-none}"

  # A RESTARTED WORKER QUEUES NOTHING IT CANNOT RUN — with a positive control, which is the half
  # that makes the check mean anything.
  #
  # Four mechanisms used to re-queue the whole corpus at every worker start, and each failed silently
  # rather than loudly: `revisions_missing(Thumbnail)` reported every revision for ever, `jobs.rs`'s
  # per-format `produce` ladder swept every rung and CAD read, the rung route answered 202 for work
  # that would never land, and `PgShapes::stale_revisions` inner-joined the L0 so a part without one
  # was never profiled and the sweep said "nothing stale". R3 holds all four shut with
  # `DerivativeKind::ALL`, under a unit test. This is the same assertion against the real binary, the
  # real schema and a real 404-part library.
  #
  # **"The job table gained nothing" would pass on a sweep that no longer runs at all**, and that is
  # not a hypothetical: `enqueue_stale_derivatives` logs and swallows every database error, so a sweep
  # whose SQL stopped parsing is exactly as quiet as one with nothing to do — R3 found this and gave
  # its own test a positive control for it. So one stale thing is planted first, of the one kind
  # something can still build, and the assertion is that the restart queues **exactly that and
  # nothing else**. Zero now fails too, and it fails saying the sweep did not run.
  #
  # The plant is an `export_stl` row with a kernel version that is not this build's, stored inline as
  # one byte so it needs no blob and no foreign key, on a part whose source is an STL. Removed again
  # with the job it produced, because `drive` runs against this library afterwards.
  #
  # Counted across the whole `job` table and not one library's: the sweep is instance-wide, so a
  # re-queue of the Default library's six examples would be just as wrong.
  # **Counted off a watermark, not off a clock.** The first draft asked for jobs `created_at > now() -
  # interval '5 minutes'` and read **1,649** — the whole scan, which had finished two minutes earlier.
  # `job.id` is a uuidv7 and so time-ordered, which makes `id > <the last id before the restart>` an
  # exact "what this restart added" with no window to get wrong.
  local plant_rev watermark jobs_before jobs_after queued_kinds
  # `derivative` is `UNIQUE (revision_id, kind)`, so the revision must not already carry an
  # `export_stl` — and after one run of this check it does, because the sweep queued the rebuild and the
  # worker WROTE it. That is the check working; it also made the second run of this seed die on a
  # duplicate key. So: a revision with no export yet, and the cleanup below takes the real export away
  # again, which leaves the library with no derivatives at all — what a freshly seeded one has.
  plant_rev=$(sql "SELECT r.id FROM revision r JOIN part p ON p.id = r.part_id
                     JOIN file f ON f.revision_id = r.id AND f.role = 'source' AND f.format = 'stl'
                    WHERE p.library_id = '$sweep'
                      AND NOT EXISTS (SELECT 1 FROM derivative d
                                       WHERE d.revision_id = r.id AND d.kind = 'export_stl')
                    LIMIT 1")
  [ -n "$plant_rev" ] || die "no STL revision in Sweep to plant a stale export on."
  # `params_json` is NOT NULL and `thumb_bytes` XOR `blake3` is `derivative_storage_is_exclusive` —
  # one byte inline is the cheapest row that satisfies both and needs no blob and no foreign key.
  sql "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json)
       VALUES (uuidv7(), '$plant_rev', 'export_stl', '\\x00'::bytea, 'mesh stl-0+stale-for-the-rig', '{}'::jsonb)" > /dev/null ||
    die "could not plant the stale export the restart check needs."
  jobs_before=$(sql "SELECT count(*) FROM job")
  # `max(id::text)`, because **there is no `max(uuid)` in Postgres** — the first draft read an empty
  # watermark, every later query failed on `uuid > ''`, and the check reported `? job(s)`, which is the
  # shape of a check that silently stopped measuring. Text is exact here and not a fudge: a uuidv7 puts
  # its timestamp in the leading hex digits, so lexicographic order on the canonical form *is* time
  # order.
  watermark=$(sql "SELECT coalesce(max(id::text), '00000000-0000-0000-0000-000000000000') FROM job")
  note "restarting the worker: one stale export planted, job rows now ${jobs_before:-?}"
  compose restart app > /dev/null 2>&1 || die "could not restart the app."
  wait_for "$WORKER_URL/api/healthz" "the restarted worker" 120 ||
    die "the worker did not come back after a restart."
  # The start-up sweep runs after the job loop is up, so health is not enough on its own: give it time
  # to do both the right thing and the wrong one. 25 s against a sweep that is one query is generous
  # by an order of magnitude, and the planted row is what proves the wait was long enough.
  sleep 25
  jobs_after=$(sql "SELECT count(*) FROM job WHERE id::text > '$watermark'")
  queued_kinds=$(sql "SELECT coalesce(string_agg(DISTINCT coalesce(payload->>'produce', kind), ', ' ORDER BY coalesce(payload->>'produce', kind)), 'none')
                        FROM job WHERE id::text > '$watermark'")
  # `${jobs_after:-99}` on purpose: an empty answer is a query that failed, and it must read as a
  # failure rather than as zero. That is not hypothetical — see the watermark note above.
  seed_check "a restarted worker queues the one stale export and nothing else" \
    "$(holds "${jobs_after:-99}" -eq 1 -a "x$queued_kinds" = "xexport_stl")" \
    "${jobs_after:-?} job(s) queued by the restart, of kind(s) ${queued_kinds:-none} — 1 export_stl is
    right, 0 means the sweep did not run at all, and anything else is a kind nothing can build"
  # Put the library back: the planted row and whatever the restart made of it, off the same watermark.
  sql "DELETE FROM job WHERE id::text > '$watermark'" > /dev/null
  sql "DELETE FROM derivative WHERE kernel_version = 'mesh stl-0+stale-for-the-rig'" > /dev/null
  # And the export the rebuild actually wrote, on that one revision. It is cache and it is rebuildable,
  # so removing it loses nothing; leaving it would make the next run of this seed die on the unique key
  # and would leave one part's storage figure different from every other part's.
  sql "DELETE FROM derivative WHERE revision_id = '$plant_rev' AND kind = 'export_stl'" > /dev/null
  echo "  the plant, its job and the export it produced are removed again"

  echo "== THE DEFAULT LIBRARY, left as the Phase 3 exit timed it"
  # The literal from `web/src/lib/api.ts`'s DEFAULT_LIBRARY_ID, seeded by migration 0002. Not
  # `value[0]`: that is whatever the list route happens to order first, and Sweep now exists.
  local default_lib=01931b6e-0000-7000-8000-000000000001 default_count
  default_count=$(curl -sf "$API/api/libraries" | field "next((l['partCount'] for l in value if l['id'] == '$default_lib'), -1)")
  # Deliberately not scanned: web/scripts/open-timing.mjs opens this library's grid, and the Phase 3
  # numbers it is compared against were measured on exactly these six example parts.
  echo "  $default_lib holds $default_count parts (the six examples, unscanned on purpose)"
  seed_check "the default library still holds its six examples" "$(holds "${default_count:-0}" -eq 6)" "$default_count"

  echo "== UPLOADED, through the upload route"
  local uploaded revised
  uploaded=$(library_named Uploaded)
  [ -n "$uploaded" ] || die "could not create the Uploaded library."
  # **Skipped when the path is already held, and that is a fix rather than an optimisation.** The
  # vee-block's path holds the *stamped* bytes after the re-upload below, so on a second seed the
  # pristine file at that same path is new bytes at an old path — which is a REVISION, correctly. So the plain upload loop was quietly adding a third revision per run and the
  # "not adding another" guard underneath it never saw them: it only ever guarded its own re-upload.
  # Measured: a stack seeded three times had three revisions on that part, and the fixture is "two".
  for name in flange-dn40-lp-3310-02 hex-spacer-m4x20-lp-2145-01 mounting-plate-lp-1180-01 vee-block-lp-3072-02; do
    local held
    held=$(sql "SELECT count(*) FROM part WHERE library_id = '$uploaded' AND source_path = 'uploads/$name.stl'")
    if [ "${held:-0}" -gt 0 ]; then
      echo "  upload $name.stl: already a part at uploads/$name.stl; not uploading over it"
      continue
    fi
    echo "  upload $name.stl: $(upload_file "$uploaded" "$ROOT/example/parts/$name.stl" "uploads/$name.stl")"
  done
  # A second revision, which is the only way to a history: a scan has no subpath so it cannot target one
  # part.
  # The stamp makes this re-runnable — the same bytes twice would be `Skipped`, not a revision.
  local vee
  vee=$(sql "SELECT id FROM part WHERE library_id = '$uploaded' AND source_path = 'uploads/vee-block-lp-3072-02.stl'")
  revised=$(curl -sf "$API/api/parts/$vee/revisions" | field 'len(value)')
  if [ "${revised:-0}" -lt 2 ]; then
    local stamped=$WORK/revised-vee-block.stl
    cp "$ROOT/example/parts/vee-block-lp-3072-02.stl" "$stamped"
    printf 'lapidary e2e rev %s' "$(date +%s)" | dd of="$stamped" bs=1 seek=0 conv=notrunc status=none
    echo "  re-upload vee-block with new bytes: $(upload_file "$uploaded" "$stamped" "uploads/vee-block-lp-3072-02.stl")"
    revised=$(curl -sf "$API/api/parts/$vee/revisions" | field 'len(value)')
  else
    # Only when it is needed: re-seeding one stack would otherwise stack up a fourth revision and a
    # fifth, and the history this fixture exists for is "two", not "however many times seed has run".
    echo "  vee-block already has a second revision; not adding another"
  fi
  echo "  vee-block now has $revised revisions"
  seed_check "an uploaded part reached a second revision" "$(holds "${revised:-0}" -ge 2)" "$revised revisions"

  echo "== OWNER, a library with its own folder: a scan, the owner's change, a re-scan (M4)"
  # `root` names a folder under the mount, and the scan reads only there. The owner then saves other
  # bytes over the file in place — the one change to the tree this rig makes, made from the host — and
  # the re-scan records it as revision 2 where it is. Neither scan may write anything under the tree.
  local owner owner_dir=$INGEST/m4-owner owner_batch owner_scanned owner_revisions before_owner
  owner=$(curl -sf "$API/api/libraries" | field "next((l['id'] for l in value if l['name'] == 'Owner'), '')")
  [ -n "$owner" ] || owner=$(json POST "$API/api/libraries" '{"name":"Owner","root":"m4-owner"}' | field 'value["id"]')
  [ -n "$owner" ] || die "could not create the Owner library."
  before_owner=$(ingest_fingerprint)
  owner_batch=$(json POST "$API/api/libraries/$owner/scan" | field 'value["batchId"]')
  owner_scanned=$(settle "$owner" "$owner_batch" 600)
  echo "  first scan: $owner_scanned"
  seed_check "a library with its own folder scans only there, and writes nothing" \
    "$(holds "$(sql "SELECT count(*) FROM part WHERE library_id = '$owner'")" -eq 1 -a "$(ingest_fingerprint)" = "$before_owner")" \
    "$(sql "SELECT string_agg(source_path, ', ') FROM part WHERE library_id = '$owner'")"
  # The owner's edit: another example's bytes saved over it, in place.
  cp "$ROOT/example/parts/hex-spacer-m4x20-lp-2145-01.stl" "$owner_dir/flange-dn40-lp-3310-02.stl"
  local edited; edited=$(ingest_fingerprint)
  owner_batch=$(json POST "$API/api/libraries/$owner/scan" | field 'value["batchId"]')
  owner_scanned=$(settle "$owner" "$owner_batch" 600)
  owner_revisions=$(sql "SELECT count(*) FROM revision r JOIN part p ON p.id = r.part_id WHERE p.library_id = '$owner'")
  echo "  after the owner's change: $owner_scanned; $owner_revisions revisions"
  seed_check "the owner's change is a revision, and recording it wrote nothing" \
    "$(holds "$(counter "$owner_scanned" revised)" -eq 1 -a "$(ingest_fingerprint)" = "$edited")" \
    "$owner_revisions revisions; $(sql "SELECT count(*) FILTER (WHERE NOT on_disk) FROM file f JOIN revision r ON r.id = f.revision_id JOIN part p ON p.id = r.part_id WHERE p.library_id = '$owner'") no longer on disk"
  seed_check "uploads land in the inbox, never under the ingest tree" \
    "$(holds "$(find "$INBOX/uploads" -type f 2>/dev/null | wc -l)" -ge 4 -a "$(sql "SELECT count(*) FROM file WHERE root = 'library' AND storage_path LIKE 'uploads/%'")" -eq 0)" \
    "$(find "$INBOX/uploads" -type f 2>/dev/null | wc -l) files under $INBOX/uploads"
  # Back to the bytes Sweep holds, so Sweep's re-scans in `drive` see the tree it was seeded from — and
  # what `drive` compares against is taken after: every move, upload and scan a flow makes must leave it
  # unchanged.
  cp "$ROOT/example/parts/flange-dn40-lp-3310-02.stl" "$owner_dir/flange-dn40-lp-3310-02.stl"
  ingest_fingerprint > "$WORK/ingest.fingerprint"

  echo "== EMPTY, created and never scanned"
  local empty empty_count
  empty=$(library_named Empty)
  empty_count=$(sql "SELECT count(*) FROM part WHERE library_id = '$empty'")
  echo "  $empty"
  # It must stay empty: it is the only fixture for the empty states, and any flow that scanned it would
  # spend the fixture and leave an 861 MiB ingest running under everything measured after it.
  seed_check "Empty is empty" "$(holds "${empty_count:-1}" -eq 0)" "$empty_count parts"

  python3 - "$WORK/seed.json" <<EOF
import json, sys
json.dump({
    "sweep": {"id": "$sweep", "parts": $parts, "pictures": ${pictured:-0}, "folders": $folders,
              "formats": [f.strip() for f in "$formats".split(",") if f.strip()],
              "scan": "$scanned"},
    "default": {"id": "$default_lib", "parts": $default_count},
    "uploaded": {"id": "$uploaded", "revisedPart": "$vee", "revisions": ${revised:-0}},
    "empty": {"id": "$empty"},
    "ingestFiles": $(find "$INGEST" -type f ! -name .seeded | wc -l),
    "checks": [
        {"name": n, "ok": ok == "1", "detail": d}
        for n, ok, d in (
            line.rstrip("\n").split("\t", 2)
            for line in open("$WORK/seed-checks.tsv")
        )
    ],
}, open(sys.argv[1], "w"), indent=2)
EOF
  # **Checked, because it was not.** This heredoc's failure was swallowed: `seed.json` is what `drive`
  # reads to address the libraries by id, and a seed that could not write one still printed its note and
  # exited 0 — a pass with no file behind it, which is the silent pass this rig exists to prevent.
  [ -s "$WORK/seed.json" ] || die "the seed's checks passed but $WORK/seed.json was not written — read
  the python traceback above. \`drive\` reads that file for the library ids, so without it a run would
  test nothing, or test whatever a re-seed happened to name."
  note "seed facts and $(wc -l < "$WORK/seed-checks.tsv") checks in $WORK/seed.json"
  [ "$seed_fail" = 0 ] || die "$seed_fail of the seed's checks failed (listed above, and in
  $WORK/seed.json). Driving a library that is not what stage 3 describes would test the wrong thing."
}

# ---------------------------------------------------------------------------------------------------
# drive

cmd_drive() {
  sign_in_rig
  local compare='' only='' stamp run
  while [ $# -gt 0 ]; do
    case $1 in
      --compare) compare=${2:?--compare needs a previous report.json}; shift 2 ;;
      --only) only=${2:?--only needs a comma-separated list of flow names}; shift 2 ;;
      *) die "unknown option for drive: $1" ;;
    esac
  done
  [ -f "$WORK/seed.json" ] || die "no $WORK/seed.json. Run \`stack.sh seed\` first — the flows address
  the seeded libraries by id."
  wait_for "$WEB/" "the web server" 30 || die "nothing answers on $LAPIDARY_PORT_WEB."

  stamp=$(date +%Y%m%dT%H%M%S)
  run=$RUNS/$stamp
  mkdir -p "$run/shots"
  note "run $stamp into $run"

  # One transcript for the whole run. No background writer inside this block: a process started with &
  # inside a `| tee` keeps the pipe open and the script never finishes (the docker stats sampler in
  # `seed` is why that is written down).
  {
    echo "== $(date -Is) drive $PROJECT at $(git -C "$ROOT" rev-parse --short HEAD)"
    LAPIDARY_E2E_COOKIE=$(rig_cookie) node "$ROOT/scripts/e2e/flows.mjs" \
      --out "$run" --web "$WEB" --api "$API" --seed "$WORK/seed.json" ${only:+--only "$only"}
    echo "flows exit $?"
    # **Skipped under `--only`, for the reason `flows.mjs` skips its own extra sessions**: `--only` means
    # run one flow, not open twelve more browsers. It is not only slow — it is twelve 60-second waits
    # when the thing under test is a viewer that does not paint, which is exactly when somebody reaches
    # for `--only`. Measured: a `--only view-original` run against a deliberately broken viewer spent
    # 12 minutes here after the flow had already failed in one second.
    if [ -n "$only" ]; then
      echo "== TIMING skipped (--only $only)"
    else
      echo "== TIMING (web/scripts/open-timing.mjs, not reimplemented here)"
      # --url is explicit: its default is the owner's localhost:3000.
      LAPIDARY_E2E_COOKIE=$(rig_cookie) node "$ROOT/web/scripts/open-timing.mjs" --url "$WEB" --rounds 2
    fi
  } 2>&1 | tee "$run/run.log"

  python3 - "$run" "$WORK/stack.json" "$WORK/seed.json" "$(git -C "$ROOT" rev-parse HEAD)" <<'EOF'
import json, pathlib, sys
run, stack, seed = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
def load(p, default=None):
    try: return json.loads(p.read_text())
    except Exception: return default
log = (run / "run.log").read_text(errors="replace")
timing = log.split("== TIMING", 1)[1] if "== TIMING" in log else ""
# What the flows process itself said. `flows.json` is written at the end of a run, so anything that kills
# flows.mjs outside a flow — Chrome refusing a debugging port, `session()` throwing for the narrow or
# reduced-motion pass, `--only` matching nothing — leaves no rows at all. An empty list must therefore be a
# failure and not an empty success: that is the same silent pass as comparing two `undefined`s, one level up.
exit_line = [l for l in log.splitlines() if l.startswith("flows exit ")]
flows_exit = int(exit_line[-1].split()[-1]) if exit_line else 1
# Three shas, and `--compare` is only readable if they are told apart. `stack.sha` is the tree the app was
# brought up from and `builtFromSha` the tree its images were built from; `driveSha` is the tree the *rig*
# ran from, which is what changes when a flow is edited between two runs. Comparing two reports whose
# driveSha differs compares two different harnesses, and calling that flakiness would be wrong.
report = {
    "driveSha": sys.argv[4],
    "stack": load(stack, {}),
    "seed": load(seed, {}),
    "flows": load(run / "flows.json", []),
    "timing": [l for l in timing.splitlines() if l.strip() and not l.startswith("==")],
}
(run / "report.json").write_text(json.dumps(report, indent=2))
flows = report["flows"]
bad = [f["name"] for f in flows if f.get("status") not in ("ok", "pending")]
if not flows:
    print("FAILED: the flows wrote no rows at all — flows.mjs died before any flow ran; see run.log")
if flows_exit != 0:
    print(f"FAILED: flows.mjs exited {flows_exit}")
print(f"\n{len(flows)} flows: " + ", ".join(
    f"{s}={sum(1 for f in flows if f.get('status') == s)}"
    for s in sorted({f.get("status") for f in flows})))
print("report " + str(run / "report.json"))
if bad: print("FAILED: " + ", ".join(bad))
# The exit code IS the result: `drive` is what a later goal runs in CI, and a run that failed a flow
# must not answer 0. The flows process exits non-zero too, but its status is swallowed by the tee.
sys.exit(0 if (not bad and flows and flows_exit == 0) else 1)
EOF
  local code=$?
  # (M4) Nothing a flow did — the scans, the moves, the uploads, the deletes — wrote under the ingest
  # tree. Compared from the host, against what the seed left.
  if [ -f "$WORK/ingest.fingerprint" ]; then
    if [ "$(ingest_fingerprint)" = "$(cat "$WORK/ingest.fingerprint")" ]; then
      echo "  the ingest tree is exactly as the seed left it: nothing under it was written"
    else
      echo "FAILED: something wrote under $INGEST during the flows — its fingerprint changed"
      code=1
    fi
  fi

  if [ -n "$compare" ]; then
    echo "== COMPARE against $compare"
    python3 - "$compare" "$run/report.json" <<'EOF'
import json, pathlib, re, sys
old, new = (json.loads(pathlib.Path(p).read_text()) for p in sys.argv[1:3])
def by_name(r): return {f["name"]: f for f in r.get("flows", [])}
a, b = by_name(old), by_name(new)
for label, report in (("was", old), ("now", new)):
    stack = report.get("stack", {})
    print(f"  {label}: rig {str(report.get('driveSha'))[:12]}, app {str(stack.get('sha'))[:12]} on "
          f"{stack.get('branch')}, images from {str(stack.get('builtFromSha'))[:12]}")
same_rig = old.get("driveSha") == new.get("driveSha")
same_app = old.get("stack", {}).get("sha") == new.get("stack", {}).get("sha")
if same_rig and same_app:
    print("  same rig and same app both runs, so a status that moved is flakiness, not a regression")
elif not same_rig:
    print("  the RIG differs between these runs, so a status that moved may be the flow's own change")
else:
    print("  the app differs between these runs, so a status that moved is a regression signal")
for name in sorted(set(a) | set(b)):
    was, now = a.get(name, {}), b.get(name, {})
    ws, ns = was.get("status", "absent"), now.get("status", "absent")
    wm, nm = was.get("ms"), now.get("ms")
    mark = "  " if ws == ns else "! "
    delta = "" if wm is None or nm is None else f"  {nm - wm:+d} ms ({wm} -> {nm})"
    print(f"{mark}{name}: {ws} -> {ns}{delta}")
num = re.compile(r"median ([\d.]+) ms")
for line, other in zip(new.get("timing", []), old.get("timing", [])):
    m, n = num.search(other), num.search(line)
    if m and n:
        print(f"  timing {line.split(':')[0].strip()}: {float(n.group(1)) - float(m.group(1)):+.1f} ms median")
EOF
  fi
  return $code
}

# ---------------------------------------------------------------------------------------------------
# down

# What `down` keeps, and why.
#
# `runs/` is the record — stage 6's `--compare` reads a previous `report.json`, and the goal's Record
# quotes numbers out of them — and `ingest/` is 861 MiB that takes minutes to rebuild from the corpus.
# Neither is root-owned, so neither can stop `scripts/release-goal.sh` removing the worktree, which is
# what stage 2's exit was actually protecting. They are printed with their size so nobody meets that
# 861 MiB by surprise, and `down --purge` sweeps the floor for somebody who wants it swept.
cmd_down() {
  local purge=0
  if [ "${1:-}" = --purge ]; then purge=1; shift; fi
  if [ "${1:-}" = --keep ]; then
    note "--keep: leaving $PROJECT up and $STORE in place. Tear it down with \`stack.sh down\` when the
  interactive pass is finished; until then this lane holds ports $LAPIDARY_PORT_WEB/$LAPIDARY_PORT_API."
    return 0
  fi
  assert_project
  note "down $PROJECT"
  # The reverse chown FIRST, while a container can still be run and the store still exists. Skipped
  # when there is nothing there, so a second `down` is not an error.
  if [ -d "$STORE" ] || [ -d "$INBOX" ]; then chown_store "$(id -u):$(id -g)"; fi
  if [ -f "$ENVFILE" ]; then
    # -v removes THIS project's volumes and no others: lapidary-e2e-<lane>_lapidary-db and friends. The
    # owner's lapidary_lapidary-db is a different project, which is what the assertion above protects.
    #
    # PIPESTATUS, not $?: piping into `tail` would otherwise hide a failed teardown behind a successful
    # tail, and the two lines below would then delete the password of a database volume that survived.
    compose down -v --remove-orphans 2>&1 | tail -4
    [ "${PIPESTATUS[0]}" = 0 ] || die "\`compose down -v\` failed for $PROJECT. Nothing was deleted —
  $ENVFILE still holds the password its database volume was initialised with, and removing it would
  make the next \`up\` fail authentication instead. Fix the teardown, then run \`down\` again."
  else
    echo "  no $ENVFILE, so no compose project to remove"
  fi
  remove_store
  # These describe a stack that no longer exists, and `drive` reads seed.json to address the libraries by
  # id. Left behind, they point at ids the next database has never heard of — so `drive` would run against
  # 404s, or against whatever a re-seed happened to give those names, and report it as flow failures. They
  # go with the database that made them; `runs/` stays, because a finished run's report is still true.
  rm -f "$WORK/seed.json" "$WORK/stack.json" "$WORK/seed-checks.tsv"
  # Last, and only once the volumes are gone: while a db volume survives, its password must too.
  rm -f "$ENVFILE"
  if [ "$purge" = 1 ]; then
    # The same path gate as the store: a variable that is set and wrong is not caught by `set -u`.
    for doomed in "$RUNS" "$INGEST"; do
      case "$doomed" in
        "$ROOT"/target/e2e/[0-4]/runs | "$ROOT"/target/e2e/[0-4]/ingest) rm -rf "$doomed" ;;
        "$ROOT"/target/e2e/[0-4][abc]/runs | "$ROOT"/target/e2e/[0-4][abc]/ingest) rm -rf "$doomed" ;;
        *) die "refusing to remove \`$doomed\`: that is not target/e2e/<lane>[a|b|c]/runs or /ingest." ;;
      esac
    done
    echo "  --purge: removed runs/ and ingest/ as well"
  fi

  untag_borrowed_images
  echo "  removed: $(docker volume ls --format '{{.Name}}' | grep -c "^${PROJECT}_" || true) of this project's volumes remain, $(docker ps -a --format '{{.Names}}' | grep -c "^${PROJECT}-" || true) of its containers"
  echo "  the owner's volumes, untouched: $(docker volume ls --format '{{.Name}}' | grep -c '^lapidary_lapidary-' || true) of 2"
  # What survives, and how big it is. `runs/` is the regression record and `ingest/` is the corpus slice;
  # both are host-owned, so neither blocks `release-goal.sh` removing the worktree.
  local kept=0
  for keep in "$RUNS" "$INGEST"; do
    if [ -d "$keep" ]; then
      kept=1
      printf '  kept: %s — %s, %s file(s)%s\n' \
        "${keep#"$ROOT"/}" \
        "$(du -sh --apparent-size "$keep" 2>/dev/null | cut -f1)" \
        "$(find "$keep" -type f 2>/dev/null | wc -l)" \
        "$([ "$keep" = "$RUNS" ] && echo ' (the regression record: drive --compare reads these)' || echo ' (the corpus slice: minutes to rebuild)')"
    fi
  done
  [ "$kept" = 1 ] && echo "  \`stack.sh down --purge\` removes those two as well."
  # The guarantee stage 2 is actually after: nothing here is root-owned, so the worktree can be removed.
  local rooted
  rooted=$(find "$WORK" ! -user "$(id -u)" 2>/dev/null | head -3)
  if [ -n "$rooted" ]; then
    die "these are not owned by $(id -un), so scripts/release-goal.sh could not remove this worktree:
$rooted"
  fi
  echo "  nothing under $WORK is owned by another user, so the worktree is removable"
}

# ---------------------------------------------------------------------------------------------------
# status

cmd_status() {
  echo "project  $PROJECT (lane $LANE${AS:+, installation $AS})"
  echo "port     app $LAPIDARY_PORT_WEB"
  echo "work     $WORK"
  echo "disk     / $(free_gb /) GB, $ROOT $(free_gb "$ROOT") GB; RAM available $(ram_gib) GiB"
  echo "images   $(or_none bash -c "docker images --format '{{.Repository}} {{.Size}}' | grep '^$PROJECT-'")"
  echo "volumes  $(or_none bash -c "docker volume ls --format '{{.Name}}' | grep '^${PROJECT}_'")"
  if [ -f "$ENVFILE" ]; then
    # The same assertion `build` and `up` run, so `status` answers "would this bind the right ports?"
    # without starting anything. It needs no daemon: `compose config` only resolves the files.
    verify_ports
    compose ps --format '{{.Service}}\t{{.Status}}\t{{.Publishers}}' 2>/dev/null || echo "compose ps failed"
    echo "health   api $(curl -so /dev/null -w '%{http_code}' "$API/api/healthz") web $(curl -so /dev/null -w '%{http_code}' "$WEB/")"
  else
    echo "no $ENVFILE — nothing has been brought up in this lane"
  fi
  echo "runs     $(or_none ls -1 "$RUNS")"
}

# ---------------------------------------------------------------------------------------------------

case "${1:-}" in
  build) shift; cmd_build "$@" ;;
  up) shift; cmd_up "$@" ;;
  seed) shift; cmd_seed "$@" ;;
  drive) shift; cmd_drive "$@" ;;
  down) shift; cmd_down "$@" ;;
  status) shift; cmd_status "$@" ;;
  *)
    echo "usage: scripts/e2e/stack.sh <build | up | seed | drive | down | status>" >&2
    echo "  drive [--compare <report.json>] [--only <flow,flow>]    down [--keep | --purge]" >&2
    echo "  up [--min-ram <gib>]" >&2
    echo "  AS=a|b|c for one of three installations on this lane's block" >&2
    exit 2
    ;;
esac
