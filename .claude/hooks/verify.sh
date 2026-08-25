#!/usr/bin/env bash
# Nothing stops with the Rust unformatted, clippy unhappy, or the agent-facing
# documentation naming an API that no longer exists.
#
# The third one is here because it already happened: six claims in the skills
# were false the day they were written, and nobody found out until somebody
# thought to look. A rule that depends on remembering to check is not a rule.
#
# Each check runs only when something it covers has changed, and the result is
# fingerprinted, so a conversational turn costs nothing.
set -uo pipefail

cwd=$(cat | jq -r '.cwd // "."')
cd "$cwd" 2>/dev/null || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

rust_changed=$(git status --porcelain -- '*.rs' 2>/dev/null)
docs_changed=$(git status --porcelain -- AGENTS.md .claude/skills .claude/commands .claude/agents 2>/dev/null)
[ -z "$rust_changed" ] && [ -z "$docs_changed" ] && exit 0

marker="target/.verify-ok"
fingerprint=$(git diff HEAD -- '*.rs' AGENTS.md .claude 2>/dev/null | sha1sum | cut -d' ' -f1)
[ -f "$marker" ] && [ "$(cat "$marker")" = "$fingerprint" ] && exit 0

if [ -n "$rust_changed" ]; then
  if ! fmt_out=$(cargo fmt --all -- --check 2>&1); then
    echo "cargo fmt --all is not clean. Run it, then continue:" >&2
    echo "$fmt_out" | head -n 40 >&2
    exit 2
  fi

  if ! clippy_out=$(cargo clippy --all-targets --all-features -- -D warnings 2>&1); then
    echo "clippy denies warnings in CI, and it is not clean here. Fix these before stopping:" >&2
    echo "$clippy_out" | grep -E '^(error|warning)' -A 6 | head -n 80 >&2
    exit 2
  fi
fi

# Documentation that names something the crate does not have is documentation
# that will be followed into a wall.
if [ -n "$docs_changed" ] || [ -n "$rust_changed" ]; then
  if ! refs_out=$(cargo test --test skill_references 2>&1); then
    echo "The agent-facing documentation names an API this crate does not have:" >&2
    echo "$refs_out" | sed -n '/documentation names/,/identifiers checked/p' | head -n 30 >&2
    exit 2
  fi
fi

mkdir -p target && printf '%s' "$fingerprint" > "$marker"
exit 0
