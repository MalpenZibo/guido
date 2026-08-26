# Guido

A reactive Rust GUI library. wgpu for rendering, Wayland layer shell for
surfaces: status bars, panels, popups, lock screens. Everything composes from a
handful of primitives, and every property that survives to paint takes a signal
as readily as a value.

Backward compatibility is not a concern. Remove dead APIs, rename things, break
callers when it makes the library better — this is pre-1.0 and unstable on
purpose. What is not negotiable is that a change can be *verified*.

`CLAUDE.md` is a symlink to this file. Every agent reads the same contract.

## Rules

**Never commit to `main`.** Branch, open a pull request, merge it when CI is
green.

**Never rewrite a snapshot or a golden to make a test pass.** Creating one is
ordinary: a new scenario needs a first picture, and `UPDATE_GOLDEN=1` and
`UPDATE_SNAPSHOTS=1` make one for a reference that does not exist yet. They
refuse to touch a reference that does, because rewriting it turns a failing test
green without changing anything back. That takes `REBLESS_GOLDEN=1` or
`REBLESS_SNAPSHOTS=1`, a hook refuses those, and CI refuses a pull request that
rewrites a reference without the `golden-update` label. Asking for the hook to be
lifted is the wrong move — read the diff instead, it is in
`target/golden-failures/`.

**Every change names the thing that proves it.** Before writing the fix, write
the test that fails without it. A change to the renderer that no golden notices
is a change nothing is watching: the missing scenario is part of the work, not a
follow-up. If a change genuinely cannot be verified automatically, say so in the
pull request in one sentence, and say what you did by hand instead.

This rule covers *this file* and everything beside it. The skills, the commands
and the reviewer's criteria name APIs, and they are followed rather than read
sceptically — so being quietly wrong costs more there than in code.
`tests/skill_references.rs` is what keeps them honest, and it runs before you
stop.

**Architectural changes are agreed before they are written.** A new core type or
trait, a new cross-cutting mechanism, a new ownership or lifetime rule, a change
to how a whole family of call sites is spelled: explain the problem as it stands
in the code, the sites that have it, why the existing pieces cannot answer it,
the alternatives weighed, and a measurement wherever performance is claimed. The
decision is the maintainer's; implementation starts after they have made it.
Ordinary work is not this — fixing a bug at its definition, adding a widget
method, following a pattern that already exists.

**`cargo fmt` and `cargo clippy --all-targets --all-features -- -D warnings`
before every commit.** Clippy runs with `-D warnings` in CI, so a warning is a
build failure there.

## Commands

```bash
cargo build                        # build
cargo check                        # errors without codegen
cargo test --all-features          # everything but the pixels (see below)
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo run --example status_bar     # a real surface, on a real compositor

mdbook build book                  # the user documentation
mdbook serve book                  # ... with live reload
```

The golden images need a software rasterizer, because a golden is only
reproducible against the rasterizer it was blessed on:

```bash
export VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
cargo test --test golden_images
```

Install it with `vulkan-swrast` on Arch, `mesa-vulkan-drivers` on Debian and
Ubuntu. On any other adapter those tests skip themselves — a golden holds only
against the rasterizer it was blessed on — so `cargo test --all-features` is
green on a machine with a GPU and the pixels are checked by the job that points
at lavapipe. In that job a skip is a failure.

## What proves what

| A change to | is proved by |
| --- | --- |
| layout geometry, what gets drawn and where | `tests/render_snapshots.rs` — the render tree, as text |
| shaders, corners, borders, shadows, gradients, clipping, HiDPI, text | `tests/golden_images.rs` — the pixels, on lavapipe |
| the reactive system | unit tests beside the code in `src/reactive/` |
| widget behaviour and public API | integration tests in `tests/` |
| documented API | doc tests, and `cargo doc` with warnings denied |
| the user documentation | `mdbook build book` in CI |
| the APIs this file and the skills name | `tests/skill_references.rs` |
| the workflow this file, `/implement` and the templates describe | `tests/agent_workflow.rs` |
| Wayland protocol behaviour, compositor integration | **nothing automated.** Run an example and say what you saw in the pull request |

That last row is the hole in the harness. It is the one place where "I ran it
and it looked right" is still the standard, and the one place worth closing
next.

## What watches the harness

Every row above is an opinion until something checks it. `cargo mutants` changes
the code in one small way and asks whether any test notices: coverage asks
whether a line ran, this asks whether it mattered.

```bash
cargo mutants -f 'src/reactive/**/*.rs' -C --lib -j 8
```

The reactive core stands at **168 caught, 84 survived — 67%** as of the run that
introduced this. The survivors are not evenly spread and the number is less
useful than the shape: the background-write queue, the whole of `clipboard`,
`cursor` and most of `focus` are unwatched, and the numeric conversions in
`into_signal` can return the wrong number without a test objecting.

Whole-crate runs take hours; a module at a time is how it is used by hand. On a
pull request CI mutates only the lines the diff touched, which asks the one
question worth asking of new code — if this were wrong, would anything have
noticed. That job reports; it does not block, until somebody decides the
ratchet is worth the friction.

## Where the rest of it is

`docs/` is the developer reference — read the relevant file before a
significant change:

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — module structure and system design
- [docs/REACTIVE.md](docs/REACTIVE.md) — signals, memos, effects, ownership
- [docs/RENDERER.md](docs/RENDERER.md) — the paint, flatten and draw pipeline
- [docs/STYLING.md](docs/STYLING.md) — colors, gradients, borders, corners, shadows
- [docs/STATE_LAYER.md](docs/STATE_LAYER.md) — hover and pressed overrides, ripples
- [docs/TRANSFORMS.md](docs/TRANSFORMS.md) — translate, rotate, scale, origins
- [docs/IMAGES.md](docs/IMAGES.md) — raster and SVG sources

`book/` is the user documentation, published to the project site. Keeping it
true is part of the change that made it false — see the `book` skill.

`.claude/skills/` holds the working knowledge for each area of the codebase:
reactive, widgets, renderer, wayland, visual-verification, book. They load
themselves when the task touches them; you do not need to read them up front.

## Working on a change

1. **The specification comes first.** An issue with what was observed, what is
   expected instead, and an acceptance criterion that can be written as a test.
   `/spec` turns a rough idea into one.
2. **A worktree per change.** One issue, one branch, one pull request. Never two
   pieces of work in the same checkout.
3. **The failing test, then the fix.** In that order, so the test is known to
   test something.
4. **Run the harness**, all of it, including the goldens.
5. **Have it read by something that did not write it.** The `reviewer` subagent,
   over the committed change and before the pull request exists — its criteria
   ask about the commits, so they need the commits. Whoever wrote a change is
   the worst judge of whether it grew. One pass, and its findings say what they
   cost: **zero blocking findings is the pass**, notes are not something to
   clear. An architectural finding stops the work — that decision belongs to the
   maintainer wherever it surfaces.
6. **Open the pull request** with the template filled in: what moved, what
   proves it, what the review found, what you looked at by hand.

`/implement <issue>` does all six.

## Commits and pull requests

Atomic commits: one focused change each, reviewable and revertible on its own.
Data structures, then rendering, then the widget API — not one commit called
"add feature".

The subject line is a sentence that says what is true now, not a category and a
summary: *"The guard belongs where the events enter, not where they land"*, not
*"fix(events): move guard"*. The body says what was wrong and why this is the
answer.

No `Co-Authored-By` trailer. No generated-with footer.
