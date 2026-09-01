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
    pub id: NodeId,                          // Unique identifier (matches widget ID)
    pub bounds: Rect,                        // Local bounds for transform origin
    pub local_transform: Transform,          // Transform relative to parent
    pub parent_position: Transform,          // Position set by parent (for cache reuse)
    pub pivot: Pivot,                        // What rotation and scale act about
    pub commands: SmallVec<[Rc<DrawCommand>; 2]>,        // Draw commands (shapes, text, images)
    pub children: Vec<Rc<RenderNode>>,       // Child nodes, Rc-shared with the paint cache
    pub overlay_commands: SmallVec<[Rc<DrawCommand>; 1]>, // Commands drawn after children
    pub clip: Option<ClipRegion>,            // Clips this node and children
    pub overlay_clip: Option<ClipRegion>,    // Clips only overlay commands
    pub repainted: Cell<bool>,               // true = freshly painted this frame
    pub partial: bool,                       // Some children were not painted (do not cache)
    pub cached_flatten: RefCell<Option<Rc<CachedFlatten>>>, // Cached flatten output
}
```

### Sharing Model

Children are `Rc`-shared. The paint cache (`Tree::cache_paint`) stores an `Rc`
to the same node that sits in the frame's render tree:

- **Caching** a painted subtree is a refcount bump, not a deep clone.
- **Reusing** a clean child whose position didn't change is `Rc::clone` —
  zero copies. If it moved, only the node header is cloned (children and
  commands stay shared).
- The per-frame flags use interior mutability (`Cell` / `RefCell`) because
  cached nodes are shared: `repainted` is cleared once the node's output has
  been cached, and flatten writes `cached_flatten` through a `&RenderNode`.
- `partial` propagates to ancestors during the cache walk: a subtree that
  embeds a partially-painted node — children narrowed away by the visible
  window, or culled one at a time beneath it — is never cached, so a
  later cache reuse cannot resurrect an incomplete paint. It also invalidates:
  `Tree::clear_cached_paint` drops whatever the last complete paint left, which
  is a picture of the widget as it no longer is — a list that scrolls culls
  from its first scrolled frame onward, so the newest complete entry is the
  list at rest.

Each surface owns a single root `RenderNode` (`ManagedSurface::root_node`),
cleared and rebuilt from dirty widgets every rendered frame.

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
ctx.apply_transform_with_pivot(transform, Pivot::CENTER);

// Set transform origin only
ctx.set_pivot(Pivot::TOP_LEFT);
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
ctx.draw_rounded_rect_full(rect, color, radius, curvature, border, shadow, gradient);

// Border frame (no fill)
ctx.draw_border_frame_with_curvature(rect, border_color, radius, border_width, curvature);

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

The `flatten_root_into()` function converts the hierarchical render tree into a flat list of `FlattenedCommand`s ready for GPU submission, reusing the output buffer's capacity across frames.

### World Transform Computation

```rust
// For each node:
let local_centered = node.local_transform.center_at(origin_x, origin_y);
let world_transform = parent_world_transform.then(&local_centered);
```

The transform origin is resolved from the node's bounds and used to center the transform operation.

### RenderLayer Ordering

Commands are bucketed by kind so that commands of a kind share a draw call:

```rust
pub enum RenderLayer {
    Shapes = 0,   // Background shapes (rectangles, borders)
    Images = 1,   // Image content
    Text = 2,     // Text content
    Overlay = 3,  // Overlay effects (ripples, highlights)
}
```

### Draw groups

Bucketing on its own reorders drawing: with one set of buckets for the whole
frame, *every* shape is drawn before *every* image, so a container background
painted over an image ends up underneath it however the tree is arranged.

The flattener therefore emits a sequence of **draw groups**. Each group has its
own four buckets, and a new group is opened whenever a command's layer would go
backwards:

```rust
pub struct CommandLayer {
    pub shapes: Range<usize>,
    pub images: Range<usize>,
    pub text: Range<usize>,
    pub overlay: Range<usize>,
}
```

The renderer walks the groups in order and draws each one bucket by bucket.
Batching survives, ordering survives with it, and a tree that never paints a
lower layer over a higher one produces exactly one group — the same single set
of draw calls as before. The split is minimal by construction, so no merge pass
is needed afterwards.

A layer going backwards is only split on when the incoming command's world
bounds actually intersect something already drawn above it in that group.
Without that test the rule fires between every pair of sibling widgets — each
paints a background then a label, so the next sibling's background regresses —
and `examples/showcase.rs` alone went to 16 groups, every one of them carrying
its own glyphon renderer. Siblings do not overlap, so those splits bought
nothing. With the test, `status_bar` is one group and `showcase` is four.

Each layer of a group tracks its commands' rects individually, not just their
union: labels scattered across a panel union into a box spanning the gaps
between them, and the next sibling's background lands in a gap and splits for
nothing. Past `MAX_TRACKED_RECTS` the union decides alone, which can only
over-split. With the exact test, `status_bar` and `showcase` are one group each
— the same draw calls as before groups existed.

Bounds are conservative: shapes grow by their shadow and border, text by half a
font size for glyph overshoot, and anything under a rotation or scale counts as
covering everything. Over-splitting costs a draw call; under-splitting would
draw in the wrong order.

glyphon draws everything a `TextRenderer` prepared in one call, so a group with
directly-rendered text gets a renderer of its own from a pool that grows on
demand. `examples/draw_order_example.rs` exercises the regressions.

### Backdrop effects

`RenderLayer::Backdrop` sits below `Shapes` so a backdrop command always opens
a group: everything it reads must already have been drawn, which means it must
land after the groups holding that content.

A backdrop effect samples the render target, which a pass cannot do to its own
attachment. When a frame contains one, it is drawn into an offscreen colour
target instead of the swapchain; at each backdrop group the pass ends, the
effect runs, and the pass resumes with `LoadOp::Load`. The target is blitted to
the swapchain once at the end and released after enough idle frames.

Per region: downsample into a quarter-size working texture (rendering into a
smaller target with a linear sampler is already a box filter), two separable
gaussian passes ping-ponging between two working textures, then a composite
back over the scene masked by the container's rounded-rect SDF. See
`src/renderer/backdrop_pass.rs`.

The mask is the one part a caller replaces. `Text::backdrop_blur` emits
`DrawCommand::TextBackdropBlur`, which resolves to the same blur with a
coverage texture in place of the SDF: the glyphs are rasterized into it by
`src/renderer/text_mask.rs`, shaped exactly as `TextRenderState` shapes the
text drawn afterwards, one texel per pixel of a region snapped to the pixel
grid. Sub-pixel position is handed to the mask as its glyph origin rather than
rounded away, and a text under a rotation or scale is skipped — the mask is
axis-aligned.

### FlattenedCommand

The output of tree flattening:

```rust
pub struct FlattenedCommand {
    pub command: Rc<DrawCommand>,   // Shared with the render node — no deep clone
    pub world_transform: Transform,
    pub world_transform_origin: Option<(f32, f32)>,
    pub layer: RenderLayer,
    pub clip: Option<WorldClip>,
    pub clip_is_local: bool,
}
```

### Incremental Flatten

The flattener caches results per node to avoid re-flattening clean subtrees:

```rust
pub struct CachedFlatten {
    pub commands: Vec<FlattenedCommand>,  // Flattened output from this subtree
    pub world_transform: Transform,       // World transform at time of caching
}
```

When a `RenderNode` has `repainted == false` (reused from paint cache) and both the
cached and current world transforms are translation-only, the flattener reuses cached
commands with a (dx, dy) offset instead of recursing into children. After a full flatten,
results are cached back onto the node for next frame.

The cache lives in `RefCell<Option<Rc<CachedFlatten>>>` on the node, so flatten
only needs `&RenderNode` and shallow node clones share the cached output.

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
fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
    // Get bounds from Tree (single source of truth)
    let bounds = tree.get_bounds(id).unwrap_or_default();

    // Set local bounds (0,0 origin with widget dimensions)
    let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
    ctx.set_bounds(local_bounds);

    // Apply user transform if set (parent already set position via set_transform)
    if !self.transform.is_identity() {
        ctx.apply_transform_with_pivot(self.transform, self.pivot);
    }

    // Draw background in LOCAL coordinates
    ctx.draw_rounded_rect(local_bounds, self.background, self.corner_radius);

    // Paint children - set their position, then let them apply their own transforms
    for &child_id in self.children.iter() {
        // Get child bounds from Tree - these are in LOCAL coordinates (relative to parent)
        let child_bounds = tree.get_bounds(child_id).unwrap_or_default();
        let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);

        let mut child_ctx = ctx.add_child(child_id.as_u64(), child_local);
        child_ctx.set_transform(Transform::translate(child_bounds.x, child_bounds.y));
        tree.with_widget(child_id, |child| {
            child.paint(tree, child_id, &mut child_ctx);
        });
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
| `src/renderer/tree.rs` | RenderNode, ClipRegion, CachedFlatten |
| `src/renderer/paint_context.rs` | PaintContext API |
| `src/renderer/commands.rs` | DrawCommand enum |
| `src/renderer/flatten.rs` | Tree flattening with transform inheritance |
| `src/renderer/gpu.rs` | ShapeInstance, GPU data structures |
| `src/renderer/render.rs` | Main Renderer, GPU pipeline |
| `src/renderer/shader.wgsl` | WGSL shaders for SDF rendering |
| `src/renderer/text.rs` | Text rendering via glyphon |
| `src/renderer/text_quad.rs` | Transformed text as textured quads |
| `src/renderer/image_quad.rs` | Image rendering |
