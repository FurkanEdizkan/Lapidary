#!/usr/bin/env bash
# The whole application, up and driven: one compose stack on this lane's port block, a library worth
# testing seeded into it, and a real browser over the real api.
#
#   scripts/e2e/stack.sh build            # deploy/'s five images, one service at a time, disk-guarded
#   scripts/e2e/stack.sh up               # bring it up and wait until every role answers
#   scripts/e2e/stack.sh seed             # the ingest tree and the four libraries
#   scripts/e2e/stack.sh drive [--compare <report.json>] [--only <flow,flow>]
#   scripts/e2e/stack.sh exit2 [<part.stl> …]    # Phase 6 exit 2, with the numbers behind the verdict
#   scripts/e2e/stack.sh down [--keep|--purge]   # reverse the chown, remove the project and the store
#   scripts/e2e/stack.sh status
#
# Sharing needs more than one installation, and the introductions need three. `AS=a|b|c` is which of the
# three this call is: it suffixes the compose project, the work directory and the images, and shifts this
# lane's four ports by 100 per letter, so the three sit inside one lane's block and cannot reach another
# lane's. `scripts/e2e/group.sh` is the three of them together.
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
# Never `docker builder prune`: the OCCT stage is 30–40 minutes to rebuild. Never `docker volume prune`,
# never `docker system prune`, and never remove an image this project did not tag.
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
# A lane owns one port block and this rig brought up one stack in it. Sharing takes two installations and
# the introductions take three, so `AS` names which one this call is. It suffixes the compose project (and
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
RUNS=$WORK/runs
WEB=http://127.0.0.1:$LAPIDARY_PORT_WEB
API=http://127.0.0.1:$LAPIDARY_PORT_API
WORKER_URL=http://127.0.0.1:$LAPIDARY_PORT_WORKER
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
  LAPIDARY_STORAGE_ROOT LAPIDARY_INGEST_DIR \
  LAPIDARY_PORT_WEB LAPIDARY_PORT_API LAPIDARY_PORT_WORKER LAPIDARY_PORT_PEER \
  LAPIDARY_WORKER_CONCURRENCY LAPIDARY_JOB_LEASE_SECS LAPIDARY_KERNEL_TIMEOUT \
  LAPIDARY_JOB_POLL_SECS LAPIDARY_WORKER_ID; do
  COMPOSE_UNSET+=(-u "$var")
done

# The only place in this rig that runs `docker compose`. `-p` is not a parameter.
compose() {
  assert_project
  [ -f "$ENVFILE" ] || die "$ENVFILE does not exist yet. Run \`stack.sh up\` (it writes it) first."
  env "${COMPOSE_UNSET[@]}" docker compose -p "$PROJECT" \
    -f "$ROOT/deploy/compose.yaml" \
    -f "$ROOT/deploy/compose.sharing.yaml" \
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
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$WORK/seed-checks.tsv"
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

# A batch's counters until nothing is pending or running. check-plain.sh's settle, with `revised` and
# `unkept` kept: those two are how a controlled library's second revision and a hobby library's refusal
# to take new bytes at an old path tell themselves apart.
settle() { # library, batch, seconds
  local line began; began=$(date +%s)
  [ -n "${2:-}" ] || { echo "no batch id to follow"; return 1; }
  while [ $(($(date +%s) - began)) -lt "${3:-3600}" ]; do
    line=$(curl -sf "$API/api/libraries/$1/jobs/$2" |
      field '" ".join(f"{k}={value[k]}" for k in ["total","pending","running","ingested","skipped","revised","unkept","failedTotal"])' 2>/dev/null)
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
# The five images belong to the lane, built once by `build` from the lane's own checkout. A suffixed
# installation borrows them under its own project's names, because compose looks for `<project>-<service>`
# when it is told not to build: `docker tag` names the same layers a second time, so three installations
# cost one build and not a byte of disk. Never the other way round — `build` refuses a suffix.
IMAGE_PROJECT=lapidary-e2e-$LANE

tag_borrowed_images() {
  [ -n "$AS" ] || return 0
  local svc
  for svc in db api worker web peer; do
    docker image inspect "$IMAGE_PROJECT-$svc" > /dev/null 2>&1 || die "there is no \`$IMAGE_PROJECT-$svc\`
  image for $PROJECT to borrow. Build this lane's five images first, with no AS set:
  \`cargo xtask heavy -- scripts/e2e/stack.sh build\`."
    docker tag "$IMAGE_PROJECT-$svc" "$PROJECT-$svc" ||
      die "could not tag $IMAGE_PROJECT-$svc as $PROJECT-$svc."
  done
  echo "  $PROJECT's five images are tags of $IMAGE_PROJECT-*: same layers, nothing built"
}

# Only ever the tags this project put on, and only when there is a suffix — the lane's own five images are
# what every installation is made of, and a `down` that removed them would cost the next one a rebuild.
untag_borrowed_images() {
  [ -n "$AS" ] || return 0
  local svc removed=0
  for svc in db api worker web peer; do
    if docker image inspect "$PROJECT-$svc" > /dev/null 2>&1; then
      docker rmi "$PROJECT-$svc" > /dev/null 2>&1 && removed=$((removed + 1))
    fi
  done
  echo "  removed $removed of $PROJECT's borrowed image tags; $IMAGE_PROJECT-* kept"
}

chown_store() { # owner, e.g. 10001:10001
  local image=''
  for candidate in "$PROJECT-api" "$PROJECT-worker" docker.io/library/postgres:18; do
    docker image inspect "$candidate" > /dev/null 2>&1 && { image=$candidate; break; }
  done
  [ -n "$image" ] || { echo "  no local image to chown with; leaving $STORE as it is"; return 1; }
  mkdir -p "$STORE"
  docker run --rm --pull never --user 0 --entrypoint chown \
    -v "$STORE:/var/lib/lapidary" "$image" -R "$1" /var/lib/lapidary > /dev/null &&
    echo "  $STORE now owned by $1 (through $image)"
}

# `rm -rf` on a path built from variables gets its own gate. `set -u` is not a safety net for a
# variable that is set and wrong.
remove_store() {
  case "$STORE" in
    "$ROOT"/target/e2e/[0-4]/store | "$ROOT"/target/e2e/[0-4][abc]/store) ;;
    *) die "refusing to remove \`$STORE\`: that is not target/e2e/<lane>[a|b|c]/store." ;;
  esac
  [ -e "$STORE" ] || return 0
  rm -rf "$STORE" || die "could not remove $STORE. If it is still root-owned the reverse chown above
  failed — fix that before the lead runs scripts/release-goal.sh, or the worktree cannot be deleted."
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
LAPIDARY_INGEST_DIR=$INGEST
# Interpolated into scripts/e2e/e2e.override.yaml, which is the only thing that moves the published
# ports off deploy/compose.yaml's literals.
LAPIDARY_PORT_WEB=$LAPIDARY_PORT_WEB
LAPIDARY_PORT_API=$LAPIDARY_PORT_API
LAPIDARY_PORT_WORKER=$LAPIDARY_PORT_WORKER
LAPIDARY_PORT_PEER=$LAPIDARY_PORT_PEER
EOF
}

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
    STORE=$STORE INGEST=$INGEST python3 -c '
import json, os, sys

resolved = json.load(sys.stdin)
want = {os.environ["LAPIDARY_PORT_" + k] for k in ("WEB", "API", "WORKER", "PEER")}
published = set()
mounts = {}
for name, service in resolved.get("services", {}).items():
    for port in service.get("ports") or []:
        published.add(str(port.get("published")))
    for volume in service.get("volumes") or []:
        if volume.get("type") == "bind":
            mounts.setdefault(volume["source"], set()).add(name)

problems = []
if published != want:
    problems.append(f"published host ports are {sorted(published)}; this lane owns {sorted(want)}")
for forbidden in ("3000", "8080", "8081", "8082"):
    if forbidden in published:
        problems.append(f"{forbidden} is published — that is deploy/compose.yaml`s literal, and the owner`s")
for path in (os.environ["STORE"], os.environ["INGEST"]):
    if path not in mounts:
        problems.append(f"nothing bind-mounts {path}; the env file is not the one compose read")
for path in mounts:
    if not path.startswith(os.path.dirname(os.environ["STORE"])):
        problems.append(f"{path} is bind-mounted from outside this lane`s target/e2e directory")
print("  ports " + ", ".join(sorted(published)) + "; bind mounts " + ", ".join(sorted(mounts)))
for problem in problems:
    print("  " + problem, file=sys.stderr)
sys.exit(1 if problems else 0)
' <<< "$resolved" || die "the resolved compose configuration is not this lane's. Nothing was started."
}

cmd_build() {
  [ -z "$AS" ] || die "an installation with AS=$AS borrows the lane's images; it never builds its own.
  Build once with no AS set (\`cargo xtask heavy -- scripts/e2e/stack.sh build\`), then bring each
  installation up — \`up\` tags the lane's five images under this project's names."
  local services=${SERVICES:-db web api peer worker}
  write_env
  verify_ports
  local summary=$WORK/build-summary.txt
  echo "== $(date -Is) build of $PROJECT from $(git -C "$ROOT" rev-parse --short HEAD); / $(free_gb /) GB free; /mnt/Storage $(free_gb "$ROOT") GB free" > "$summary"
  git -C "$ROOT" rev-parse HEAD > "$WORK/built-from.sha"
  for svc in $services; do
    # The guard, before each service and not once at the top: the OCCT worker stage alone can move
    # root free space by several gigabytes. Under it we stop and ask — we never prune, because
    # `docker builder prune` drops the cached OCCT layer and costs a 30-40 minute rebuild.
    [ "$(free_gb /)" -ge 8 ] || ask_owner "/ has $(free_gb /) GB free, under the 8 GB this build needs,
  before service \`$svc\`. This rig never prunes: \`docker builder prune\` would drop the cached OCCT
  layer and cost a 30-40 minute rebuild. Free space elsewhere, or say to go ahead."
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
  [ "$(free_gb /)" -ge 6 ] || ask_owner "/ has $(free_gb /) GB free, under the 6 GB this stack needs."
  [ "$min_ram" = 5 ] ||
    note "--min-ram $min_ram: brought up under the 5 GiB default on purpose. Say why in the goal file."
  # The declared ceilings in deploy/ total 4.3 GB. A stack brought up beside a cargo build is how a
  # session gets its processes killed; 5 GiB available is the floor that leaves the stack room.
  [ "$(ram_gib)" -ge "$min_ram" ] || ask_owner "$(ram_gib) GiB of RAM is available, under the $min_ram GiB
  floor. This stack's declared ceilings total 4.3 GB (db 1g, worker 2g, api 512m, peer 512m, web 256m).
  Wait until the other lanes have stopped compiling, then run this again — or, for installations that are
  never seeded, say \`--min-ram <gib>\` and record the number you chose."

  write_env
  # Before the directories exist, so a wrong bind source is caught before docker creates it as root.
  verify_ports
  mkdir -p "$STORE" "$INGEST" "$RUNS"
  tag_borrowed_images
  chown_store 10001:10001

  note "up"
  local began=$SECONDS
  compose up -d --no-build 2>&1 | tail -8
  wait_for "$API/api/healthz" "the api" || die "the api never became healthy. \`stack.sh status\` and
  \`docker compose -p $PROJECT logs api\` say why."
  wait_for "$WORKER_URL/api/healthz" "the worker" || die "the worker never became healthy."
  wait_for "$WEB/" "the web server" || die "the web server never answered on $LAPIDARY_PORT_WEB."
  # The peer role claims this installation's identity on first start. Its absence is how goal 9's run
  # found a peer that had come up without its key volume.
  until_true "$API/api/sharing/identity" 'value["deviceId"] is not None' \
    "the peer role claimed a device id" 240 || die "no device id: the peer role never came up."

  local secs=$((SECONDS - began))
  local extensions migration device
  extensions=$(sql "SELECT string_agg(extname || ' ' || extversion, ', ' ORDER BY extname) FROM pg_extension")
  migration=$(sql "SELECT max(version) || ' of ' || count(*) FROM _sqlx_migrations")
  device=$(curl -sf "$API/api/sharing/identity" | field 'value["deviceId"]')
  note "healthy after ${secs} s"
  echo "  services: $(compose ps --format '{{.Service}} {{.Status}}' | paste -sd'; ' -)"
  echo "  extensions: $extensions"
  echo "  newest migration: $migration"
  echo "  device id: $device"
  echo "  ports: web $LAPIDARY_PORT_WEB, api $LAPIDARY_PORT_API, worker $LAPIDARY_PORT_WORKER, peer $LAPIDARY_PORT_PEER"

  python3 - "$WORK/stack.json" <<EOF
import json, sys
json.dump({
    "project": "$PROJECT", "lane": $LANE, "upSeconds": $secs,
    "ports": {"web": $LAPIDARY_PORT_WEB, "api": $LAPIDARY_PORT_API,
              "worker": $LAPIDARY_PORT_WORKER, "peer": $LAPIDARY_PORT_PEER},
    "extensions": """$extensions""", "migration": """$migration""",
    "deviceId": "$device",
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
  find "$INGEST" -mindepth 1 -delete
  mkdir -p "$INGEST/step" "$INGEST/misc" "$INGEST/alike"

  # step/: the six B-rep fixtures, which is what sends work through the OCCT worker at all.
  cp "$ROOT"/fixtures/step/* "$INGEST/step/" || die "fixtures/step is not where it was."
  # misc/: the two non-STL mesh formats, so the format facet has five values and not three.
  cp "$ROOT"/fixtures/*.obj "$ROOT"/fixtures/*.3mf "$INGEST/misc/" || die "the .obj/.3mf fixtures moved."

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

  # alike/: the three deliberate duplicate cases, beside the part they are duplicates *of* — a
  # duplicate needs both halves in one library to be one.
  local flange=$ROOT/example/parts/flange-dn40-lp-3310-02.stl
  cp "$flange" "$INGEST/alike/flange-dn40-lp-3310-02.stl"
  # Identical: the same bytes at a second path. `PgBlobs::library_holds` joins on source_path *and*
  # hash, so this is a second part rather than a `Skipped` job — which is precisely the case a
  # duplicate finder has to catch and an ingest-time hash check cannot.
  cp "$flange" "$INGEST/alike/flange-dn40-lp-3310-02-second-copy.stl"
  # Near-duplicate: the same solid stood on a different axis. docs/phase-6.md claims exactly this
  # invariance, so it is exactly what the fixture should be.
  python3 "$ROOT/scripts/e2e/skew-stl.py" --rotate "$flange" "$INGEST/alike/flange-dn40-lp-3310-02-rotated.stl" ||
    die "skew-stl.py --rotate failed."
  # Similar but not near: 15 % larger, far outside the ln(1.02) band by design. A detector that calls
  # this one a near-duplicate is wrong, and this is how we would find out.
  python3 "$ROOT/scripts/e2e/skew-stl.py" --scale 1.15 "$flange" "$INGEST/alike/flange-dn40-lp-3310-02-scaled-115.stl" ||
    die "skew-stl.py --scale failed."

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
  seen=$(compose exec -T worker sh -c 'find /ingest -type f ! -name .seeded | wc -l' 2>/dev/null | tr -dc '0-9')
  [ "${seen:-0}" = "$host_files" ] || die "the host has $host_files files under $INGEST but the worker sees
  ${seen:-0} in /ingest, so the bind mount is not showing this directory and every scan will find nothing.
  A bind mount follows the inode, not the path: if anything replaced the directory rather than its
  contents, only \`stack.sh down\` then \`up\` reattaches it. Compare \`stat -c %i $INGEST\` with
  \`compose exec -T worker stat -c %i /ingest\` to confirm that is what happened."
  echo "  the worker sees all $seen of them in /ingest"
}

# A library by name, made if it is not there. Names, not ids, so `seed` is re-runnable.
library_named() { # name, mode
  local existing
  existing=$(curl -sf "$API/api/libraries" | field "next((l['id'] for l in value if l['name'] == $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1")), '')")
  if [ -n "$existing" ]; then echo "$existing"; return 0; fi
  json POST "$API/api/libraries" "{\"name\":$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"),\"mode\":\"$2\"}" | field 'value["id"]'
}

# The chunked upload, following the plan the server answers rather than sending regardless.
#
# `POST /uploads/probe` answers `UploadPlan { have, needRows, needBytes }`, and those three cases are the
# whole protocol: `have` means this library already holds these bytes at this path and there is nothing to
# do; `needRows` means the store has the bytes (some other library ingested them) so only a row is needed
# and **no transfer at all**; `needBytes` means send them. check-plain.sh sent the chunks unconditionally
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

  case "$(printf '%s' "$plan" | field 'next((k for k in ("have", "needRows", "needBytes") if value.get(k)), "?")')" in
    have)
      echo "already here (the library holds these bytes at this path)"
      return 0
      ;;
    needRows)
      # The bytes are already in the store, so commit alone is right: sending them again would be the
      # transfer the content-addressed store exists to avoid.
      echo -n "bytes already stored, row only: "
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
      echo "  the probe named neither have, needRows nor needBytes for $path: $plan"
      return 1
      ;;
  esac

  commit=$(json POST "$API/api/libraries/$lib/uploads/commit" "$manifest") ||
    { echo "  commit refused for $path"; return 1; }
  settle "$lib" "$(echo "$commit" | field 'value["batchId"]')" 600
}

cmd_seed() {
  wait_for "$API/api/healthz" "the api" 30 || die "nothing is up. Run \`stack.sh up\` first."
  echo "== INGEST TREE"
  build_ingest_tree

  echo "== SWEEP (hobby), the whole tree"
  local sweep began batch
  sweep=$(library_named Sweep hobby)
  [ -n "$sweep" ] || die "could not create the Sweep library."
  # One scan is the whole ingest directory into one library: POST /scan takes no subpath.
  began=$SECONDS
  batch=$(json POST "$API/api/libraries/$sweep/scan" | field 'value["batchId"]')
  # The worker's memory while the STEP files go through the kernel. Sampled into a file, never into
  # this script's stdout: a background writer on a `tee` pipe keeps it open and the caller hangs.
  ( while true; do docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' "$PROJECT-worker-1" 2>/dev/null; sleep 2; done ) > "$WORK/worker-stats.txt" 2>/dev/null &
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
  # Stage 3 said "no failures". That is not what a slice of 400 real downloaded files can promise: one of
  # them has a non-finite coordinate at triangle 49957 and the application says so, precisely and
  # actionably, which is the behaviour we want rather than a regression. So the check is that failures
  # stay within the one known-bad file **and that every one of them explains itself** — a failure with an
  # empty reason would break this repository's own rule that errors say what broke and what to do.
  seed_check "at most the one known-corrupt file fails" "$(holds "${failed_total:-99}" -le 1)" \
    "failedTotal=${failed_total:-none} of $(counter "$scanned" total)"
  seed_check "every failure explains itself" "$(holds "${unexplained:-99}" -eq 0)" \
    "${unexplained:-?} of ${failed_total:-?} failures carry no reason"

  local parts thumbs folders formats
  parts=$(sql "SELECT count(*) FROM part WHERE library_id = '$sweep' AND deleted_at IS NULL")
  local thumb_sql="SELECT count(DISTINCT p.id) FROM part p JOIN revision r ON r.part_id = p.id JOIN derivative d ON d.revision_id = r.id WHERE p.library_id = '$sweep' AND d.kind = 'thumbnail'"
  # Waited for, not sampled: thumbnails are their own jobs and finish after the ingest batch settles, so
  # counting once would fail on a library that is perfectly fine thirty seconds later.
  local waited=0
  thumbs=$(sql "$thumb_sql")
  while [ "${thumbs:-0}" -lt "${parts:-1}" ] && [ "$waited" -lt 600 ]; do
    sleep 5
    waited=$((waited + 5))
    thumbs=$(sql "$thumb_sql")
  done
  folders=$(curl -sf "$API/api/libraries/$sweep/folders" | python3 -c 'import json,sys
def count(nodes): return sum(1 + count(n.get("children") or []) for n in nodes)
print(count(json.load(sys.stdin)))')
  formats=$(curl -sf "$API/api/libraries/$sweep/facets" | field '", ".join(f["value"] for f in value["formats"])')
  echo "  parts $parts, thumbnails $thumbs (after ${waited} s), folders $folders, formats: $formats"
  # 400 corpus + 6 step + 2 misc + 4 alike = 412, less the one corrupt file = 411; 409 is stage 3's floor.
  # Asserted on what the library holds, not on this batch's `ingested`: a second seed against a library
  # that already has the tree ingests nothing new and would fail a per-batch check while being correct.
  seed_check "the library holds at least 409 parts" "$(holds "${parts:-0}" -ge 409)" "$parts parts"
  seed_check "a thumbnail for every part" "$(holds "${thumbs:-0}" -eq "${parts:-1}")" "$thumbs of $parts"
  seed_check "at least 8 categories" "$(holds "${folders:-0}" -ge 8)" "$folders"
  local missing=''
  for want in stl step igs obj 3mf; do
    case ",${formats// /}," in *",$want,"*) ;; *) missing="$missing $want" ;; esac
  done
  seed_check "five formats present" "$(holds -z "\"$missing\"")" "${formats:-none}${missing:+ (missing$missing)}"

  echo "  re-scan settles all-skipped:"
  local again rescan
  again=$(json POST "$API/api/libraries/$sweep/scan" | field 'value["batchId"]')
  rescan=$(settle "$sweep" "$again" 3600)
  echo "    $rescan"
  seed_check "the re-scan adds nothing" \
    "$(holds "$(counter "$rescan" ingested)" -eq 0 -a "$(counter "$rescan" skipped)" -ge 409)" \
    "$rescan (the corrupt file is retried and fails again, which is why skipped is one short of total)"

  # The PMI cylinder's detail, check-plain.sh's assertions kept: this is what proves the real kernel
  # ran and not the mock.
  local pmi_part detail
  pmi_part=$(sql "SELECT id FROM part WHERE library_id = '$sweep' AND source_path LIKE '%pmi%' LIMIT 1")
  if [ -n "$pmi_part" ]; then
    detail=$(curl -sf "$API/api/parts/$pmi_part")
    local pmi_ok
    pmi_ok=$(echo "$detail" | field 'int(all(value.get(k) is not None for k in ("structure", "entities", "pmi", "kernelVersion")))')
    seed_check "the real kernel read the PMI cylinder" "${pmi_ok:-0}" \
      "format $(echo "$detail" | field 'value["sourceFormat"]'), kernel $(echo "$detail" | field 'value["kernelVersion"]'), structure/entities/pmi $(echo "$detail" | field '[value.get(k) is not None for k in ("structure","entities","pmi")]')"
  else
    seed_check "the real kernel read the PMI cylinder" 0 "no part matched '%pmi%' in Sweep — the STEP scan did not land"
  fi

  echo "== THE DEFAULT LIBRARY, left as the Phase 3 exit timed it"
  # The literal from `web/src/lib/api.ts`'s DEFAULT_LIBRARY_ID, seeded by migration 0002. Not
  # `value[0]`: that is whatever the list route happens to order first, and Sweep now exists.
  local default_lib=01931b6e-0000-7000-8000-000000000001 default_count
  default_count=$(curl -sf "$API/api/libraries" | field "next((l['partCount'] for l in value if l['id'] == '$default_lib'), -1)")
  # Deliberately not scanned: web/scripts/open-timing.mjs opens this library's grid, and the Phase 3
  # numbers it is compared against were measured on exactly these six example parts.
  echo "  $default_lib holds $default_count parts (the six examples, unscanned on purpose)"
  seed_check "the default library still holds its six examples" "$(holds "${default_count:-0}" -eq 6)" "$default_count"

  echo "== GOVERNED (controlled), through the upload route"
  local governed revised
  governed=$(library_named Governed controlled)
  [ -n "$governed" ] || die "could not create the Governed library."
  for name in flange-dn40-lp-3310-02 hex-spacer-m4x20-lp-2145-01 mounting-plate-lp-1180-01 vee-block-lp-3072-02; do
    echo "  upload $name.stl: $(upload_file "$governed" "$ROOT/example/parts/$name.stl" "uploads/$name.stl")"
  done
  # A second revision, which is the only way to a history, a diff and a lock: a scan has no subpath so
  # it cannot target one part, and a hobby library answers `Unkept` to new bytes at a path it holds.
  # The stamp makes this re-runnable — the same bytes twice would be `Skipped`, not a revision.
  local vee
  vee=$(sql "SELECT id FROM part WHERE library_id = '$governed' AND source_path = 'uploads/vee-block-lp-3072-02.stl'")
  revised=$(curl -sf "$API/api/parts/$vee/revisions" | field 'len(value)')
  if [ "${revised:-0}" -lt 2 ]; then
    local stamped=$WORK/revised-vee-block.stl
    cp "$ROOT/example/parts/vee-block-lp-3072-02.stl" "$stamped"
    printf 'lapidary e2e rev %s' "$(date +%s)" | dd of="$stamped" bs=1 seek=0 conv=notrunc status=none
    echo "  re-upload vee-block with new bytes: $(upload_file "$governed" "$stamped" "uploads/vee-block-lp-3072-02.stl")"
    revised=$(curl -sf "$API/api/parts/$vee/revisions" | field 'len(value)')
  else
    # Only when it is needed: re-seeding one stack would otherwise stack up a fourth revision and a
    # fifth, and the history this fixture exists for is "two", not "however many times seed has run".
    echo "  vee-block already has a second revision; not adding another"
  fi
  echo "  vee-block now has $revised revisions"
  seed_check "a controlled part reached a second revision" "$(holds "${revised:-0}" -ge 2)" "$revised revisions"

  echo "== EMPTY, created and never scanned"
  local empty empty_count
  empty=$(library_named Empty hobby)
  empty_count=$(sql "SELECT count(*) FROM part WHERE library_id = '$empty'")
  echo "  $empty"
  # It must stay empty: it is the only fixture for the empty states, and any flow that scanned it would
  # spend the fixture and leave an 861 MiB ingest running under everything measured after it.
  seed_check "Empty is empty" "$(holds "${empty_count:-1}" -eq 0)" "$empty_count parts"

  python3 - "$WORK/seed.json" <<EOF
import json, sys
json.dump({
    "sweep": {"id": "$sweep", "parts": $parts, "thumbnails": $thumbs, "folders": $folders,
              "formats": [f.strip() for f in "$formats".split(",") if f.strip()],
              "scan": "$scanned"},
    "default": {"id": "$default_lib", "parts": $default_count},
    "governed": {"id": "$governed", "revisedPart": "$vee", "revisions": ${revised:-0}},
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
  note "seed facts and $(wc -l < "$WORK/seed-checks.tsv") checks in $WORK/seed.json"
  [ "$seed_fail" = 0 ] || die "$seed_fail of the seed's checks failed (listed above, and in
  $WORK/seed.json). Driving a library that is not what stage 3 describes would test the wrong thing."
}

# ---------------------------------------------------------------------------------------------------
# drive

cmd_drive() {
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
    node "$ROOT/scripts/e2e/flows.mjs" \
      --out "$run" --web "$WEB" --api "$API" --seed "$WORK/seed.json" ${only:+--only "$only"}
    echo "flows exit $?"
    echo "== TIMING (web/scripts/open-timing.mjs, not reimplemented here)"
    # --url is explicit: its default is the owner's localhost:3000.
    node "$ROOT/web/scripts/open-timing.mjs" --url "$WEB" --rounds 2
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
# exit2: Phase 6's second exit, measured — "uploading a known part surfaces its near-duplicates".
#
# One library per pair, because near-duplicates are found within a library and two pairs in one library
# would be each other's candidates. The original goes in, then the same solid turned **off-axis** by 37
# degrees goes in at a second path, and the question is whether `/likeness` surfaces it. The numbers behind
# the verdict are read from `part_shape` rather than inferred: the 35-float descriptor and `size_mm` are
# both columns, so the Euclidean distance and the size-band position are exact rather than estimated.
#
# No score is shown in the interface, on purpose — `phase-6.md` says a figure like 0.038 invites a
# judgement nobody can calibrate — so reading the database is the only way to report one, and this is the
# place it is legitimate to.
cmd_exit2() {
  wait_for "$API/api/healthz" "the api" 30 || die "nothing is up. Run \`stack.sh up\` first."
  local sources=("$@")
  [ ${#sources[@]} -gt 0 ] || sources=("$ROOT/fixtures/bracket-lp-1042-03.stl")
  local degrees=${TURN_DEGREES:-37}
  local stamp; stamp=$(date +%s)
  local out=$WORK/exit2-$stamp.json
  echo "[" > "$out"
  local first=1

  for source in "${sources[@]}"; do
    [ -f "$source" ] || die "no such file: $source"
    local name; name=$(basename "$source" .stl)
    local turned=$WORK/exit2-$name-turn$degrees.stl
    python3 "$ROOT/scripts/e2e/skew-stl.py" --turn "$degrees" "$source" "$turned" > /dev/null ||
      die "skew-stl.py --turn $degrees failed on $source"

    local lib
    lib=$(library_named "Exit 2 $name $stamp" hobby)
    [ -n "$lib" ] || die "could not make a library for $name"
    echo "== $name into ${lib:0:8}…"
    local began=$SECONDS
    echo "  the known part:  $(upload_file "$lib" "$source" "pair/$name.stl")"
    echo "  turned $degrees degrees: $(upload_file "$lib" "$turned" "pair/$name-turn$degrees.stl")"
    local settled=$((SECONDS - began))

    local a b
    a=$(sql "SELECT id FROM part WHERE library_id = '$lib' AND source_path = 'pair/$name.stl'")
    b=$(sql "SELECT id FROM part WHERE library_id = '$lib' AND source_path = 'pair/$name-turn$degrees.stl'")
    # Both profiles, and how many parts of this library have none. Profiling runs in line inside the ingest
    # job, so a settled batch should leave nothing unprofiled and no `profile_shape` job behind.
    local rows stray
    rows=$(sql "SELECT p.source_path || E'\t' || s.size_mm || E'\t' || array_to_string(s.descriptor, ',') FROM part p JOIN part_shape s ON s.part_id = p.id WHERE p.library_id = '$lib' ORDER BY p.source_path")
    stray=$(sql "SELECT count(*) FROM job WHERE library_id = '$lib' AND kind LIKE '%profile%'")
    local unprofiled
    unprofiled=$(sql "SELECT count(*) FROM part p WHERE p.library_id = '$lib' AND p.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM part_shape s WHERE s.part_id = p.id)")
    local likeness
    likeness=$(curl -sf "$API/api/parts/$a/likeness")

    WORK=$WORK TURN_DEGREES=$degrees PAIR_NAME=$name SETTLED=$settled STRAY=${stray:-?} UNPROFILED=${unprofiled:-?} \
      TURNED_ID=$b LIKENESS=$likeness ROWS=$rows python3 "$ROOT/scripts/e2e/exit2.py" | tee "$WORK/exit2-$name.txt"
    [ "$first" = 1 ] || echo "," >> "$out"
    first=0
    cat "$WORK/exit2-$name.json" >> "$out"
  done
  echo "]" >> "$out"
  note "exit 2 measurements in $out"
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
  if [ -d "$STORE" ]; then chown_store "$(id -u):$(id -g)"; fi
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

# One service stopped and started again, and only the peer.
#
# An installation whose peer role is down is the whole of the relay case: a folder read from one of its
# other people while its owner is away can only be tested by taking the owner away, and stopping the
# process is the only honest way to do that. Through `compose`, so it is this project's peer and not a
# container named by hand.
cmd_peer() {
  case "${1:-}" in
    stop | start) note "peer $1 for $PROJECT"; compose "$1" peer ;;
    *) die "\`peer\` takes stop or start; got \`${1:-}\`." ;;
  esac
}

cmd_status() {
  echo "project  $PROJECT (lane $LANE${AS:+, installation $AS})"
  echo "ports    web $LAPIDARY_PORT_WEB, api $LAPIDARY_PORT_API, worker $LAPIDARY_PORT_WORKER, peer $LAPIDARY_PORT_PEER"
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
  exit2) shift; cmd_exit2 "$@" ;;
  drive) shift; cmd_drive "$@" ;;
  peer) shift; cmd_peer "$@" ;;
  down) shift; cmd_down "$@" ;;
  status) shift; cmd_status "$@" ;;
  *)
    echo "usage: scripts/e2e/stack.sh <build | up | seed | drive | down | status>" >&2
    echo "  drive [--compare <report.json>] [--only <flow,flow>]    down [--keep | --purge]" >&2
    echo "  up [--min-ram <gib>]   peer <stop|start>" >&2
    echo "  AS=a|b|c for one of three installations on this lane's block; scripts/e2e/group.sh runs three" >&2
    echo "  exit2 [<part.stl> …]   Phase 6 exit 2: a known part, then the same solid turned off-axis" >&2
    exit 2
    ;;
esac
