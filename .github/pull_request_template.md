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
The `reviewer` subagent read this change before the pull request existed. What
did it say, and what did you do about it. "Nothing — it reported the change
sound" is a fine answer; so is a finding you disagreed with and why, which is
worth a sentence here rather than silence.

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
Anything deliberately out of scope, and why. A follow-up issue number if there
is one. "Nothing" is a fine answer.
-->
