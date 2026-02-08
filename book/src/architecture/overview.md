# System Overview

This page details Guido's module structure and key types.

## Module Structure

### `reactive/` - Reactive System

Single-threaded reactive primitives inspired by SolidJS.

**Key Types:**
- `Signal<T>` - Reactive values with automatic dependency tracking
- `Memo<T>` - Eager derived values that only notify on actual changes
- `Effect` - Side effects that re-run on changes
- `MaybeDyn<T>` - Enum for static or dynamic property values

**How It Works:**

The runtime uses thread-local storage for dependency tracking. When a signal is read inside a `Memo`, `Effect`, or during widget `paint()`/`layout()`, it registers as a dependency.

```rust
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);
// Runtime knows doubled depends on count
```

### `widgets/` - UI Components

**Container** (`widgets/container.rs`)

The primary building block supporting:
- Padding, backgrounds (solid/gradient)
- Corners with superellipse curvature
- Borders with SDF rendering
- Shadows (elevation)
- Transforms
- State layers (hover/pressed)
- Ripple effects
- Event handlers
- Pluggable layouts

**Text** (`widgets/text.rs`)

Text rendering with:
- Reactive content
- Font styling (size, weight, color)
- Wrapping control

**Layout** (`widgets/layout.rs`)

Pluggable layouts via the `Layout` trait:

```rust
pub trait Layout {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size;
}
```

Built-in: `Flex` for row/column layouts.

### `renderer/` - GPU Rendering

**Components:**
- `Renderer` - GPU resource management
- `PaintContext` - Accumulates shapes during painting
- WGSL shaders for SDF-based rendering

**Features:**
- Superellipse corners (CSS K-values)
- SDF borders for crisp anti-aliasing
- Linear gradients
- Clipping
- Transform support
- HiDPI scaling

### `platform/` - Wayland Integration

**Features:**
- Smithay-client-toolkit for protocols
- Layer shell (Top, Bottom, Overlay, Background)
- Anchor edges and exclusive zones
- Event loop via calloop

### `transform.rs` - 2D Transforms

4x4 matrices for 2D operations:

```rust
Transform::translate(x, y)
Transform::rotate_degrees(deg)
Transform::scale(s)
t1.then(&t2)  // Composition
t.inverse()   // Inversion
```

### `transform_origin.rs` - Pivot Points

Define rotation/scale pivot:

```rust
TransformOrigin::CENTER
TransformOrigin::TOP_LEFT
TransformOrigin::custom(0.25, 0.75)
```

## Widget Trait

All widgets implement:

```rust
pub trait Widget {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size;
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext);
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse;
}
```

Widgets access children through the `Tree` parameter, which provides centralized widget storage and layout metadata. Widget bounds and origins are stored in the `Tree` (use `tree.get_bounds(id)` and `tree.set_origin(id, x, y)`).

## Constraints System

Parent passes constraints to children:

```rust
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}
```

Children choose a size within constraints.

## Key Files Reference

| File | Purpose |
|------|---------|
| `src/lib.rs` | App entry, main loop |
| `src/widgets/container.rs` | Container implementation |
| `src/widgets/state_layer.rs` | State layer types |
| `src/renderer/mod.rs` | Renderer, GPU setup |
| `src/renderer/types.rs` | Shape types (Gradient, Shadow) |
| `src/renderer/shader.wgsl` | GPU shaders |
| `src/reactive/signal.rs` | Signal implementation |
| `src/transform.rs` | Transform matrices |
| `src/platform/mod.rs` | Wayland integration |

## Performance Considerations

### Buffer Reuse

`PaintContext` uses pre-allocated buffers cleared each frame, avoiding per-frame allocations.

### Reactive Efficiency

Signals only notify dependents when values actually change. The render loop reads current values without recreating widgets.

### GPU Batching

Shapes batch into vertex/index buffers for efficient GPU submission. Text uses glyphon's atlas system.
