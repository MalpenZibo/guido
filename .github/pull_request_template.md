<!--
Five questions. The first two decide whether this can be reviewed at all.
-->

## What changed

<!-- One paragraph. What is true now that was not before. Closes #<issue> -->

## What proves it

<!--
Name it: the test that fails without this change, the snapshot that moved, the
golden that moved. "cargo test passes" is not an answer — every pull request
passes cargo test.

If a golden or a snapshot was re-blessed, attach the before and after, say what
moved and why that is the intended change, and add the `golden-update` label.
CI refuses the pull request otherwise.
-->

## What the review found

<!--
The `reviewer` subagent read this change before the pull request existed. How
many findings blocked, and what happened to them — that number is usually zero,
and zero is a pass, not a thing to apologise for.

Findings worth answering that you did not act on belong here in a sentence
each: silence is what is not allowed, disagreement is fine. Notes do not need
repeating here unless somebody should pick them up later.

If it did not run — it failed, it timed out, nobody invoked it — say that
instead, in those words. A review that did not happen and a review that found
nothing look identical here and mean opposite things.
-->

## What was checked by hand

<!--
Anything the harness cannot see: a compositor, a real surface, an animation over
time. Which example, which compositor, what you saw. Screenshots welcome.

"Nothing — the harness covers all of it" is a fine answer when it is true.
-->

## Left undone

<!--
Anything deliberately out of scope, and why. "Nothing" is a fine answer.

Whatever has to outlive this change gets an issue in the same act; put its
number here. What earns one is a bar, not whatever is left over: ask who is
worse off today and what they would see. Nobody worse off is a sentence here
and no issue. Worse off, and it fits under this pull request's title, is
something you should have done rather than listed. Worse off, and it needs an
acceptance criterion this change's test cannot state, is the issue. AGENTS.md
step 7 has the three in full.
-->
