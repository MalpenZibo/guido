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

## 5. Run the harness, all of it

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

## 6. Commit, atomically

One focused change per commit, in the order the code was built: data structures,
then behaviour, then the API that exposes it. Subject lines are sentences that
say what is now true. No `Co-Authored-By`, no generated-with footer.

## 7. Have it read by somebody who did not write it

Now the commits exist and the pull request does not, which is the moment the
`reviewer` subagent is written for: it asks about atomic commits, about what the
pull request will have to say, and about the diff as a whole.

Run it over the change. Its criteria live in `.claude/agents/reviewer.md` and are
not repeated here — a second copy is a second thing to keep true, and the copy
is always the one that goes stale.

What to do with what it says:

- **An architectural finding stops the work.** A new core type, a new trait, a
  new cross-cutting mechanism, a new ownership rule, a family of call sites
  respelled: that was the maintainer's decision to make before it was written,
  and finding it here does not transfer the decision to whoever wrote it. Stop,
  say what was found, and ask — the same verb as step 1.
- **Everything else is yours to act on, or to disagree with in writing.** A
  finding you did not act on is worth a sentence in the pull request rather than
  silence; the next person will wonder the same thing.
- **A clean report is one line and you move on.** Do not ask again hoping for a
  different answer.
- **A review that did not run is not a review that found nothing.** The two look
  identical from the outside and mean opposite things. If it failed, timed out,
  or was skipped, run it again; if it still will not run, say so in the pull
  request in those words. Writing "nothing found" for a review that never
  happened is the same act as re-blessing a golden to make a test pass.

## 8. Open the pull request

Push the branch, `gh pr create`, fill in the template. Link the issue with
`Closes #$ARGUMENTS`.

Then report back, in five lines: what changed, what proves it, what the review
found, what you looked at by hand, and anything you left undone and why.
