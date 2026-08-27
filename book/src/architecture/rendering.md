# Rendering Pipeline

This page explains how Guido renders widgets to the screen.

## Pipeline Overview

```
Main loop (once per iteration):
 1. flush_bg_writes()              → Drain queued background-thread signal writes
 2. take_frame_request()           → Check if a frame was requested

Per-surface rendering:
 3. Dispatch events                → Route input events (MouseMoves coalesced),
                                     each one declaring when the compositor saw
                                     it happen — not when this loop got to it
 4. Frame-pacing gate              → Return if the compositor hasn't shown the
                                     previous frame yet (jobs stay queued)
 5. set_frame_instant(now)         → What time it is, for this whole frame:
                                     everything below is asked about this one
                                     moment rather than reading a clock of its
                                     own (cleared again after step 13)
 6. drain_pending_jobs()           → Process jobs: unregister, advance
    + process_jobs()                 animations, reconcile children, mark dirty
 7. Partial layout                 → Only dirty subtrees re-layout
 8. Skip-frame check               → Skip paint if root is clean
 9. widget.paint(tree, ctx)        → Build render tree (Rc cache reuse for clean children)
10. flatten_root_into()            → Flatten to draw commands (incremental for clean subtrees)
11. frame() + damage_buffer()      → Re-arm the frame callback and report damage,
                                     both BEFORE presenting
12. GPU rendering + present()      → Instanced SDF shapes with HiDPI scaling;
                                     present() commits the surface
13. cache_paint_results()          → Rc-share rendered nodes into the cache,
                                     clear needs_paint flags
```

Rendering is paced by the compositor's `wl_surface.frame` callbacks: after each
present, a new callback is requested, and the surface won't render again until
it fires. An animating surface renders exactly once per compositor frame; an
idle surface renders nothing and the loop sleeps.

## Layout Pass

The main loop calls layout with screen constraints:

```rust
let constraints = Constraints {
    min_width: 0.0,
    max_width: screen_width,
    min_height: 0.0,
    max_height: screen_height,
};

widget.layout(constraints);
```

Each widget:
1. Calculates its preferred size within constraints
2. Positions children (if any)
3. Returns its final size

## Paint Pass

After layout, widgets paint to the `PaintContext`:

```rust
fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
    // Bounds come from the Tree — the single source of truth — and paint
    // happens in LOCAL coordinates, the parent having placed the node.
    let bounds = tree.get_bounds(id).unwrap_or_default();
    let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);

    // Background and border are one command: a rounded rect carries its own
    // border, shadow and gradient.
    ctx.draw_rounded_rect(local, self.background, self.corner_radius);

    // Paint children
    for &child_id in self.children.iter() {
        tree.with_widget(child_id, |child| child.paint(tree, child_id, ctx));
    }
}
```

`PaintContext` accumulates:
- **Shapes** - Rectangles, rounded rects, gradients
- **Text** - Glyphs for text rendering
- **Overlay shapes** - Ripples, effects on top of content

## HiDPI Scaling

The renderer converts logical coordinates to physical pixels:

```rust
let physical_x = logical_x * scale_factor;
let physical_y = logical_y * scale_factor;
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

```rust
struct RoundedRect {
    bounds: Rect,
    color: Color,
    corner_radius: f32,
    corner_curvature: f32,  // K-value
}
```

### Gradient

```rust
struct GradientRect {
    bounds: Rect,
    start_color: Color,
    end_color: Color,
    direction: GradientDirection,
}
```

### Border

Rendered as SDF outline:

```rust
struct Border {
    bounds: Rect,
    width: f32,
    color: Color,
    corner_radius: f32,
}
```

## Transform Handling

The render tree handles transforms hierarchically:

```rust
fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
    // Get bounds from Tree (single source of truth)
    let bounds = tree.get_bounds(id).unwrap_or_default();

    // Apply user transform (rotation, scale) if set
    if !self.user_transform.is_identity() {
        ctx.apply_transform_with_pivot(self.user_transform, self.pivot);
    }

    // Paint content in LOCAL coordinates (0,0 is widget origin)
    let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
    ctx.draw_rounded_rect(local_bounds, Color::BLUE, 8.0);

    // Paint children - parent sets their position transform
    for &child_id in self.children.iter() {
        // Get child bounds from Tree - in LOCAL coordinates (relative to parent)
        let child_bounds = tree.get_bounds(child_id).unwrap_or_default();
        let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);
        let mut child_ctx = ctx.add_child(child_id.as_u64(), child_local);
        child_ctx.set_transform(Transform::translate(child_bounds.x, child_bounds.y));
        tree.with_widget(child_id, |child| {
            child.paint(tree, child_id, &mut child_ctx);
        });
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

```rust
// Set clip for this node and all children (in local coordinates)
ctx.set_clip(local_bounds, self.corner_radius, self.corner_curvature);

// For overlay-only clipping (e.g., ripple effects)
ctx.set_overlay_clip(local_bounds, self.corner_radius, self.corner_curvature);
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

```rust
self.vertices.clear();  // Reuse allocation
self.indices.clear();   // Reuse allocation
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
