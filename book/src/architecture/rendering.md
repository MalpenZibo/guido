# Rendering Pipeline

This page explains how Guido renders widgets to the screen.

## Pipeline Overview

```
1. Layout Pass
   widget.layout(constraints) → Size

2. Paint Pass
   widget.paint(ctx) → Shapes added to PaintContext

3. GPU Submission
   PaintContext → Vertex/Index buffers → GPU

4. Render Order
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

Transforms affect the paint context:

```rust
fn paint(&self, ctx: &mut PaintContext) {
    ctx.push_transform(self.transform);
    // Paint content...
    ctx.pop_transform();
}
```

Transforms accumulate through the hierarchy for nested widgets.

## Text Rendering

Text uses the glyphon library:

1. Text widget provides content and style
2. Glyphon lays out glyphs
3. Glyphs render from a texture atlas
4. Correct blending with background

## Clipping

Containers can clip children to their bounds:

```rust
ctx.push_clip(self.bounds, self.corner_radius);
// Paint children...
ctx.pop_clip();
```

Clipping respects corner radius for proper rounded container clipping.

## Performance Notes

### Vertex Buffer Reuse

PaintContext reuses buffers between frames:

```rust
self.vertices.clear();  // Reuse allocation
self.indices.clear();   // Reuse allocation
```

### Batching

Similar shapes batch together to reduce draw calls. Text renders in a single pass using the glyph atlas.

### Dirty Tracking

Currently, the entire tree re-layouts and re-paints each frame. Future optimization: relayout boundaries to skip unchanged subtrees.
