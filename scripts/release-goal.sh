#!/usr/bin/env bash
# Release a goal's worktree after the lead merged it: docs/goals/PROTOCOL.md, "Lead".
#
# Refuses a branch main does not contain, unless --abandon says the work is being given up. Stops
# the lane's test database once no other worktree uses that lane.
set -euo pipefail

usage() { echo "usage: scripts/release-goal.sh <goal-id> [--abandon]" >&2; exit 2; }
[ $# -ge 1 ] && [ $# -le 2 ] || usage
goal=$1
abandon=${2:-}
[ -z "$abandon" ] || [ "$abandon" = "--abandon" ] || usage

main_root=$(cd "$(git rev-parse --git-common-dir)/.." && pwd)
worktree=$main_root/.claude/worktrees/$goal
[ -d "$worktree" ] || { echo "No worktree at $worktree." >&2; exit 1; }

branch=$(git -C "$worktree" rev-parse --abbrev-ref HEAD)
if [ -z "$abandon" ] && ! git -C "$main_root" merge-base --is-ancestor "$branch" main; then
  echo "$branch is not merged into main. Merge it first, or pass --abandon to give the work up." >&2
  exit 1
fi

lane=$(sed -n 's/^LAPIDARY_LANE=//p' "$worktree/.lane.env" 2>/dev/null | head -1)

# --force: the worktree holds its own untracked target/, .lane.env and node_modules link.
git -C "$main_root" worktree remove --force "$worktree"
if [ -z "$abandon" ]; then
  git -C "$main_root" branch -d "$branch"
else
  echo "Kept branch $branch (abandoned, not merged); delete it with git branch -D when sure."
fi

if [ -n "$lane" ]; then
  still_used=$(grep -lx "LAPIDARY_LANE=$lane" "$main_root"/.claude/worktrees/*/.lane.env 2>/dev/null || true)
  if [ -z "$still_used" ]; then
    docker stop "lapidary-test-db-$lane" > /dev/null 2>&1 && echo "Stopped lapidary-test-db-$lane." || true
  fi
fi
echo "Released $goal."
