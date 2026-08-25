---
name: visual-verification
description: How a visual change in Guido is proved — render-tree snapshots, golden images on lavapipe, and the screenshot of last resort. Use whenever a change touches geometry, shaders, styling or anything that alters what appears on screen, when a snapshot or golden test fails, or when adding a scenario.
---

# Proving a visual change

Three instruments, in the order you should reach for them.

## 1. Render-tree snapshots — geometry and what gets drawn

`tests/render_snapshots.rs` lays out and paints widget trees taken from
`examples/`, with no compositor and no GPU, and diffs a text dump of the render
tree against `tests/snapshots/*.snap`. It catches a widget that moved by a
pixel, a clip that stopped being emitted, a scrollbar that quietly disappeared —
without anybody having predicted the assertion.

No text in these: text metrics depend on the fonts on the machine.

## 2. Golden images — the pixels

`tests/golden_images.rs` renders scenarios into a texture the test owns, reads
them back and compares against PNGs in `tests/golden/`. This is the only thing
that covers the SDF shaders, corner curvature, borders, shadows, gradients,
rounded clipping, HiDPI scaling, and text — both as glyphon draws it and as the
textured-quad path draws it under a transform.

The font is vendored under `tests/assets/` and registered per test thread, so a
scenario drawing text names that family and never reaches a system font. Use the
`label` helper rather than `text(..)` directly, or the golden becomes a record of
which fonts the machine had.

```bash
export VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
cargo test --test golden_images
```

**The rasterizer is part of the golden.** lavapipe, Mesa's software Vulkan
implementation — `vulkan-swrast` on Arch, `mesa-vulkan-drivers` on Debian and
Ubuntu. Two GPUs antialias an edge differently: the same scenarios on a desktop
Radeon differ from lavapipe on 0.03%–0.08% of their pixels, all of them on
corner tangent points. Blessing on anything else is refused by the test.

On any other adapter they skip themselves, which is why `cargo test` on a
machine with a GPU does not drown in failures that are not regressions. Set
`GUIDO_GOLDEN_ANY_ADAPTER=1` to run them anyway and look. CI sets
`GUIDO_GOLDEN_REQUIRED=1` in the golden job, where a skip is a failure.

### Reading a failure

The message says how many pixels moved, by how much, and the first few
coordinates with their before and after. Three images land in
`target/golden-failures/`: `.expected.png`, `.actual.png`, and `.diff.png` with
the changed pixels in magenta, dilated so a handful of them is visible at
natural size. In CI they are an artifact on the failed run.

A few dozen pixels on an edge is usually the rasterizer. Thousands is a
regression. Look before concluding either.

### Adding a scenario

Scenarios live at the bottom of the file, one `#[test]` each, built from
`container()` trees the same way `render_snapshots.rs` builds its own. Make the
scenario a sharp oracle: content that would cover the corner *if nothing
stopped it* tests a clip; a ladder of values tests a property across its range;
scale 2 tests what scale 1 cannot see. Then bless once, on lavapipe, and look at
the PNG that comes out before committing it.

### Creating a reference, and rewriting one

They are different acts and they have different words.

`UPDATE_GOLDEN=1` creates a picture for a scenario that has none — that is how
a new scenario starts, it is ordinary work, and nothing blocks it. Point it at
a reference that already exists and it declines: the comparison runs instead,
and if the pixels disagree you get the three images and the count.

`REBLESS_GOLDEN=1` rewrites one that exists. That turns a failing test green
without changing anything back, so a hook refuses it and CI refuses a pull
request that rewrites a reference without the `golden-update` label. When the
change is intended: look at the diff first, rewrite on lavapipe, put the before
and after in the pull request, add the label. The diff is the review.

`UPDATE_SNAPSHOTS` and `REBLESS_SNAPSHOTS` split the same way.

## 3. The screenshot

For anything the first two cannot reach — a real compositor, a real surface,
animation over time — run an example and capture it with `grim`. Do not ask
permission to take a screenshot; take it. Then say in the pull request what you
ran and what you saw, because nobody else can re-derive it from the diff.
