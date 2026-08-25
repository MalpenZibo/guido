#!/usr/bin/env bash
# Nothing stops with the Rust unformatted or clippy unhappy: CI denies warnings,
# so a warning left behind is a failed build somebody else waits for.
#
# The check is skipped when no Rust changed since it last passed, so a
# conversational turn costs nothing.
set -uo pipefail

cwd=$(cat | jq -r '.cwd // "."')
cd "$cwd" 2>/dev/null || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

changed=$(git status --porcelain -- '*.rs' 2>/dev/null)
[ -z "$changed" ] && exit 0

marker="target/.verify-ok"
fingerprint=$(git diff HEAD -- '*.rs' 2>/dev/null | sha1sum | cut -d' ' -f1)
[ -f "$marker" ] && [ "$(cat "$marker")" = "$fingerprint" ] && exit 0

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

mkdir -p target && printf '%s' "$fingerprint" > "$marker"
exit 0
