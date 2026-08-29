---
name: book
description: Keeping Guido's mdbook user documentation true when the API changes. Use when adding or changing a public API, a widget method, a styling option, an animation or a transform — the book is part of that change, not a follow-up.
---

# The book

`book/` is the user documentation, published to the project site by
`.github/workflows/docs.yml` on every push to `main` that touches it. CI builds
it on pull requests, so a broken book fails before it is merged.

```bash
mdbook build book       # renders it
mdbook serve book       # live reload while writing

# and what CI runs, which is rustdoc over every sample
cargo build --all-features --target-dir target/book
env -u WAYLAND_DISPLAY mdbook test book -L target/book/debug/deps
```

The `-L` points at a directory holding exactly one copy of the library:
`mdbook test` gives rustdoc no `--extern`, so `extern crate guido` resolves by
searching that path, and a shared `target/debug/deps` holds one artifact per
feature set. `WAYLAND_DISPLAY` is unset because **`mdbook test` runs what it
compiles** — a sample that starts an application will otherwise open a surface
on your screen.

## Writing a sample that compiles

Every fence is one of four things, and picking wrong is silent:

| the block is | write | why |
| --- | --- | --- |
| a fragment — a builder chain, a few lines | ```` ```rust ```` with a hidden preamble | it has no `use` and no `main` of its own |
| a whole program | ```` ```rust,no_run ```` | it must compile, and must not dial a compositor |
| a signature listing, a diagram, terminal output | ```` ```text ```` | **an unlabelled fence is given to rustdoc as Rust** |
| genuinely illustrative — internals, elisions | ```` ```rust,ignore ```` | and it then proves nothing, so prefer the others |

The hidden preamble for a fragment, none of which the reader sees — `book.toml`
hides `#` lines:

```text
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().padding(8.0)
# ;
# }
```

`extern crate` because there is no `--extern`; the trailing `# ;` because a
fragment inside `fn main` is otherwise its return value. A name the chapter
introduced in an earlier block gets a hidden `let` too — each sample is compiled
alone, and nothing carries between them.

`tests/documentation_references.rs` checks that every fence carries an info
string rustdoc knows. It does not check that the sample is *good*.

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
compiles against the library it documents — and until #294 nothing compiled
them, so a rename was invisible here. It is CI's job now, but 32% of the blocks
are `ignore` and CI is silent about those: when you change a signature, grep
`book/` for the old spelling rather than trusting the green tick.

If the feature has a visual result, capture a screenshot with `grim` and put it
beside the text. The chapters that show what a thing looks like are the ones
people actually read.
