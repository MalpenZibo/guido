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
rounded clipping and HiDPI scaling.

```bash
export VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
cargo test --test golden_images
```

**The rasterizer is part of the golden.** lavapipe, Mesa's software Vulkan
implementation — `vulkan-swrast` on Arch, `mesa-vulkan-drivers` on Debian and
Ubuntu. Two GPUs antialias an edge differently: the same scenarios on a desktop
Radeon differ from lavapipe on 0.03%–0.08% of their pixels, all of them on
corner tangent points. Blessing on anything else is refused by the test.

Without a Vulkan adapter the tests skip themselves. CI sets
`GUIDO_GOLDEN_REQUIRED=1`, where a skip is a failure.

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

### Never re-bless to make a test pass

`UPDATE_GOLDEN=1` and `UPDATE_SNAPSHOTS=1` rewrite the thing that was supposed
to fail. A hook blocks both, and CI refuses a pull request that edits
`tests/golden/` without the `golden-update` label. When a change is intended:
bless on lavapipe, put the before and after images in the pull request, add the
label. The diff is the review.

## 3. The screenshot

For anything the first two cannot reach — a real compositor, a real surface,
animation over time — run an example and capture it with `grim`. Do not ask
permission to take a screenshot; take it. Then say in the pull request what you
ran and what you saw, because nobody else can re-derive it from the diff.
