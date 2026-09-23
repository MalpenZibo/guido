#!/usr/bin/env bash
# The version the release pull request has to carry, given the one release-plz
# proposed.
#
# release-plz picks a version from two things: conventional-commit subjects,
# which guido does not write, and cargo-semver-checks, which sees a removed or
# changed public item and nothing else. So on its own it proposes a patch for a
# release whose pull requests added features, because an addition breaks
# nobody. The labels on those pull requests are what say "feature", and
# `.github/labeler.yml` puts most of them there without anybody remembering to.
#
# This reads the labels of every pull request squash-merged between the last
# `v*` tag and HEAD — the same commits release-plz writes into CHANGELOG.md —
# and works out the bump they ask for:
#
#   breaking             major, or minor while the version is 0.x
#   feature, enhancement minor
#   anything else        patch
#
# counted from the tag, not from the proposal: counted from the proposal, every
# push to main would raise an already-raised release pull request once more.
# The higher of the two versions wins, so the labels can raise what release-plz
# found and never lower it — a breaking change semver-checks catches still
# bumps the minor with no label on it at all.
#
# Everything it could not read is red. The version a release is published under
# cannot be taken back, and a label step that quietly agreed with the proposal
# because GitHub did not answer is how a feature ships as a patch.
#
# Usage: next-version.sh <proposed version>   (run inside the repository)
# Prints the version to release on stdout; says why on stderr.
set -uo pipefail

error() {
  printf '::error::%s\n' "$*" >&2
  exit 1
}

semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

proposed=${1:-}
[[ $proposed =~ $semver ]] ||
  error "release-plz proposed '$proposed', which is not a MAJOR.MINOR.PATCH version this can compare."

if ! tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>&1); then
  error "There is no v* tag behind HEAD to count the labels from: ${tag//$'\n'/ }"
fi
last=${tag#v}
[[ $last =~ $semver ]] || error "The last tag, $tag, is not a vMAJOR.MINOR.PATCH version."
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

# The pull requests in the release are the ones whose squash commits, which
# GitHub ends with `(#N)`, sit between the tag and HEAD. History rather than a
# merge date: the release pull request is merged a second after the commit it
# tags, and by date it would count itself.
numbers=$(git log --format=%s "$tag..HEAD" | sed -nE 's/.*\(#([0-9]+)\)$/\1/p' |
  jq -Rsc 'split("\n") | map(select(. != "") | tonumber)')

# Their labels, from one search that the tag's date bounds. UTC, because the
# search reads an offset like `+02:00` only when nothing on the way has turned
# the `+` into a space.
since=$(TZ=UTC git log -1 --format=%cd --date=format-local:%Y-%m-%dT%H:%M:%SZ "$tag")
if ! prs=$(gh pr list --state merged --base main --search "merged:>=$since" \
  --limit 1000 --json number,labels 2>&1); then
  error "The pull requests merged since $tag could not be read: ${prs//$'\n'/ }"
fi

# One line per pull request: its number, then the bump its labels ask for.
if ! votes=$(printf '%s' "$prs" | jq -r --argjson numbers "$numbers" '.[]
    | select(.number as $n | $numbers | index($n))
    | (.labels | map(.name)) as $labels
    | [.number,
       if ($labels | index("breaking")) then "breaking"
       elif ($labels | index("feature") or index("enhancement")) then "minor"
       else "patch" end]
    | @tsv' 2>&1); then
  error "The pull requests merged since $tag are not a shape this can read: ${votes//$'\n'/ }"
fi

bump=patch
while IFS=$'\t' read -r number vote; do
  [ -n "$number" ] || continue
  printf '#%s asks for %s\n' "$number" "$vote" >&2
  case $vote in
    breaking) bump=breaking ;;
    minor) [ "$bump" = breaking ] || bump=minor ;;
  esac
done <<<"$votes"

case $bump in
  breaking) if [ "$major" = 0 ]; then
    wanted="$major.$((minor + 1)).0"
  else
    wanted="$((major + 1)).0.0"
  fi ;;
  minor) wanted="$major.$((minor + 1)).0" ;;
  patch) wanted="$major.$minor.$((patch + 1))" ;;
esac

# `sort -V` orders MAJOR.MINOR.PATCH numerically, which is all that is left to
# compare once both have matched the pattern above.
next=$(printf '%s\n%s\n' "$proposed" "$wanted" | sort -V | tail -1)
printf 'Since %s the labels ask for a %s bump, %s; release-plz proposed %s. Releasing %s.\n' \
  "$tag" "$bump" "$wanted" "$proposed" "$next" >&2
printf '%s\n' "$next"
