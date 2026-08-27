---
description: Take an issue from its acceptance criterion to a pull request, in a worktree of its own
argument-hint: <issue number>
---

Implement issue $ARGUMENTS, end to end.

## 1. Read the specification

`gh issue view $ARGUMENTS`. Find the acceptance criterion.

**If there is no acceptance criterion, stop.** Say what is missing and offer to
write it with `/spec`. Implementing against a description that cannot be checked
is how unverifiable changes get merged.

If the issue is labelled `needs-design`, stop as well: the architecture is
agreed with the maintainer before it is written, not presented afterwards.

## 2. A worktree of its own

```bash
git -C <repo> worktree add ../guido-$ARGUMENTS -b <kind>/<slug> origin/main
```

The kind is one of fix, feat, perf, refactor or docs. One issue, one
branch, one pull request; never two pieces of work in the same checkout.

Load the skill for the area you are about to touch.

## 3. The failing test first

Write the test the acceptance criterion describes. Run it. **Confirm it fails,
and that it fails for the stated reason** — a test that passes before the change
is testing something else, and a test that fails for the wrong reason is worse
than none.

## 4. Then the change

Make it. Keep it inside the non-goals. Follow the patterns already in the file
rather than importing new ones.

## 5. Clean it up, before the harness sees it again

`/simplify` over the change — a built-in skill, not one of this repository's:
four readings by four agents, none of which wrote the code, for reuse,
simplification, efficiency and altitude. It is quality only; it does not hunt
for correctness bugs, which is what step 8 is for.

Here, and not later, for three reasons. It **applies** what it finds rather than
reporting it, so the harness has to run after it — put it below step 6 and you
commit code nothing has re-tested since it changed, and if it touched the test
you wrote in step 3, the red you confirmed there is no longer the red that
ships. Its edits land inside the commits instead of on top of them, and a
tidy-up commit is exactly what the review's atomic-commits criterion exists to
catch. And what it changed is then read by the review, because code that edits
itself and ships unread is the one thing that step must not become.

## 6. Run the harness, all of it

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1) \
  cargo test --test golden_images
```

If a golden moved, **look at the images** in `target/golden-failures/` before
concluding anything. A few dozen pixels on an edge is the rasterizer; thousands
is a regression you just caused. Never re-bless to make it pass: report what
moved and why, and let the maintainer decide.

If the change touched the renderer and *no* golden moved, the scenario that
should have noticed is missing — add it.

## 7. Commit, atomically

One focused change per commit, in the order the code was built: data structures,
then behaviour, then the API that exposes it. Subject lines are sentences that
say what is now true. No `Co-Authored-By`, no generated-with footer.

## 8. Have it read by somebody who did not write it

Now the commits exist, the cleanup is inside them, and the pull request does
not — which is the moment the
`reviewer` subagent is written for: it asks about atomic commits, about what the
pull request will have to say, and about the diff as a whole.

Run it over the change. Its criteria live in `.claude/agents/reviewer.md` and are
not repeated here — a second copy is a second thing to keep true, and the copy
is always the one that goes stale.

**One pass.** Not until it comes back clean — that is not a state it reaches,
because a reviewer asked to review returns a list and a list always has items.

It sorts its findings into three levels and says how many block. What those
levels mean is its business and is defined where it is defined; what you do with
each is this:

- **Blocks** — stop. If it is the architectural case, say what was found and
  ask, which is the same verb as step 1: finding it late does not move the
  decision to whoever wrote it.
- **Worth answering** — act on it, or say in the pull request why you did not.
  Silence is what is not allowed; disagreement is fine.
- **Note** — into the pull request or an issue.

**Zero blocking findings is the pass.** A review that comes back with only notes
has said the change is sound; it does not become unsound because the notes
exist, and clearing them is not what this step is for.

A review that did not run is not a review that found nothing: the two look
identical from the outside and mean opposite things. If it failed, timed out or
was skipped, run it again; if it still will not run, say so in the pull request
in those words. Writing "nothing found" for a review that never happened is the
same act as re-blessing a golden to make a test pass.

## 9. Open the pull request

Push the branch, `gh pr create`, fill in the template. Link the issue with
`Closes #$ARGUMENTS`.

Whatever goes under **Left undone** that has to outlive this change gets an
issue in the same act, and the template gets its number. `AGENTS.md` says why.

Then report back, in five lines: what changed, what proves it, what the review
found, what you looked at by hand, and anything you left undone, why, and
its issue number.
