# Rendering Pipeline

This page explains how Guido renders widgets to the screen.

## Pipeline Overview

```
1. Animation Advancement
   widget.advance_animations() → Update animation states

2. Layout Pass
   widget.layout(constraints) → Size (skipped if cached)

3. Paint Pass
   widget.paint(ctx) → Shapes added to PaintContext

4. GPU Submission
   PaintContext → Vertex/Index buffers → GPU

5. Render Order
   Background shapes → Text → Overlay shapes
```

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
fn paint(&self, ctx: &mut PaintContext) {
    // Draw background
    ctx.draw_rounded_rect(self.bounds, self.background, self.corner_radius);

    // Draw border
    ctx.draw_border(self.bounds, self.border_width, self.border_color);

    // Paint children
    for child in &self.children {
        child.paint(ctx);
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
        ctx.apply_transform_with_origin(self.user_transform, self.transform_origin);
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

Before layout, the render loop calls `advance_animations()` on the widget tree:

```rust
// Main loop each frame:
widget.advance_animations();  // Update animation states
widget.layout(constraints);   // Calculate sizes (may skip if cached)
widget.paint(ctx);            // Render to screen
```

This allows animations to update their internal state (e.g., spring physics,
time-based easing) before layout reads the animated values.

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
