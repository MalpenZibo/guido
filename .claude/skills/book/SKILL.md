---
name: book
description: Keeping Guido's mdbook user documentation true when the API changes. Use when adding or changing a public API, a widget method, a styling option, an animation or a transform — the book is part of that change, not a follow-up.
---

# The book

`book/` is the user documentation, published to the project site by
`.github/workflows/docs.yml` on every push to `main` that touches it. CI builds
it on pull requests, so a broken book fails before it is merged.

```bash
mdbook build book       # what CI runs
mdbook serve book       # live reload while writing
```

## Which chapter

| A change to | goes in |
| --- | --- |
| a widget method | `book/src/concepts/container.md` or the relevant chapter |
| a styling option | `book/src/building-ui/` |
| a state layer feature | `book/src/interactivity/` |
| an animation option | `book/src/animations/` |
| a transform feature | `book/src/transforms/` |
| a renamed or removed API | every chapter that used it — grep the old name |

`book/src/SUMMARY.md` is the table of contents; a new chapter that is not in it
does not exist.

## What makes it wrong

The failure mode is not a missing chapter, it is a code sample that no longer
compiles against the library it documents. When you change a signature, grep
`book/` for the old spelling before assuming nothing referenced it.

If the feature has a visual result, capture a screenshot with `grim` and put it
beside the text. The chapters that show what a thing looks like are the ones
people actually read.
