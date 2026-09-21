#!/usr/bin/env bash
# The one string this repository shares with a setting it cannot see.
#
# The `main` ruleset requires a status check by display name, and that name is
# the `ci` job's `name:` in .github/workflows/ci.yml. The two sit on opposite
# sides of a fence — one versioned here, one a repository setting — and a rename
# that moves only this side leaves every pull request green and unmergeable,
# waiting for a check that can no longer report. That is #288, and it cost a
# merge before anybody worked out why: `repos/.../branches/main/protection`
# answers 404 and reads as "nothing is required".
#
# `repos/{owner}/{repo}/rules/branches/{branch}` is the endpoint that answers,
# so this asks it and compares. It needs no secret: GitHub documents it as
# needing `"Metadata" repository permissions (read)`, which the workflow token
# always carries, and adds that it "can be used without authentication or the
# aforementioned permissions if only public resources are requested". guido is
# public and the call answers 200 with no token at all.
#
# A read that did not happen is not a disagreement. A fork whose ruleset
# requires nothing, a network blip, a rate limit, a repository that goes private
# one day: each of those is a notice and a zero exit, because failing
# everybody's build for want of an answer is worse than the hole this closes.
# What is red is a ruleset whose required contexts are not exactly this
# workflow's gate, and a reader that could not do its job at all.
#
# Usage: required-context.sh [path/to/ci.yml]
set -uo pipefail

# The job whose `name:` the ruleset is expected to hold. The key, not the name:
# the name is what is being checked, so it is never written down here.
gate=ci

workflow=${1:-$(cd "$(dirname "$0")/.." && pwd)/.github/workflows/ci.yml}
repository=${GITHUB_REPOSITORY:-}
# The branch the ruleset guards, which is the one the gate exists for.
branch=${RULESET_BRANCH:-main}

notice() {
  printf '::notice::%s\n' "$*"
  printf 'The gate was not compared against the ruleset. This is not a failure.\n'
}

# Every tool this needs is part of the runner image, so a missing one is the
# reader being broken rather than an answer failing to arrive — red, like a
# ci.yml that cannot be parsed. Without this, a dropped tool would take the
# branch below meant for a network that is down: the comparison would report
# nothing wrong because it never happened, which is the shape of #288 all over
# again.
for tool in yq jq gh timeout; do
  command -v "$tool" >/dev/null 2>&1 && continue
  printf '::error::%s\n' "\`$tool\` is not installed, so what the ruleset requires cannot be \
compared with the name the gate reports under. A guard that cannot run must not report that it \
found nothing."
  exit 1
done

# The gate's `name:`, read out of the workflow rather than repeated here. A
# failure is the reader being broken rather than an answer being missing, so it
# is red: a guard that cannot run is not a guard that found nothing.
if ! name=$(yq '.jobs["'"$gate"'"].name // ""' "$workflow" 2>&1); then
  printf '::error::%s\n' "$workflow could not be read: ${name//$'\n'/ }"
  exit 1
fi

if [ -z "$name" ]; then
  printf '::error::%s\n' "The \`$gate\` job of $workflow has no \`name:\`, so it reports under its \
key and the ruleset is waiting for a context nothing produces."
  exit 1
fi

if [ -z "$repository" ]; then
  notice "GITHUB_REPOSITORY is unset, so there is no repository to ask about."
  exit 0
fi

# Bounded, because this runs inside a job the merge waits on and `gh` neither
# retries nor gives up on its own: a stalled connection would otherwise hold a
# runner for the six hours a job is allowed and leave the pull request pending
# rather than answered. `timeout` exits 124, which is a read that did not happen
# like any other.
if ! rules=$(timeout "${RULESET_TIMEOUT:-20}" \
  gh api "repos/$repository/rules/branches/$branch" 2>&1); then
  notice "The rules of $repository@$branch could not be read: ${rules//$'\n'/ }"
  exit 0
fi

# Every rule that applies to the branch comes back, from every ruleset at every
# level. Only the required status checks are this script's business: the merge
# method, the approvals and the rest are #434's non-goals.
if ! contexts=$(printf '%s' "$rules" | jq -r '.[]
    | select(.type == "required_status_checks")
    | .parameters.required_status_checks[].context' 2>&1); then
  notice "The rules of $repository@$branch are not a shape this can read: ${contexts//$'\n'/ }"
  exit 0
fi

if [ -z "$contexts" ]; then
  notice "$repository@$branch requires no status checks, so there is no name to disagree with."
  exit 0
fi

# Exactly the gate, and nothing beside it. A required context no job in ci.yml
# reports is the same hole from the other side, and #290 settled that the list
# is one long.
if [ "$contexts" = "$name" ]; then
  printf '%s@%s requires "%s", which is what the %s job reports under.\n' \
    "$repository" "$branch" "$name" "$gate"
  exit 0
fi

printf '::error::%s\n' "The ruleset and ci.yml disagree about what gates a merge: \
$repository@$branch requires ${contexts//$'\n'/, } and the \`$gate\` job reports under \"$name\". \
A required check no job can report stays pending for ever, so every pull request is refused with \
every check green — that is #288. The ruleset has to end up requiring that one name and nothing \
beside it; move whichever of the two is wrong."
exit 1
