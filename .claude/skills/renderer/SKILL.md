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
    Partial paints poison their ancestors and are never cached

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
`load_font`. Text metrics depend on the fonts installed on the machine —
which is why the render-tree snapshots exclude text, and why the goldens do not
render any yet.

## Backdrop

A backdrop effect samples pixels already drawn, which a pass cannot do to its
own attachment: a frame that uses one draws into an offscreen target and is
blitted over at the end. Frames without one draw straight to the swapchain and
never allocate it.

## Rendering somewhere other than a screen

`Renderer::render_to_view(view, width, height, commands, layers, clear)` draws a
frame into any texture. `render()` is that, plus acquiring a swapchain texture
and presenting. This is what the golden image tests use, and it means anything
in this pipeline can be tested without a compositor.

## Performance

Claims about performance need a measurement, not a story. `render-stats` is a
cargo feature; `GUIDO_LAYOUT_STATS=1` reports layout work; tracy captures are
available. Measure before and after, and put both numbers in the pull request.
