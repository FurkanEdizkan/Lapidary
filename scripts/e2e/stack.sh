#!/usr/bin/env bash
# The whole application, up and driven: one compose stack on this lane's port block, a library worth
# testing seeded into it, and a real browser over the real api.
#
#   scripts/e2e/stack.sh build            # deploy/'s five images, one service at a time, disk-guarded
#   scripts/e2e/stack.sh up               # bring it up and wait until every role answers
#   scripts/e2e/stack.sh seed             # the ingest tree and the four libraries
#   scripts/e2e/stack.sh drive [--compare <report.json>] [--only <flow,flow>]
#   scripts/e2e/stack.sh down [--keep]    # reverse the chown, remove the project, remove the store
#   scripts/e2e/stack.sh status
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

PROJECT=lapidary-e2e-${LAPIDARY_LANE:-}
[[ $PROJECT =~ ^lapidary-e2e-[0-4]$ ]] || refuse "the compose project would be \`$PROJECT\`, and this
  rig only ever speaks to \`lapidary-e2e-<lane 0-4>\`. LAPIDARY_LANE is \`${LAPIDARY_LANE:-}\`; it comes
  from $LANE_FILE, which scripts/claim-goal.sh writes. Anything else risks naming the owner's own
  \`lapidary\` project, whose volumes hold their real library."

# Asserted again inside `compose` — so immediately before every `down`, every `up` and every `build` —
# because the cost of this one being wrong is not a failed test, it is the owner's library.
assert_project() {
  [[ $PROJECT =~ ^lapidary-e2e-[0-4]$ ]] ||
    die "compose project \`$PROJECT\` is not lapidary-e2e-<lane>. Refusing to run docker compose."
}

# ---------------------------------------------------------------------------------------------------
# Paths, urls, and the one compose function.

LANE=$LAPIDARY_LANE
WORK=$ROOT/target/e2e/$LANE
ENVFILE=$WORK/e2e.env
STORE=$WORK/store
INGEST=$WORK/ingest
RUNS=$WORK/runs
WEB=http://127.0.0.1:$LAPIDARY_PORT_WEB
API=http://127.0.0.1:$LAPIDARY_PORT_API
WORKER_URL=http://127.0.0.1:$LAPIDARY_PORT_WORKER
CORPUS="$ROOT/target/sharing-check/corpus-1000/STL Files"
CORPUS_TSV=$ROOT/target/sharing-check/corpus-1000.tsv
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
    "$ROOT"/target/e2e/[0-4]/store) ;;
    *) die "refusing to remove \`$STORE\`: that is not target/e2e/<lane>/store." ;;
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
  local services=${SERVICES:-db web api peer worker}
  write_env
  verify_ports
  local summary=$WORK/build-summary.txt
  echo "== $(date -Is) build of $PROJECT; / $(free_gb /) GB free; /mnt/Storage $(free_gb "$ROOT") GB free" > "$summary"
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
  [ "$(free_gb /)" -ge 6 ] || ask_owner "/ has $(free_gb /) GB free, under the 6 GB this stack needs."
  # The declared ceilings in deploy/ total 4.3 GB. A stack brought up beside a cargo build is how a
  # session gets its processes killed; 5 GiB available is the floor that leaves the stack room.
  [ "$(ram_gib)" -ge 5 ] || ask_owner "$(ram_gib) GiB of RAM is available, and this stack's declared
  ceilings total 4.3 GB (db 1g, worker 2g, api 512m, peer 512m, web 256m). Wait until the other lanes
  have stopped compiling, then run this again."

  write_env
  # Before the directories exist, so a wrong bind source is caught before docker creates it as root.
  verify_ports
  mkdir -p "$STORE" "$INGEST" "$RUNS"
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
}, open(sys.argv[1], "w"), indent=2)
EOF
}

# ---------------------------------------------------------------------------------------------------
# seed: one ingest tree, four libraries

# The ingest tree, whose subdirectories become this library's categories.
build_ingest_tree() {
  if [ -f "$INGEST/.seeded" ] && [ "${FRESH_INGEST:-}" != 1 ]; then
    echo "  ingest tree already built ($(find "$INGEST" -type f ! -name .seeded | wc -l) files); FRESH_INGEST=1 to rebuild"
    return 0
  fi
  rm -rf "$INGEST"
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
  the \`STL Files\` level. If /mnt/Storage2 is not mounted, mount it; the slice needs the real bytes."
  [ -f "$CORPUS_TSV" ] || die "no corpus index at $CORPUS_TSV."
  local first
  first=$(head -1 "$CORPUS_TSV" | cut -f2)
  [ -e "$CORPUS/$first" ] || die "the first row of $CORPUS_TSV names \`$first\`, which does not exist
  under \`$CORPUS\`. Either the corpus root is wrong or /mnt/Storage2 is not mounted — copying would
  otherwise quietly produce an empty library."

  local copied=0 failed=0
  while IFS=$'\t' read -r size path; do
    [ -n "$path" ] || continue
    if (cd "$CORPUS" && cp -L --parents -- "$path" "$INGEST/"); then
      copied=$((copied + 1))
    else
      failed=$((failed + 1))
    fi
  done < <(sort -n "$CORPUS_TSV" | head -"$CORPUS_SLICE")
  [ "$failed" = 0 ] || die "$failed of $CORPUS_SLICE corpus files would not copy."
  echo "  corpus slice: $copied files, $(du -sh --apparent-size "$INGEST" | cut -f1) so far"

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

  : > "$INGEST/.seeded"
  echo "  ingest tree: $(find "$INGEST" -type f ! -name .seeded | wc -l) files, $(du -sh --apparent-size "$INGEST" | cut -f1), $(find "$INGEST" -mindepth 1 -type d | wc -l) directories"
}

# A library by name, made if it is not there. Names, not ids, so `seed` is re-runnable.
library_named() { # name, mode
  local existing
  existing=$(curl -sf "$API/api/libraries" | field "next((l['id'] for l in value if l['name'] == $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1")), '')")
  if [ -n "$existing" ]; then echo "$existing"; return 0; fi
  json POST "$API/api/libraries" "{\"name\":$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"),\"mode\":\"$2\"}" | field 'value["id"]'
}

# The chunked upload loop, lifted from check-plain.sh: probe, PUT 4 MiB at a time, commit, settle.
upload_file() { # library, local file, source path in the library
  local lib=$1 file=$2 path=$3 hash size offset=0 manifest commit
  hash=$(cd "$ROOT/web" && node -e "const {blake3}=require('hash-wasm'); blake3(require('fs').readFileSync(process.argv[1])).then(h=>console.log(h))" "$file") ||
    { echo "  could not hash $file (is web/node_modules linked?)"; return 1; }
  size=$(stat -c %s "$file")
  manifest="{\"files\":[{\"path\":\"$path\",\"blake3\":\"$hash\"}]}"
  json POST "$API/api/libraries/$lib/uploads/probe" "$manifest" > /dev/null || { echo "  probe refused for $path"; return 1; }
  while [ $offset -lt "$size" ]; do
    dd if="$file" iflag=skip_bytes,count_bytes skip=$offset count=$((4 * 1048576)) status=none |
      curl -sf -X PUT -H 'content-type: application/octet-stream' --data-binary @- \
        "$API/api/libraries/$lib/uploads/$hash?offset=$offset" > /dev/null ||
      { echo "  chunk at $offset refused for $path"; return 1; }
    offset=$((offset + 4 * 1048576))
  done
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
  echo "  failures: $(curl -sf "$API/api/libraries/$sweep/jobs/$batch" | field '[(f.get("path"), f.get("message","")[:160]) for f in value.get("failed", [])]')"

  local parts thumbs folders formats
  parts=$(sql "SELECT count(*) FROM part WHERE library_id = '$sweep' AND deleted_at IS NULL")
  thumbs=$(sql "SELECT count(DISTINCT p.id) FROM part p JOIN revision r ON r.part_id = p.id JOIN derivative d ON d.revision_id = r.id WHERE p.library_id = '$sweep' AND d.kind = 'thumbnail'")
  folders=$(curl -sf "$API/api/libraries/$sweep/folders" | python3 -c 'import json,sys
def count(nodes): return sum(1 + count(n.get("children") or []) for n in nodes)
print(count(json.load(sys.stdin)))')
  formats=$(curl -sf "$API/api/libraries/$sweep/facets" | field '", ".join(f["value"] for f in value["formats"])')
  echo "  parts $parts, thumbnails $thumbs, folders $folders, formats: $formats"

  echo "  re-scan settles all-skipped:"
  local again
  again=$(json POST "$API/api/libraries/$sweep/scan" | field 'value["batchId"]')
  echo "    $(settle "$sweep" "$again" 3600)"

  # The PMI cylinder's detail, check-plain.sh's assertions kept: this is what proves the real kernel
  # ran and not the mock.
  local pmi_part detail
  pmi_part=$(sql "SELECT id FROM part WHERE library_id = '$sweep' AND source_path LIKE '%pmi%' LIMIT 1")
  if [ -n "$pmi_part" ]; then
    detail=$(curl -sf "$API/api/parts/$pmi_part")
    echo "  PMI cylinder: format $(echo "$detail" | field 'value["sourceFormat"]'), structure $(echo "$detail" | field 'value["structure"] is not None'), entities $(echo "$detail" | field 'value["entities"] is not None'), pmi $(echo "$detail" | field 'value["pmi"] is not None'), kernel $(echo "$detail" | field 'value["kernelVersion"]')"
  else
    echo "  PMI cylinder: no part matched '%pmi%' in Sweep — the STEP scan did not land."
  fi

  echo "== THE DEFAULT LIBRARY, left as the Phase 3 exit timed it"
  local default_lib default_count
  default_lib=$(curl -sf "$API/api/libraries" | field 'value[0]["id"]')
  default_count=$(curl -sf "$API/api/libraries" | field 'value[0]["partCount"]')
  # Deliberately not scanned: web/scripts/open-timing.mjs opens this library's grid, and the Phase 3
  # numbers it is compared against were measured on exactly these six example parts.
  echo "  $default_lib holds $default_count parts (the six examples, unscanned on purpose)"

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
  local stamped=$WORK/revised-vee-block.stl
  cp "$ROOT/example/parts/vee-block-lp-3072-02.stl" "$stamped"
  printf 'lapidary e2e rev %s' "$(date +%s)" | dd of="$stamped" bs=1 seek=0 conv=notrunc status=none
  echo "  re-upload vee-block with new bytes: $(upload_file "$governed" "$stamped" "uploads/vee-block-lp-3072-02.stl")"
  local vee
  vee=$(sql "SELECT id FROM part WHERE library_id = '$governed' AND source_path = 'uploads/vee-block-lp-3072-02.stl'")
  revised=$(curl -sf "$API/api/parts/$vee/revisions" | field 'len(value)')
  echo "  vee-block now has $revised revisions"

  echo "== EMPTY, created and never scanned"
  local empty
  empty=$(library_named Empty hobby)
  echo "  $empty"

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
}, open(sys.argv[1], "w"), indent=2)
EOF
  note "seed facts in $WORK/seed.json"
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

  python3 - "$run" "$WORK/stack.json" "$WORK/seed.json" <<'EOF'
import json, pathlib, sys
run, stack, seed = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
def load(p, default=None):
    try: return json.loads(p.read_text())
    except Exception: return default
log = (run / "run.log").read_text(errors="replace")
timing = log.split("== TIMING", 1)[1] if "== TIMING" in log else ""
report = {
    "stack": load(stack, {}),
    "seed": load(seed, {}),
    "flows": load(run / "flows.json", []),
    "timing": [l for l in timing.splitlines() if l.strip() and not l.startswith("==")],
}
(run / "report.json").write_text(json.dumps(report, indent=2))
flows = report["flows"]
bad = [f["name"] for f in flows if f.get("status") not in ("ok", "pending")]
print(f"\n{len(flows)} flows: " + ", ".join(
    f"{s}={sum(1 for f in flows if f.get('status') == s)}"
    for s in sorted({f.get("status") for f in flows})))
print("report " + str(run / "report.json"))
if bad: print("FAILED: " + ", ".join(bad))
EOF
  local code=$?

  if [ -n "$compare" ]; then
    echo "== COMPARE against $compare"
    python3 - "$compare" "$run/report.json" <<'EOF'
import json, pathlib, re, sys
old, new = (json.loads(pathlib.Path(p).read_text()) for p in sys.argv[1:3])
def by_name(r): return {f["name"]: f for f in r.get("flows", [])}
a, b = by_name(old), by_name(new)
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

cmd_down() {
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
    compose down -v --remove-orphans 2>&1 | tail -4
  else
    echo "  no $ENVFILE, so no compose project to remove"
  fi
  remove_store
  # Last, and only once the volumes are gone: while a db volume survives, its password must too.
  rm -f "$ENVFILE"
  echo "  left behind: $(docker volume ls --format '{{.Name}}' | grep -c "^${PROJECT}_" || true) of this project's volumes, $(docker ps -a --format '{{.Names}}' | grep -c "^${PROJECT}-" || true) of its containers"
  echo "  the owner's volumes, untouched: $(docker volume ls --format '{{.Name}}' | grep -c '^lapidary_lapidary-' || true) of 2"
  echo "  $WORK holds: $(ls -A "$WORK" 2>/dev/null | paste -sd', ' -)"
}

# ---------------------------------------------------------------------------------------------------
# status

cmd_status() {
  echo "project  $PROJECT (lane $LANE)"
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
  drive) shift; cmd_drive "$@" ;;
  down) shift; cmd_down "$@" ;;
  status) shift; cmd_status "$@" ;;
  *)
    echo "usage: scripts/e2e/stack.sh <build | up | seed | drive | down | status>" >&2
    echo "  drive [--compare <report.json>] [--only <flow,flow>]    down [--keep]" >&2
    exit 2
    ;;
esac
