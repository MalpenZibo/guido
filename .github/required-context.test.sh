#!/usr/bin/env bash
# What required-context.sh has to get right, as a table. Why it exists at all is
# at the top of the script itself.
#
# Its two halves fail in opposite directions, which is why most of these cases
# are green ones. The comparison has to be red when the ruleset and the gate's
# name disagree, and say what it found on both sides — an error naming neither
# is an error somebody has to reproduce by hand. Everything else has to be
# green: a fork's ruleset requires nothing, a runner loses the network, a rate
# limit answers instead of GitHub. The one check a merge waits on is the worst
# possible place for a build that fails because an answer did not arrive, and a
# script that reads an endpoint is exactly the kind that learns to fail that way
# one edit at a time.
#
# The endpoint is stubbed. What the real one says is not this file's business
# and changes without a commit; what the script does with each shape of answer
# is, and does not.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/required-context.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# A `gh` that answers whatever the case asked for, ahead of the real one, and a
# `yq` that is the real one until a case wants it broken — the reader failing is
# a case of its own, because a guard that cannot run has to be red rather than
# quietly green.
real_yq=$(command -v yq) || {
  echo "required-context.sh reads ci.yml with yq, and yq is not installed"
  exit 1
}
mkdir -p "$tmp/bin"
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
[ -n "${GH_STUB_SLEEP:-}" ] && sleep "$GH_STUB_SLEEP"
printf '%s' "$GH_STUB_OUT"
exit "${GH_STUB_CODE:-0}"
STUB
cat >"$tmp/bin/yq" <<STUB
#!/usr/bin/env bash
if [ -n "\${YQ_STUB_CODE:-}" ]; then exit "\$YQ_STUB_CODE"; fi
exec "$real_yq" "\$@"
STUB
chmod +x "$tmp/bin/gh" "$tmp/bin/yq"
# `#!/usr/bin/env bash` resolves through PATH, so the one case that runs with
# `$tmp/bin` alone needs a bash there to be read by at all.
ln -s "$(command -v bash)" "$tmp/bin/bash"
PATH="$tmp/bin:$PATH"

# A ci.yml holding one `ci` job, spelled however the case needs it.
workflow() {
  local file=$tmp/$1.yml
  shift
  {
    printf 'name: CI\n\non:\n  pull_request:\n\njobs:\n'
    printf '%s\n' "$@"
  } >"$file"
  printf '%s' "$file"
}

# What the endpoint says when it requires the given contexts.
rules() {
  jq -nc --args '[{type: "required_status_checks",
    parameters: {required_status_checks: ($ARGS.positional | map({context: .}))}}]' "$@"
}

failures=0

# check <expected exit> <workflow file> <what this case is> [<text the output must contain> ...]
check() {
  local expect=$1 file=$2 why=$3
  shift 3
  local out code
  out=$(GITHUB_REPOSITORY="${REPOSITORY-owner/repo}" PATH="${TEST_PATH:-$PATH}" \
    "$script" "$file" 2>&1)
  code=$?
  if [ "$code" != "$expect" ]; then
    echo "FAIL  expected exit $expect, got $code: $why"
    printf '      %s\n' "$out"
    failures=$((failures + 1))
    return
  fi
  local wanted
  for wanted in "$@"; do
    if ! printf '%s' "$out" | grep -qF -- "$wanted"; then
      echo "FAIL  output does not mention '$wanted': $why"
      printf '      %s\n' "$out"
      failures=$((failures + 1))
      return
    fi
  done
  echo "ok    $why"
}

gate=$(workflow gate '  ci:' '    name: CI' '    runs-on: ubuntu-latest')

# The ordinary day, and the one thing it must not do is pass quietly on a read
# that never happened: it says which repository answered and what it said.
GH_STUB_OUT=$(rules CI) \
  check 0 "$gate" "the ruleset requires exactly the gate's name" 'owner/repo' 'CI'

# #288 itself. Both names have to appear: the one the ruleset waits for and the
# one the workflow can produce, because the fix is to move one of them and
# whoever reads the log has to be able to tell which.
renamed=$(workflow renamed '  ci:' '    name: Merge gate' '    runs-on: ubuntu-latest')
GH_STUB_OUT=$(rules CI) \
  check 1 "$renamed" "the gate was renamed and the ruleset was not" 'CI' 'Merge gate' '::error::'

# A second required context is the same hole from the other side: a name no job
# in ci.yml reports, waited for by the ruleset, watched by nothing.
GH_STUB_OUT=$(rules CI "Lint the lockfile") \
  check 1 "$gate" "the ruleset requires a context beside the gate" 'Lint the lockfile' '::error::'

GH_STUB_OUT=$(rules "CI ") \
  check 1 "$gate" "a required context differs from the name by a space" '::error::'

# Green, all of it. A fork's own ruleset, a runner that cannot reach GitHub, a
# rate limit answering in GitHub's place, a call cut off before GitHub answers,
# a checkout that does not know its repository: none of them is a disagreement,
# and none of them is allowed to fail a pull request.
GH_STUB_OUT='[{"type":"non_fast_forward"}]' \
  check 0 "$gate" "a ruleset that requires no status checks" '::notice::'

GH_STUB_OUT='[]' \
  check 0 "$gate" "a branch with no rules at all, as on a fork" '::notice::'

GH_STUB_OUT='gh: Not Found (HTTP 404)' GH_STUB_CODE=1 \
  check 0 "$gate" "the endpoint refuses the token" '::notice::' 'HTTP 404'

GH_STUB_OUT='<html>rate limited</html>' \
  check 0 "$gate" "something that is not JSON answers" '::notice::'

# An endpoint that never answers at all, which is the shape that would hold a
# gating job for the six hours a job is allowed. The bound is what turns it into
# an ordinary missing answer, so the case waits on a real one rather than
# fabricating the exit code the bound produces.
GH_STUB_SLEEP=5 RULESET_TIMEOUT=1 GH_STUB_OUT=$(rules CI) \
  check 0 "$gate" "GitHub never answers" '::notice::'

REPOSITORY= GH_STUB_OUT=$(rules CI) \
  check 0 "$gate" "nothing says which repository to ask about" '::notice::'

# Red, and each for its own reason. A gate with no `name:` reports under its key,
# so there is nothing for the ruleset to hold; a reader that cannot read ci.yml
# has found nothing and must not say so in green, which is the fail-open shape
# every line above is arranged against.
nameless=$(workflow nameless '  ci:' '    runs-on: ubuntu-latest')
GH_STUB_OUT=$(rules CI) \
  check 1 "$nameless" "the gate has no name to report under" '::error::' 'has no `name:`'

YQ_STUB_CODE=1 GH_STUB_OUT=$(rules CI) \
  check 1 "$gate" "ci.yml cannot be read at all" '::error::'

# A runner image that drops one of the tools. Without the preflight this is
# indistinguishable from a network that is down — `command not found` exits 127
# and the read simply fails — so the guard would go green having never run,
# which is #288's shape exactly. `$tmp/bin` holds the two stubs and nothing else.
TEST_PATH="$tmp/bin" GH_STUB_OUT=$(rules CI) \
  check 1 "$gate" "a tool the reader needs is missing" '::error::' 'not installed'

# The gate's name, and not whichever job's comes first or last.
between=$(workflow between '  book:' '    name: Book' '    runs-on: ubuntu-latest' \
  '  ci:' '    name: CI' '    runs-on: ubuntu-latest' \
  '  test:' '    name: Test' '    runs-on: ubuntu-latest')
GH_STUB_OUT=$(rules CI) \
  check 0 "$between" "the gate sits between two other jobs"

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases pass"
