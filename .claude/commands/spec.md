---
description: Turn a rough idea into an issue whose acceptance criterion can be written as a test
argument-hint: <what you noticed, or what you want to be true>
---

Turn this into a specification, and do not write any implementation code:

$ARGUMENTS

## 1. Find out whether it is real

Read the code before believing the description. Locate the actual site, and
confirm the behaviour is what was reported — a report is a symptom, and the
symptom is often not where the defect lives. If what you find differs from what
was described, say so and describe what is actually there.

If two readings of the request would lead to materially different work, ask —
once, with the two readings spelled out. Otherwise decide and state the
assumption in the issue.

## 2. Write it as a specification

The title is a sentence that states the defect or the truth that should hold —
*"A container collapsed to nothing still answers for the area it covers when
open"* — not a category and a summary.

The body, in this order:

- **Observed** — what happens now, with the file and line where it happens.
- **Expected** — what should be true instead, phrased so it can be checked.
- **Acceptance** — the test that will fail before the change and pass after,
  named concretely: which file it goes in, what it asserts. If the answer is a
  golden scenario, say which one and what it would show. **If you cannot state
  this, the specification is not finished** — either dig until you can, or say
  plainly that this change cannot be verified automatically and what the
  manual check would be.
- **Non-goals** — the neighbouring things this change deliberately does not do,
  so the implementation does not grow into them.
- **Where it lives** — the files and functions involved.

Consult the skill for the area (reactive, widgets, renderer, wayland) so the
issue is written in the terms the codebase already uses.

## 3. Create it

Show the issue body first and get agreement, then `gh issue create`. Print the
URL when it exists.

If the change would introduce a new core type, a cross-cutting mechanism, a new
ownership rule, or would change how a family of call sites is spelled, label it
`needs-design` and say in the body that the architecture is to be agreed before
anything is written.
