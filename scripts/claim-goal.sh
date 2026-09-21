#!/usr/bin/env bash
# Claim a goal for one lane: docs/goals/PROTOCOL.md, "Claiming a goal".
#
# The claim is creating the goal's branch. `git worktree add -b` refuses a branch that already
# exists, so two sessions claiming the same goal cannot both succeed — no board edit, no lock.
#
# It then gives the lane everything it must not share with another session: its own worktree (and
# so its own `target/`), its own test database, its own ports, and rust-analyzer off. Those settings
# go in the worktree's untracked `.lane.env`, which every `cargo xtask` command applies.
set -euo pipefail

usage() {
  echo "usage: scripts/claim-goal.sh <goal-id> <lane 1-4>   e.g. scripts/claim-goal.sh G2 1" >&2
  exit 2
}
[ $# -eq 2 ] || usage
goal=$1
lane=$2
case "$lane" in [1-4]) ;; *) usage ;; esac

# The main checkout, whichever worktree this runs from: claims always branch from main.
main_root=$(cd "$(git rev-parse --git-common-dir)/.." && pwd)
worktree=$main_root/.claude/worktrees/$goal

goal_file=$(git -C "$main_root" show "main:docs/goals/$goal.md" 2>/dev/null) || {
  echo "No docs/goals/$goal.md on main. Pick a goal id from docs/goals/BOARD.md." >&2
  exit 1
}
branch=$(printf '%s\n' "$goal_file" | sed -n 's/^\*\*Branch:\*\* `\([^`]*\)`.*/\1/p' | head -1)
[ -n "$branch" ] || { echo "docs/goals/$goal.md names no **Branch:** line." >&2; exit 1; }

# Disk guards (PROTOCOL.md): a lane builds its own target/, about 13 GB.
free_gb() { df --output=avail -BG "$1" | tail -1 | tr -dc '0-9'; }
[ "$(free_gb /)" -ge 8 ] || { echo "Root has under 8 GB free. Ask before building." >&2; exit 1; }
[ "$(free_gb "$main_root")" -ge 20 ] || { echo "$main_root has under 20 GB free. Ask before building." >&2; exit 1; }

# The claim itself.
if ! git -C "$main_root" worktree add "$worktree" -b "$branch" main; then
  echo "Could not claim $goal: branch $branch or worktree $worktree already exists. Somebody holds it — pick another goal." >&2
  exit 1
fi

ln -s "$main_root/web/node_modules" "$worktree/web/node_modules"

# The lane's own test database: sqlx names each test's database after the test's path, so two
# sessions sharing one server drop each other's databases mid-run.
db=lapidary-test-db-$lane
db_port=$((55432 + lane))
if ! docker ps --format '{{.Names}}' | grep -qx "$db"; then
  docker run -d --rm --pull never --name "$db" \
    -e POSTGRES_PASSWORD=localdev -e POSTGRES_USER=lapidary -e POSTGRES_DB=lapidary \
    -p "$db_port:5432" docker.io/library/postgres:18 > /dev/null
  for _ in $(seq 1 30); do
    docker exec "$db" pg_isready -U lapidary > /dev/null 2>&1 && break
    sleep 1
  done
fi

cat > "$worktree/.lane.env" <<EOF
# Lane $lane, goal $goal, branch $branch. Written by scripts/claim-goal.sh; untracked.
# Every \`cargo xtask\` command applies these (xtask/src/lane.rs).
LAPIDARY_LANE=$lane
DATABASE_URL=postgres://lapidary:localdev@localhost:$db_port/lapidary
CARGO_BUILD_JOBS=4
CARGO_INCREMENTAL=0
LAPIDARY_PORT_WEB=3${lane}000
LAPIDARY_PORT_API=3${lane}080
LAPIDARY_PORT_WORKER=3${lane}081
LAPIDARY_PORT_PEER=3${lane}082
EOF

# rust-analyzer builds the workspace in the background; several of them do not fit in 15.5 GB.
mkdir -p "$worktree/.claude"
cat > "$worktree/.claude/settings.local.json" <<'EOF'
{
  "enabledPlugins": {
    "rust-analyzer-lsp@claude-plugins-official": false
  }
}
EOF

cat <<EOF
Claimed $goal for lane $lane.
  worktree  $worktree
  branch    $branch
  database  $db on port $db_port
  ports     web 3${lane}000, api 3${lane}080, worker 3${lane}081, peer 3${lane}082
Next: enter the worktree (EnterWorktree with path $worktree), read docs/goals/PROTOCOL.md and
docs/goals/$goal.md, and tell the lead (ListAgents, then SendMessage to lapidary-lead) that you hold it.
EOF
