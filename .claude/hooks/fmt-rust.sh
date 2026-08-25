#!/usr/bin/env bash
# Formatting is not a thing to remember, ask about, or spend a turn on.
set -euo pipefail

file=$(cat | jq -r '.tool_input.file_path // ""')
case "$file" in
  *.rs) [ -f "$file" ] && rustfmt --edition 2024 "$file" >/dev/null 2>&1 || true ;;
esac
exit 0
