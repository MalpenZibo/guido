---
description: Ask what would have caught this — find what the current change leaves unverified, and propose the smallest test that closes the biggest hole
argument-hint: [area or file; defaults to the current diff]
---

Work out what is *not* being watched.

Target: $ARGUMENTS — if that is empty, use the current diff (`git diff HEAD`).

## 1. What does this change actually alter

Not the lines. The observable behaviour: geometry, pixels, reactive updates,
public API shape, protocol traffic, timing.

## 2. For each of those, what would fail if it were wrong

Go and check, rather than assuming. Run the suite with the change inverted where
that is cheap — flip a comparison, drop a guard, change a constant — and see
whether anything goes red. A test that does not fail when the code is wrong is
not covering it.

Map each behaviour to one of:

- covered by a named test that would genuinely fail
- covered only incidentally, by a test that would fail for an unrelated reason
- not covered at all

## 3. Report

A short table: behaviour, what covers it, verdict. Then the single test worth
writing first — the one that closes the largest gap for the least work — with
where it goes and what it asserts.

Prefer, in this order: a unit test beside the code, an integration test in
`tests/`, a render-tree snapshot scenario, a golden image scenario. Reach for
the expensive instrument only when the cheap one cannot see the thing.

Offer to write it. Do not write it unasked.
