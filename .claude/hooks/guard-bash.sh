#!/usr/bin/env bash
# Two things that must not happen quietly, refused where they are typed rather
# than asked for in prose.
#
#   1. Rewriting a snapshot or a golden. It turns a failing test green without
#      changing anything back, so it is a decision, and a decision is the
#      maintainer's to make with the diff in front of them. Creating a
#      reference that does not exist yet is not that: a new scenario needs a
#      first picture, and the harness only lets UPDATE_GOLDEN and
#      UPDATE_SNAPSHOTS make one — rewriting needs REBLESS_*, which is this.
#   2. Committing on main. The project merges through pull requests.
#
# Both are decided by looking at the position a word occupies, not at whether
# the word appears. `git log | sed 's/^/commit: /'` is not a commit, a heredoc
# that writes the name of the blessing variable into a file is not a blessing,
# and the branch that matters is the one belonging to the directory the command
# will run in — which, in a project that puts every change in a worktree of its
# own, is usually not the directory the session is standing in.
#
# `.claude/hooks/guard-bash.test.sh` is the table of what that means.
set -uo pipefail

payload=$(cat)
command=$(printf '%s' "$payload" | jq -r '.tool_input.command // ""')
session_cwd=$(printf '%s' "$payload" | jq -r '.cwd // "."')

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

# A heredoc body is data being written, not commands being run. Drop it before
# anything else looks at the text.
strip_heredocs() {
  awk '
    {
      if (skip) { if ($0 == delim) skip = 0; next }
      line = $0
      if (match(line, /<<-?[ ]*"?'"'"'?[A-Za-z_][A-Za-z0-9_]*"?'"'"'?/)) {
        word = substr(line, RSTART, RLENGTH)
        gsub(/^<<-?[ ]*/, "", word)
        gsub(/["'"'"']/, "", word)
        delim = word
        skip = 1
      }
      print line
    }
  ' <<< "$1"
}

runnable=$(strip_heredocs "$command")

# Split into command segments: a new command starts after ; && || | or a newline.
segments=$(printf '%s' "$runnable" | sed -E 's/(\|\||&&|;|\||\n)/\n/g')

# `cd somewhere &&` in an earlier segment sets where the later ones run.
dir="$session_cwd"

while IFS= read -r segment; do
  # shellcheck disable=SC2206
  read -ra tokens <<< "$segment"
  [ ${#tokens[@]} -eq 0 ] && continue

  i=0
  # Leading `env` and NAME=VALUE assignments belong to the command that follows.
  while [ $i -lt ${#tokens[@]} ]; do
    case "${tokens[$i]}" in
      env) i=$((i + 1)) ;;
      REBLESS_GOLDEN=*|REBLESS_SNAPSHOTS=*)
        deny "Rewriting a reference is blocked. It makes a failing test pass without changing a pixel back, so the failure is the finding: read it. The golden diff images are in target/golden-failures/, and the snapshot diff is in the test output. Creating a reference that does not exist yet is not this and is not blocked — UPDATE_GOLDEN and UPDATE_SNAPSHOTS do that. Rewriting one is the maintainer's decision, and the pull request carries the golden-update label to say they made it."
        ;;
      *=*) i=$((i + 1)) ;;
      *) break ;;
    esac
  done
  [ $i -ge ${#tokens[@]} ] && continue

  case "${tokens[$i]}" in
    cd)
      # Where the rest of the line runs.
      candidate="${tokens[$((i + 1))]:-}"
      [ -n "$candidate" ] && dir="$candidate"
      ;;
    git)
      target="$dir"
      j=$((i + 1))
      # Walk git's own flags to reach the subcommand.
      while [ $j -lt ${#tokens[@]} ]; do
        case "${tokens[$j]}" in
          -C)
            target="${tokens[$((j + 1))]:-$dir}"
            j=$((j + 2))
            ;;
          -c)
            j=$((j + 2))
            ;;
          --*)
            j=$((j + 1))
            ;;
          *) break ;;
        esac
      done
      if [ "${tokens[$j]:-}" = "commit" ]; then
        branch=$(git -C "$target" symbolic-ref --quiet --short HEAD 2>/dev/null || echo "")
        if [ "$branch" = "main" ]; then
          deny "This is main (in $target). Guido merges through pull requests: branch first (git checkout -b <kind>/<slug>), then commit, then open the pull request."
        fi
      fi
      ;;
  esac
done <<< "$segments"

exit 0
