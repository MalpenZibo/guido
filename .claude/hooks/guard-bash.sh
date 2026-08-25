#!/usr/bin/env bash
# Two things that must not happen quietly, refused where they are typed rather
# than asked for in prose.
#
#   1. Re-blessing a snapshot or a golden. Both rewrite the artifact that was
#      supposed to fail, which turns a failing test green without changing
#      anything back. Blessing is a decision, and a decision is the
#      maintainer's to make with the diff in front of them.
#   2. Committing on main. The project merges through pull requests.
set -euo pipefail

payload=$(cat)
command=$(printf '%s' "$payload" | jq -r '.tool_input.command // ""')
cwd=$(printf '%s' "$payload" | jq -r '.cwd // ""')

deny() {
  jq -n --arg reason "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

if printf '%s' "$command" | grep -Eq '(^|[^A-Z_])(UPDATE_GOLDEN|UPDATE_SNAPSHOTS)='; then
  deny "Re-blessing is blocked. A rewritten snapshot or golden makes a failing test pass without changing a pixel back — the failure is the finding, so read it: the golden diff images are in target/golden-failures/, and the snapshot diff is in the test output. If the change is intended, say so and the maintainer blesses it, with the golden-update label on the pull request."
fi

if printf '%s' "$command" | grep -Eq '\bgit\b.*\bcommit\b'; then
  branch=$(git -C "${cwd:-.}" symbolic-ref --quiet --short HEAD 2>/dev/null || echo "")
  if [ "$branch" = "main" ]; then
    deny "This is main. Guido merges through pull requests: branch first (git checkout -b <kind>/<slug>), then commit, then open the pull request."
  fi
fi

exit 0
