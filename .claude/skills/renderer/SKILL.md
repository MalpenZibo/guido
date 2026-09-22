---
name: renderer
description: Guido's rendering pipeline — paint into a render tree, flatten to draw commands, batch into layers, draw with instanced SDF shapes on wgpu; text, images, backdrop blur, damage and frame pacing. Use when touching src/renderer/, when something draws in the wrong order or not at all, or when a frame costs too much.
---

# The rendering pipeline

Reference in [docs/RENDERER.md](../../../docs/RENDERER.md) and
[docs/ARCHITECTURE.md](../../../docs/ARCHITECTURE.md). This is the shape of it.

## One frame

Rendering is paced by the compositor's `wl_surface.frame` callbacks: an
animating surface renders once per callback, an idle surface renders nothing.

1. `flush_bg_writes()` — drain queued background-thread signal writes
2. `take_wake_request()` — was a frame asked for
3. dispatch events (queued `MouseMove`s coalesce to the latest position)
4. **frame-pacing gate** — if the previous callback has not fired, return before
   draining jobs; animation continuations stay queued, init and resizes bypass
5. `distribute_jobs()` sorts pending work by surface, each surface takes its
   own with `drain_surface_jobs()`, and `process_jobs()` applies them —
   unregister, advance animations, reconcile dynamic children, set dirty flags
6. partial layout from `layout_roots` — only dirty subtrees
7. **skip the frame** if the root does not need paint
8. `paint()` — build the render tree; clean children reuse `Rc`-shared cached
   `RenderNode`s, so reuse is a refcount bump and not a clone
9. `flatten_root_into()` — render tree to draw commands; clean subtrees reuse
   cached commands
10. re-arm the frame callback and report damage via `wl_surface.damage_buffer()`
    — both *before* presenting, so they ride the commit inside `present()`
11. GPU: instanced SDF shapes, HiDPI scaling, per-group layer order. On a lost
    or outdated swapchain the dirty state is kept and a retry frame requested
12. `cache_paint_results()` — `Rc`-share paint output, clear `needs_paint`.
    Partial paints poison their ancestors, are never cached, and drop the
    entry the last complete paint left

## Layers and ordering

Within a draw group the order is shapes → images → text → overlay. A group is
split whenever the tree paints a lower layer over a higher one, so batching
never reorders drawing. Overlays (ripples) naturally land after children
because the render tree is explicit rather than a push/pop stack.

## Shapes

Everything is an SDF in one fragment shader: rounded rectangles with
superellipse corners (the CSS K-value system), circles, gradients, borders with
uniform width, shadows with smooth falloff, clipping. Corner curvature and
border width are scaled in the shader, not in layout — which is why a HiDPI bug
is invisible to any test that renders at scale 1.

## Text

`glyphon` over `cosmic-text`. Measurement lives in `text_measurer.rs` and is
cached. Fonts come from the system plus anything the application handed to
`load_font`. Text metrics depend on the fonts installed on the machine, which
is why the render-tree snapshots exclude text. The goldens do render it: they
vendor a font under `tests/assets/` and name it in every scenario, so nothing
there can reach a system font.

Transformed text is a path of its own. Rotated or scaled text is not handed to
glyphon — it is rasterised to a texture and drawn as a quad
(`text_quad.rs`, `textured_quad_shader.wgsl`). Anything true of axis-aligned
text has to be checked again there.

A frosted text's coverage mask follows the same recipe as that quad: rasterised
in the text's own space, read back through `to_shape` by the composite, and
taking `TEXT_SUPERSAMPLE` only when a transform stretches it — both flat rules
are worse in half the cases, measured in the comment on
`command_to_text_backdrop`. Where it is read is all the transform decides, so a
scale animation reuses one rasterisation rather than asking for a new one per
frame — a mask per *text*, not per *transform*. `frosted_text_follows_its_letters` is the golden; before it the
renderer refused the job outright for anything but a translation.

## Clipping

A draw command *names* its clip rather than carrying one: `cmd.clip` is a
`ClipRef` into the frame's clip tree (`src/renderer/clip.rs`) and `cmd.clip()`
resolves it, at the half dozen reads in `render.rs` and `region.rs` and nowhere
else. That is what lets a cached subtree be replayed somewhere else without
dragging its scroller's viewport along (#441).

What it resolves to is unchanged. `PlacedShape` (`src/shape.rs`) holds a rounded
rect as its widget declared it plus the transform that places it, and every
consumer that can invert the placement and test in the clip's own space does:
the shape shader, both quad pipelines — images and transformed text draw the
same quad and take the same `QuadClip::shape` — and the region tessellator, and
the backdrop pass, whose viewport is a box but whose fragments are tested
against the clip's shape.
`world_aabb()` is left for glyphon alone, whose `TextBounds` is four integers
— and it is exact there, because a text the box would cut differently from the
shape is routed to the quad instead. `PlacedShape::box_cuts_like_the_shape` is
what decides: a turned clip always differs, an upright one only inside its
corner boxes, and a label in the middle of a rounded card is cut the same
either way and stays with glyphon (#405).

Five pipelines, so a change to clipping has to be checked in all of them.
`clipped_images`, `clipped_text`, `backdrop_blur_is_cut_by_its_scroller` and
`frosted_text_is_cut_by_its_scroller` are the goldens; before each existed,
nothing drew an image, a clipped text, or a clipped effect of either kind at
all.

## Regions

A `wl_region` is a union of rectangles, and `src/region.rs` is where a shape
becomes one — for the compositor's blur and for the input region, from the same
function. It takes the shape and its transform and cuts bands out of the
transformed outline, so a turned container claims what it drew rather than the
box around it. The clip is a shape too and is intersected scanline by scanline
rather than as a box — but what arrives is the one shape `cmd.clip()` resolves
to, already collapsed by `intersect_clips` — exactly when the two spaces are a
scale, a quarter turn or a mirror apart, and to the box around both at any
other angle.

A scanline's answer is a *list* of spans, not one. Every curvature but the
scoop bounds a convex region, which a horizontal line meets once; a scoop is
the box minus a disc at each corner, so a turned one can be entered, left
through a bite and entered again on the same line. `Outline::bounds_at` gives
the convex part and `Outline::take_bites_from` removes what each disc covers —
the *union* of it across the whole band, because a sheared disc is an ellipse
whose chord slides sideways as it grows.

The backdrop blur's *mask* is the same story inside the renderer: the viewport
is the box, the mask is the shape, tested in the container's own space.
`backdrop_blur_follows_its_shape` is the only golden that draws a backdrop blur
at all — before it, the entire pass had no pixel watching it.

## Backdrop

A backdrop effect samples pixels already drawn, which a pass cannot do to its
own attachment: a frame that uses one draws into an offscreen target and is
blitted over at the end. Frames without one draw straight to the swapchain and
never allocate it.

## Rendering somewhere other than a screen

`Renderer::render(target, commands, layers, clear)` takes a `RenderTarget`, and
a frame lands in one of two places. `RenderTarget::Swapchain` is the
compositor's: acquired per frame, presented after, and able to fail — `render`
returns `false` for a lost or outdated swapchain, which is common right after a
resize. `RenderTarget::Offscreen` is an `OffscreenTarget`, a texture the caller
owns and nobody shows; it cannot fail that way because it is already there.
Everything upstream draws the same commands either way.

The offscreen half is behind `#[cfg(any(test, feature = "testing"))]`, so a test
outside the crate turns on the `testing` feature to reach it. Read a frame back
with `OffscreenTarget::read_pixel(x, y)`, which owns the row padding wgpu
requires and the map-then-poll order — written correctly once here rather than
copied wrongly per caller.

`Renderer::render_to_view(view, width, height, commands, layers, clear)` is the
layer under both: it draws into a texture view and knows nothing about where the
view came from. `tests/golden_images.rs` still calls it and builds its own
target. Everything since goes through `RenderTarget::offscreen(&gpu, width,
height)` instead — `Headless` in `src/testing.rs` builds one per surface, which
is what `tests/headless_app.rs` drives without naming it. That is the path to
follow: it is the same `render` the compositor branch runs, and a third way to
build a target is how the two drift.

## Performance

Claims about performance need a measurement, not a story. `render-stats` is a
cargo feature; `GUIDO_LAYOUT_STATS=1` reports layout work; tracy captures are
available. Measure before and after, and put both numbers in the pull request.

A scrolling claim has somewhere to come from. `benches/scroll_list` plays a
scripted gesture over the rows `examples/bench_list` shows — no compositor and
no hand on a wheel — and prints what each phase cost:

```bash
cargo bench --bench scroll_list --features testing,render-stats -- 5000
```

Run it on the revision before and the revision after, and diff the two tables.
The counts repeat exactly, which is what makes the diff readable at all: a
hand-scrolled run of the same example polled 136714 frames one time and 83209
the next, and neither average described the other's workload.

Under the table is what the run cost the heap, which is the number a change that
exists to stop allocating has to move. It is counted by `CountingAllocator`, a
`GlobalAlloc` the benchmark binary installs — guido ships it and installs
nothing, because a `#[global_allocator]` belongs to the final binary — and it is
printed over two windows. The frame figures are the scripted frames' own,
sampled by `render_stats` at each end of a frame; the play figures cover
everything from before the tree exists. A change to what a frame does shows in
both and loudest in the first; a change to how a widget is *built* shows only in
the second. The live-byte lines are a level rather than a total, and the peak is
measured from where the play found it.

Two things it will not tell you. The microseconds are the machine's as much as
the code's, so a loaded machine has nothing to say — and nothing asserts them,
so no CI job can fail on them. And the **GPU phase is printed apart, under the
adapter that drew it**: headless takes whatever adapter is present, lavapipe
included, and a frame's cost there is a fact about lavapipe. The CPU phases —
paint, flatten, cache — are the comparable ones.
