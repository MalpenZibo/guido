#!/usr/bin/env bash
# What the guard has to get right, as a table.
#
# It exists because all three of the guard's first defects were things a table
# would have caught before anybody met them: a command refused for containing
# the word "commit" inside a sed expression; the branch read from the session's
# directory rather than the one the command runs in, which in a project that
# puts every change in a worktree of its own is the wrong repository nearly
# every time; and a heredoc *writing* this very file refused because the text
# being written mentioned the variable that blesses a golden.
#
# One root cause under all three: matching a word anywhere in the command
# instead of at the position where a command is actually invoked.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
guard="$here/guard-bash.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The variables the guard refuses, spelled so that this file does not contain
# what it is testing for — otherwise writing it trips the guard.
bless_golden="UPDATE_""GOLDEN=1"
bless_snapshot="UPDATE_""SNAPSHOTS=1"

# Two repositories: one sitting on main, one on a branch.
for name in on-main on-branch; do
  git init -q -b main "$tmp/$name"
  git -C "$tmp/$name" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
done
git -C "$tmp/on-branch" checkout -q -b fix/something

failures=0

# check <allow|deny> <cwd> <command> <what this case is>
check() {
  local expect=$1 cwd=$2 command=$3 why=$4
  local out decision
  out=$(jq -n --arg c "$command" --arg d "$cwd" \
    '{tool_input:{command:$c},cwd:$d}' | "$guard" 2>/dev/null)
  if printf '%s' "$out" | grep -q '"permissionDecision": *"deny"'; then
    decision=deny
  else
    decision=allow
  fi
  if [ "$decision" != "$expect" ]; then
    echo "FAIL  expected $expect, got $decision: $why"
    echo "      command: $command"
    failures=$((failures + 1))
  else
    echo "ok    $decision  $why"
  fi
}

# The word is not the deed.
check allow "$tmp/on-main" "git log --oneline | sed 's/^/commit: /'" \
  "a read-only log that prints the word commit"
check allow "$tmp/on-main" "echo 'run git commit after branching'" \
  "prose about committing"
check allow "$tmp/on-main" "cargo test --test golden_images" \
  "an ordinary build command"
check allow "$tmp/on-branch" "cat > notes.md <<'EOF'
$bless_golden rewrites the reference
EOF" \
  "writing a file that mentions the blessing variable"

# A commit on main is refused, wherever the session happens to be standing.
check deny "$tmp/on-main" "git commit -m 'x'" \
  "a commit on main"
check deny "$tmp/on-main" "git commit -F - <<'MSG'
A subject
MSG" \
  "a commit on main with a heredoc message"
check deny "$tmp/on-branch" "git -C $tmp/on-main commit -m 'x'" \
  "a commit aimed at main from elsewhere by -C"
check deny "$tmp/on-main" "cd $tmp/on-main && git commit -m 'x'" \
  "a commit on main reached by cd"

# A commit on a branch is the normal case and must not be refused.
check allow "$tmp/on-branch" "git commit -m 'x'" \
  "a commit on a feature branch"
check allow "$tmp/on-main" "git -C $tmp/on-branch commit -m 'x'" \
  "a commit on a branch while the session sits on main"
check allow "$tmp/on-main" "cd $tmp/on-branch && git commit -m 'x'" \
  "a commit on a branch reached by cd from main"

# Re-blessing, wherever it is typed.
check deny "$tmp/on-branch" "$bless_golden cargo test --test golden_images" \
  "re-blessing a golden"
check deny "$tmp/on-branch" "$bless_snapshot cargo test" \
  "re-blessing a snapshot"
check deny "$tmp/on-branch" "cargo test && $bless_golden cargo test" \
  "re-blessing after something else on the same line"
check allow "$tmp/on-branch" "cargo test --test render_snapshots" \
  "running the snapshots without blessing them"

if [ "$failures" -ne 0 ]; then
  echo
  echo "$failures case(s) wrong."
  exit 1
fi
echo
echo "all cases pass"
