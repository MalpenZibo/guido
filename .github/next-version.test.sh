#!/usr/bin/env bash
# What next-version.sh has to get right, as a table. Why it exists at all is at
# the top of the script itself.
#
# Every case is a small git repository holding a tag, and a `gh` that answers
# with the pull requests the case says were merged since. What GitHub really
# holds changes without a commit; what the script does with each shape of
# answer is this file's business and does not.
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/next-version.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# A `gh` that records what it was asked and answers whatever the case asked for.
mkdir -p "$tmp/bin"
cat >"$tmp/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >"$GH_STUB_ARGS"
printf '%s' "$GH_STUB_OUT"
exit "${GH_STUB_CODE:-0}"
STUB
chmod +x "$tmp/bin/gh"
PATH="$tmp/bin:$PATH"
export GH_STUB_ARGS="$tmp/gh-args"

# A repository whose history holds the given tags, one commit each, in order,
# then six squash-merged pull requests, #1 to #6. The last tag's commit is made
# on 2026-09-23 at 15:38:16 UTC and every other a month earlier, so a search
# that starts from the wrong tag says so; it is the merge of release pull
# request #99, which is not in the release it tags.
repo() {
  local dir=$tmp/$1
  shift
  git init -q "$dir"
  local tag date n
  for tag in "$@"; do
    date=2026-08-23T15:38:16Z
    [ "$tag" = "${*: -1}" ] && date=2026-09-23T15:38:16Z
    GIT_COMMITTER_DATE=$date GIT_AUTHOR_DATE=$date \
      git -C "$dir" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "$tag is the next release (#99)"
    git -C "$dir" tag "$tag"
  done
  if [ $# -gt 0 ]; then
    for n in 1 2 3 4 5 6; do
      git -C "$dir" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "Change $n (#$n)"
    done
  fi
  printf '%s' "$dir"
}

# What `gh pr list --json number,labels` says about pull requests carrying the
# given labels: one argument per pull request, labels separated by commas.
merged() {
  local n=0 pr
  for pr in "$@"; do
    n=$((n + 1))
    jq -nc --arg n "$n" --arg labels "$pr" \
      '{number: ($n | tonumber), labels: ($labels | split(",") | map(select(. != "")) | map({name: .}))}'
  done | jq -sc .
}

failures=0

# fail <why> [<output> ...]
fail() {
  echo "FAIL  $1"
  shift
  printf '      %s\n' "$@"
  failures=$((failures + 1))
}

# check <expected exit> <repo> <proposed> <what this case is> [<expected stdout>] [<text the output must contain> ...]
check() {
  local expect=$1 dir=$2 proposed=$3 why=$4
  shift 4
  local want=${1-}
  [ $# -gt 0 ] && shift
  local out err code
  out=$(cd "$dir" && "$script" "$proposed" 2>"$tmp/stderr")
  code=$?
  err=$(cat "$tmp/stderr")
  if [ "$code" != "$expect" ]; then
    fail "expected exit $expect, got $code: $why" "$out" "$err"
    return
  fi
  if [ "$expect" = 0 ] && [ "$out" != "$want" ]; then
    fail "expected $want, got '$out': $why" "$err"
    return
  fi
  local wanted
  for wanted in "$@"; do
    if ! printf '%s\n%s' "$out" "$err" | grep -qF -- "$wanted"; then
      fail "output does not mention '$wanted': $why" "$out" "$err"
      return
    fi
  done
  echo "ok    $why"
}

released=$(repo released v0.6.0 v0.7.0)

# The acceptance criteria, one each. release-plz proposes a patch whenever
# semver-checks finds nothing breaking; a label is what raises it.
GH_STUB_OUT=$(merged fix feature,ci) \
  check 0 "$released" 0.7.1 "a pull request labelled feature asks for a minor" 0.8.0 '#2'

GH_STUB_OUT=$(merged enhancement) \
  check 0 "$released" 0.7.1 "enhancement is a feature by another name" 0.8.0

GH_STUB_OUT=$(merged fix bug,ci docs dependencies chore) \
  check 0 "$released" 0.7.1 "fixes and chores ask for a patch" 0.7.1

GH_STUB_OUT=$(merged breaking) \
  check 0 "$released" 0.7.1 "breaking asks for a minor while guido is 0.x" 0.8.0

# The floor is release-plz's, and a label never lowers it: semver-checks found
# something breaking that nobody labelled, or a feature bump is already there.
GH_STUB_OUT=$(merged fix) \
  check 0 "$released" 0.8.0 "a higher proposal is left alone" 0.8.0

# The label bump counts from the last tag, not from the proposal, or every push
# to main would raise an already-raised release pull request again.
GH_STUB_OUT=$(merged feature) \
  check 0 "$released" 0.8.0 "a feature on top of a minor proposal is the same minor" 0.8.0

# Also what was asked of GitHub: pull requests into main merged after the last
# tag's commit, not after an older tag's.
GH_STUB_OUT='[]' \
  check 0 "$released" 0.7.1 "nothing merged with a label says nothing" 0.7.1
grep -qF -- 'merged:>=2026-09-23T15:38:16Z' "$GH_STUB_ARGS" && grep -qF -- '--base main' "$GH_STUB_ARGS" ||
  fail "gh was not asked for pull requests into main merged after v0.7.0" "$(cat "$GH_STUB_ARGS")"

# The release pull request merged after the tag's date, and a pull request
# into main that the release branch does not hold: neither is in the release.
GH_STUB_OUT='[{"number":99,"labels":[{"name":"feature"}]},{"number":7,"labels":[{"name":"breaking"}]}]' \
  check 0 "$released" 0.7.1 "only the pull requests between the tag and HEAD vote" 0.7.1

GH_STUB_OUT=$(merged ',') \
  check 0 "$released" 0.7.1 "a pull request with no labels at all" 0.7.1

# Past 1.0 the labels mean what semver says they mean.
stable=$(repo stable v1.2.3)
GH_STUB_OUT=$(merged fix breaking) \
  check 0 "$stable" 1.2.4 "breaking asks for a major from 1.0 on" 2.0.0
GH_STUB_OUT=$(merged feature) \
  check 0 "$stable" 1.2.4 "a feature past 1.0 is a minor" 1.3.0

# Red, each for its own reason. A version the release pull request carries is
# not a thing to guess: a label step that could not read the labels has to say
# so rather than leave the proposal standing as if it had agreed with it.
GH_STUB_OUT='gh: Bad credentials (HTTP 401)' GH_STUB_CODE=1 \
  check 1 "$released" 0.7.1 "GitHub cannot be asked" '' '::error::' 'HTTP 401'

GH_STUB_OUT='<html>rate limited</html>' \
  check 1 "$released" 0.7.1 "something that is not JSON answers" '' '::error::'

untagged=$(repo untagged)
GH_STUB_OUT='[]' \
  check 1 "$untagged" 0.7.1 "no v* tag to count from" '' '::error::'

GH_STUB_OUT='[]' \
  check 1 "$released" 0.8.0-rc.1 "a proposal that is not X.Y.Z" '' '::error::' '0.8.0-rc.1'

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases pass"
