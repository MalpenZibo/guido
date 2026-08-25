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

## 7. Open the pull request

Push the branch, `gh pr create`, fill in the template. Link the issue with
`Closes #$ARGUMENTS`.

Then report back, in four lines: what changed, what proves it, what you looked
at by hand, and anything you left undone and why.
