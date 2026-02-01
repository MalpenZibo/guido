# Renderer

This document provides a developer reference for Guido's GPU rendering system.

## Overview

Guido uses a hierarchical render tree architecture where widgets paint to their own nodes using local coordinates. Transforms are automatically inherited from parent to child during tree flattening, eliminating confusion from manual push/pop operations.

**Key Benefits:**
- Widgets always paint at (0,0) in local coordinates
- Transform inheritance happens automatically
- Clean separation between painting and coordinate transformation
- Proper clipping with rounded corners

## Render Tree Architecture

### RenderNode

Each widget creates a `RenderNode` containing its visual output:

```rust
pub struct RenderNode {
    pub id: NodeId,                         // Unique identifier (matches widget ID)
    pub bounds: Rect,                       // Local bounds for transform origin
    pub local_transform: Transform,         // Transform relative to parent
    pub transform_origin: TransformOrigin,  // Pivot point for transforms
    pub commands: Vec<DrawCommand>,         // Draw commands (shapes, text, images)
    pub children: Vec<RenderNode>,          // Child nodes
    pub overlay_commands: Vec<DrawCommand>, // Commands drawn after children
    pub clip: Option<ClipRegion>,           // Clips this node and children
    pub overlay_clip: Option<ClipRegion>,   // Clips only overlay commands
}
```

### RenderTree

The complete render tree for a frame:

```rust
pub struct RenderTree {
    pub roots: Vec<RenderNode>,  // Root nodes (one per surface)
}
```

### Local Coordinate System

Widgets paint in local coordinates where (0,0) is the widget's top-left corner. The parent widget sets the child's position via `set_transform()` before calling `paint()`.

### ClipRegion

Defines a clip area for a node and its children:

```rust
pub struct ClipRegion {
    pub rect: Rect,         // Clip rectangle in local coordinates
    pub corner_radius: f32, // Corner radius for rounded clipping
    pub curvature: f32,     // Superellipse curvature (K-value)
}
```

## PaintContext API

`PaintContext` is the interface widgets use to build their render nodes.

### Node Properties

```rust
// Set bounds (for transform origin resolution)
ctx.set_bounds(Rect::new(0.0, 0.0, width, height));

// Set transform (replaces existing)
ctx.set_transform(Transform::translate(x, y));

// Apply transform (composes with existing): result = existing.then(transform)
ctx.apply_transform(Transform::rotate_degrees(45.0));

// Apply transform with origin
ctx.apply_transform_with_origin(transform, TransformOrigin::CENTER);

// Set transform origin only
ctx.set_transform_origin(TransformOrigin::TOP_LEFT);
```

### Clipping

```rust
// Set clip for this node and children
ctx.set_clip(rect, corner_radius, curvature);

// Set rectangular clip (no rounded corners)
ctx.set_clip_rect(rect);

// Set clip only for overlay commands (doesn't clip children)
ctx.set_overlay_clip(rect, corner_radius, curvature);
```

### Draw Commands

```rust
// Rounded rectangle (basic)
ctx.draw_rounded_rect(rect, color, radius);

// Rounded rectangle with curvature
ctx.draw_rounded_rect_with_curvature(rect, color, radius, curvature);

// Gradient rectangle
ctx.draw_gradient_rect(rect, gradient, radius, curvature);

// Border frame (no fill)
ctx.draw_border_frame(rect, border_color, radius, border_width);

// With shadow
ctx.draw_rounded_rect_with_shadow(rect, color, radius, curvature, shadow);

// Full configuration
ctx.draw_rounded_rect_full(rect, color, radius, curvature, border, shadow, gradient);

// Circle
ctx.draw_circle(cx, cy, radius, color);

// Text
ctx.draw_text(text, rect, color, font_size);
ctx.draw_text_styled(text, rect, color, font_size, font_family, font_weight);

// Image
ctx.draw_image(source, rect, content_fit);
```

### Children

```rust
// Add a child and get its paint context
let mut child_ctx = ctx.add_child(child_id, child_bounds);
child_ctx.set_transform(Transform::translate(offset_x, offset_y));
child.paint(&mut child_ctx);
```

### Overlay Commands

Overlay commands are drawn after all children, useful for effects like ripples:

```rust
ctx.draw_overlay_circle(cx, cy, radius, color);
ctx.draw_overlay_rounded_rect(rect, color, radius);
```

## Tree Flattening

The `flatten_tree()` function converts the hierarchical `RenderTree` into a flat list of `FlattenedCommand`s ready for GPU submission.

### World Transform Computation

```rust
// For each node:
let local_centered = node.local_transform.center_at(origin_x, origin_y);
let world_transform = parent_world_transform.then(&local_centered);
```

The transform origin is resolved from the node's bounds and used to center the transform operation.

### RenderLayer Ordering

Commands are sorted by layer for correct render order:

```rust
pub enum RenderLayer {
    Shapes = 0,   // Background shapes (rectangles, borders)
    Images = 1,   // Image content
    Text = 2,     // Text content
    Overlay = 3,  // Overlay effects (ripples, highlights)
}
```

### FlattenedCommand

The output of tree flattening:

```rust
pub struct FlattenedCommand {
    pub command: DrawCommand,
    pub world_transform: Transform,
    pub world_transform_origin: Option<(f32, f32)>,
    pub layer: RenderLayer,
    pub clip: Option<WorldClip>,
    pub clip_is_local: bool,
}
```

## GPU Rendering Pipeline

### Instanced Rendering

The renderer uses instanced rendering for efficiency: a single draw call per layer renders all shapes using one shared unit quad and per-instance data.

### ShapeInstance

Per-instance data for each shape (224 bytes):

```rust
pub struct ShapeInstance {
    pub rect: [f32; 4],           // [x, y, width, height] in physical pixels
    pub corner_radius: f32,       // Corner radius
    pub shape_curvature: f32,     // Superellipse K-value
    pub fill_color: [f32; 4],     // RGBA
    pub border_color: [f32; 4],   // RGBA
    pub border_width: f32,
    pub shadow_offset: [f32; 2],
    pub shadow_blur: f32,
    pub shadow_spread: f32,
    pub shadow_color: [f32; 4],
    pub transform: [f32; 6],      // 2x3 affine matrix [a, b, tx, c, d, ty]
    pub clip_rect: [f32; 4],      // Clip region
    pub clip_corner_radius: f32,
    pub clip_curvature: f32,
    pub clip_is_local: f32,       // 1.0 for local, 0.0 for world
    pub gradient_start: [f32; 4],
    pub gradient_end: [f32; 4],
    pub gradient_type: u32,       // 0=none, 1=horizontal, 2=vertical, 3/4=diagonal
}
```

### HiDPI Scaling

All coordinates are scaled to physical pixels during instance creation:

```rust
instance.rect = [rect.x * scale, rect.y * scale, rect.width * scale, rect.height * scale];
instance.corner_radius = radius * scale;
```

### Render Order

1. **Shapes** - Background rectangles, borders, shadows
2. **Images** - Image quads via `ImageQuadRenderer`
3. **Text** - Regular text via glyphon, transformed text via `TextQuadRenderer`
4. **Overlay** - Ripple effects and highlights

## Example: Implementing paint()

```rust
fn paint(&self, ctx: &mut PaintContext) {
    // Set local bounds (0,0 origin with widget dimensions)
    let local_bounds = Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height);
    ctx.set_bounds(local_bounds);

    // Apply user transform if set (parent already set position via set_transform)
    if !self.transform.is_identity() {
        ctx.apply_transform_with_origin(self.transform, self.transform_origin);
    }

    // Draw background in LOCAL coordinates
    ctx.draw_rounded_rect(local_bounds, self.background, self.corner_radius);

    // Paint children - set their position, then let them apply their own transforms
    for child in &self.children {
        let child_global = child.bounds();
        let child_local = Rect::new(0.0, 0.0, child_global.width, child_global.height);

        // Calculate offset from parent's origin to child's position
        let offset_x = child_global.x - self.bounds.x;
        let offset_y = child_global.y - self.bounds.y;

        let mut child_ctx = ctx.add_child(child.id(), child_local);
        child_ctx.set_transform(Transform::translate(offset_x, offset_y));
        child.paint(&mut child_ctx);  // Child applies its own transform
    }

    // Draw overlay effects (after children) in LOCAL coords
    if let Some(ripple) = &self.ripple {
        ctx.set_overlay_clip(local_bounds, self.corner_radius, self.curvature);
        ctx.draw_overlay_circle(ripple.x, ripple.y, ripple.radius, ripple.color);
    }
}
```

## Key Files

| File | Purpose |
|------|---------|
| `src/renderer/tree.rs` | RenderNode, RenderTree, ClipRegion |
| `src/renderer/paint_context.rs` | PaintContext API |
| `src/renderer/commands.rs` | DrawCommand enum |
| `src/renderer/flatten.rs` | Tree flattening with transform inheritance |
| `src/renderer/gpu.rs` | ShapeInstance, GPU data structures |
| `src/renderer/render.rs` | Main Renderer, GPU pipeline |
| `src/renderer/shader.wgsl` | WGSL shaders for SDF rendering |
| `src/renderer/text.rs` | Text rendering via glyphon |
| `src/renderer/text_quad.rs` | Transformed text as textured quads |
| `src/renderer/image_quad.rs` | Image rendering |
