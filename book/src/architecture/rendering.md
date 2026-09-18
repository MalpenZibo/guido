# Rendering Pipeline

This page explains how Guido renders widgets to the screen.

## Pipeline Overview

```text
Main loop (once per iteration):
 1. init_pending_gpu()             → Give any surface born this iteration a
                                     render target and its first layout, at
                                     this iteration's instant — which is what
                                     an enter animation is seeded from
 2. flush_bg_writes()              → Drain queued background-thread signal writes
 3. take_frame_request()           → Check if a frame was requested

Per-surface rendering:
 4. Dispatch events                → Route input events (MouseMoves coalesced),
                                     each one declaring when the compositor saw
                                     it happen — not when this loop got to it
 5. Frame-pacing gate              → Return if the compositor hasn't shown the
                                     previous frame yet (jobs stay queued)
 6. set_frame_instant(now)         → What time it is, for this whole frame:
                                     everything below is asked about this one
                                     moment rather than reading a clock of its
                                     own (cleared again after step 14)
 7. drain_pending_jobs()           → Process jobs: unregister, advance
    + process_jobs()                 animations, reconcile children, mark dirty
 8. Partial layout                 → Only dirty subtrees re-layout
 9. Skip-frame check               → Skip paint if root is clean
10. Tree::paint_widget()           → Build render tree (Rc cache reuse for clean children)
11. flatten_root_into()            → Flatten to draw commands (incremental for clean subtrees)
12. frame() + damage_buffer()      → Re-arm the frame callback and report damage,
                                     both BEFORE presenting
13. GPU rendering + present()      → Instanced SDF shapes with HiDPI scaling;
                                     present() commits the surface
14. cache_paint_results()          → Rc-share rendered nodes into the cache,
                                     clear needs_paint flags
```

Rendering is paced by the compositor's `wl_surface.frame` callbacks: after each
present, a new callback is requested, and the surface won't render again until
it fires. An animating surface renders exactly once per compositor frame; an
idle surface renders nothing and the loop sleeps.

## Layout Pass

The main loop calls layout with screen constraints:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let constraints = Constraints {
    min_width: 0.0,
    max_width: screen_width,
    min_height: 0.0,
    max_height: screen_height,
};

widget.layout(constraints);
# ;
# }
```

Each widget:
1. Calculates its preferred size within constraints
2. Positions children (if any)
3. Returns its final size

## Paint Pass

After layout, widgets paint to the `PaintContext`:

```rust,ignore
fn paint(&self, ctx: &mut PaintContext) {
    // Bounds come from the Tree — the single source of truth — and paint
    // happens in LOCAL coordinates, the parent having placed the node.
    let (tree, id) = (ctx.tree(), ctx.id());
    let bounds = tree.get_bounds(id).unwrap_or_default();
    let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);

    // Background and border are one command: a rounded rect carries its own
    // border, shadow and gradient.
    ctx.draw_rounded_rect(local, self.background, self.corner_radius);

    // Paint children. One call each, and it is the framework's: it culls a
    // child nobody can see, serves a clean one from its cache, places it, and
    // opens its Paint scope.
    for &child_id in self.children.iter() {
        ctx.paint_child(child_id, &ChildPaintOptions::default());
    }
}
```

`PaintContext` accumulates:
- **Shapes** - Rectangles, rounded rects, gradients
- **Text** - Glyphs for text rendering
- **Overlay shapes** - Ripples, effects on top of content

## HiDPI Scaling

The renderer converts logical coordinates to physical pixels:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let physical_x = logical_x * scale_factor;
let physical_y = logical_y * scale_factor;
# ;
# }
```

Widgets work in logical coordinates; scaling is automatic.

## SDF Rendering

Shapes use Signed Distance Field techniques:

```wgsl
// In shader
let dist = sdf_rounded_rect(uv, size, radius, k_value);
let alpha = smoothstep(0.0, -pixel_width, dist);
```

Benefits:
- Resolution-independent anti-aliasing
- Crisp edges at any scale
- Superellipse corner support

## Render Order

Shapes render in three layers:

1. **Background layer** - Container backgrounds, borders
2. **Text layer** - Text content
3. **Overlay layer** - Ripple effects, state layer overlays

This ensures ripples appear on top of text.

## Shape Types

### Rounded Rectangle

```rust,ignore
struct RoundedRect {
    bounds: Rect,
    color: Color,
    corner_radius: f32,
    corner_curvature: f32,  // K-value
}
```

### Gradient

```rust,ignore
struct GradientRect {
    bounds: Rect,
    start_color: Color,
    end_color: Color,
    direction: GradientDirection,
}
```

### Border

Rendered as SDF outline:

```rust,ignore
struct Border {
    bounds: Rect,
    width: f32,
    color: Color,
    corner_radius: f32,
}
```

## Transform Handling

The render tree handles transforms hierarchically:

```rust,ignore
fn paint(&self, ctx: &mut PaintContext) {
    // Get bounds from Tree (single source of truth)
    let bounds = ctx.tree().get_bounds(ctx.id()).unwrap_or_default();

    // Apply user transform (rotation, scale) if set
    if !self.user_transform.is_identity() {
        ctx.apply_transform_with_pivot(self.user_transform, self.pivot);
    }

    // Paint content in LOCAL coordinates (0,0 is widget origin)
    let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
    ctx.draw_rounded_rect(local_bounds, Color::BLUE, 8.0);

    // Paint children. `paint_child` places each one from its bounds in the
    // tree, so a parent that wants the ordinary placement writes nothing.
    for &child_id in self.children.iter() {
        ctx.paint_child(child_id, &ChildPaintOptions::default());
    }
}
```

Transforms are inherited through the render tree hierarchy. Each node has a local transform that is composed with its parent's world transform during tree flattening.

## Text Rendering

Text uses the glyphon library:

1. Text widget provides content and style
2. Glyphon lays out glyphs
3. Glyphs render from a texture atlas
4. Correct blending with background

## Clipping

Containers set a clip region for their content:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let bounds = tree.get_bounds(id).unwrap_or_default();
# let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
// Set clip for this node and all children (in local coordinates)
ctx.set_clip(local_bounds, self.corner_radius, self.corner_curvature);

// For overlay-only clipping (e.g., ripple effects)
ctx.set_overlay_clip(local_bounds, self.corner_radius, self.corner_curvature);
# ;
# }
```

Clipping respects corner radius and curvature for proper rounded container clipping. Clip regions are inherited through the render tree and transformed along with their parent nodes.

## Animation Advancement

Animations advance during per-surface job processing, before layout and paint,
so the frame being built always renders with up-to-date animation values. Each
advance pushes a continuation job (`Animation` plus a `Paint` or `Layout`
follow-up when the value changed), which schedules the next advancement.

Continuation jobs are throttled by the frame-pacing gate: while the compositor
hasn't shown the previous frame, they stay queued, so animations step once per
displayed frame instead of once per loop iteration. Widgets like TextInput use
this mechanism to drive cursor blinking.

## Performance Notes

### Vertex Buffer Reuse

PaintContext reuses buffers between frames:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
self.vertices.clear();  // Reuse allocation
self.indices.clear();   // Reuse allocation
# ;
# }
```

### Batching

Similar shapes batch together to reduce draw calls. Text renders in a single pass using the glyph atlas.

### Layout Optimization

The layout system includes several optimizations:

**Relayout Boundaries**: Widgets with fixed width and height are relayout boundaries.
Layout changes inside don't propagate to the parent, limiting recalculation scope.

**Layout Caching**: Layout results are cached. The system uses reactive version tracking
to detect when signals have changed. Layout only runs when:
- Constraints change
- Animations are active
- Reactive state (signals) update

**Paint-Only Scrolling**: Scroll is implemented as a transform operation during paint,
not a layout change. When content scrolls:
1. Scroll offset is stored as a transform
2. Transform is applied during paint phase
3. Children render at their original layout positions
4. The transform shifts content visually
5. Clip bounds are adjusted for correct clipping

This means scrolling doesn't trigger layout, significantly reducing CPU overhead.

### Paint Caching and Damage Regions

The rendering pipeline includes several optimizations to avoid redundant work:

**Partial Paint**: Each widget in the Tree has a `needs_paint` flag. When a widget's
visual state changes (e.g., a signal update triggers a Paint job), the flag propagates
upward to ancestors. During paint, Container checks each child's flag — clean children
reuse their cached `RenderNode` from the previous frame. The cache is `Rc`-shared with
the render tree: reusing an unmoved child is a refcount bump, and a moved child clones
only the node header (its subtree and draw commands stay shared).

**Skip Frame**: If no widget needs paint after job processing and layout, the entire
paint→flatten→render cycle is skipped for that surface.

**Damage Regions**: As widgets are marked for paint, their surface-relative bounds are
accumulated into a per-surface `DamageRegion` (None, Partial, or Full). The damage is
set as pending Wayland state before presenting — `present()` commits it together with
the new buffer — allowing the compositor to optimize its own compositing.

**Incremental Flatten**: The flattener caches its output per `RenderNode`. Clean subtrees
(where `repainted == false`) reuse their cached flattened commands with a translation
offset, avoiding the cost of recursing into unchanged subtrees.
