# Guido Architecture

This document provides an overview of Guido's architecture for developers working on or with the codebase.

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                          Application                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Widgets   │  │  Reactive   │  │       Platform          │  │
│  │  Container  │  │   Signals   │  │   Wayland Layer Shell   │  │
│  │    Text     │  │  Computed   │  │   Event Loop (calloop)  │  │
│  │   Layout    │  │   Effects   │  │                         │  │
│  └──────┬──────┘  └──────┬──────┘  └───────────┬─────────────┘  │
│         │                │                     │                 │
│         └────────────────┼─────────────────────┘                 │
│                          │                                       │
│                    ┌─────┴─────┐                                 │
│                    │  Renderer │                                 │
│                    │   wgpu    │                                 │
│                    │  glyphon  │                                 │
│                    └───────────┘                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Module Structure

### `reactive/` - Reactive System

Thread-safe reactive primitives inspired by SolidJS and Floem.

**Key Types:**
- `Signal<T>` - Reactive values with automatic dependency tracking. Signals are `Copy` (backed by `Arc`), so no cloning needed.
- `Computed<T>` - Derived values that auto-update when dependencies change
- `Effect` - Side effects that re-run when tracked signals change
- `MaybeDyn<T>` - Enum allowing properties to be static or reactive

**How it works:**
```rust
let count = create_signal(0);           // Create a signal
let doubled = create_computed(move ||   // Create derived value
    count.get() * 2
);
count.set(5);                           // doubled automatically becomes 10
```

The runtime uses thread-local storage for automatic dependency tracking. When a signal is read inside a `Computed` or `Effect`, it registers itself as a dependency.

### `widgets/` - UI Components

Composable UI primitives implementing the `Widget` trait.

**Container** (`widgets/container.rs`)
The primary building block. Supports:
- Padding, background (solid or gradient)
- Corner radius with superellipse curvature
- Borders with SDF rendering
- Shadows with elevation levels
- Transforms (translate, rotate, scale)
- State layers (hover/pressed styles)
- Ripple effects
- Event handlers (click, hover, scroll)
- Pluggable layouts via `Layout` trait

**Text** (`widgets/text.rs`)
Text rendering with:
- Reactive content (static string or `Signal<String>`)
- Font size, color, weight styling
- Text wrapping or `nowrap()` mode

**Layout System** (`widgets/layout.rs`)
Pluggable layouts via the `Layout` trait:
```rust
pub trait Layout {
    fn layout(&self, children: &mut [ChildEntry], constraints: Constraints) -> Size;
}
```

Built-in implementation:
- `Flex` - Flexbox-style row/column layout with spacing and alignment

### `renderer/` - GPU Rendering

Hardware-accelerated rendering using wgpu.

**Components:**
- `Renderer` - Main renderer managing GPU resources and render passes
- `PaintContext` - Accumulates shapes and text during widget painting
- `Vertex` / `RoundedRect` - Primitive types for GPU submission
- Custom WGSL shaders for SDF-based rendering

**Rendering Pipeline:**
1. `widget.layout(constraints)` - Calculate sizes
2. `widget.paint(ctx)` - Collect shapes into PaintContext
3. Renderer converts to GPU vertices with HiDPI scaling
4. Three-layer render order: shapes → text → overlay (ripples)

**Shape Features:**
- Rounded rectangles with configurable superellipse curvature
- CSS K-value corner styles: squircle (K=2), circle (K=1), bevel (K=0), scoop (K=-1)
- SDF-based borders for crisp anti-aliasing
- Linear gradients (horizontal, vertical, diagonal)
- Clipping to rounded regions
- Transform support with proper hit testing

### `platform/` - Wayland Integration

Layer shell protocol implementation for desktop widgets.

**Features:**
- Smithay-client-toolkit for Wayland protocols
- Layer shell positioning (Top, Bottom, Overlay, Background)
- Anchor edges (TOP, BOTTOM, LEFT, RIGHT combinations)
- Exclusive zones for panels
- Event loop via calloop

### `transform.rs` - 2D Transforms

4x4 transformation matrices for 2D operations.

**Operations:**
```rust
Transform::translate(x, y)      // Move
Transform::rotate_degrees(deg)  // Rotate
Transform::scale(s)             // Uniform scale
Transform::scale_xy(sx, sy)     // Non-uniform scale
t1.then(&t2)                    // Compose transforms
t.inverse()                     // Invert transform
t.center_at(cx, cy)             // Apply around point
```

### `transform_origin.rs` - Pivot Points

Define rotation/scale pivot points:
```rust
TransformOrigin::CENTER       // Default
TransformOrigin::TOP_LEFT
TransformOrigin::BOTTOM_RIGHT
TransformOrigin::custom(0.25, 0.75)  // 25% from left, 75% from top
```

## Widget Trait

All widgets implement this trait:

```rust
pub trait Widget {
    fn layout(&mut self, constraints: Constraints) -> Size;
    fn paint(&self, ctx: &mut PaintContext);
    fn event(&mut self, event: &Event) -> EventResponse;
    fn set_origin(&mut self, x: f32, y: f32);
    fn bounds(&self) -> Rect;
}
```

## Event Flow

```
Wayland → Platform → App → Widget Tree
                              │
                              ├─ MouseMove/Enter/Leave
                              ├─ MouseDown/MouseUp
                              └─ Scroll
```

Events propagate down the widget tree. Each widget can:
- Handle the event (`EventResponse::Handled`)
- Ignore and let parent continue (`EventResponse::Ignored`)

## State Layer System

Declarative style overrides for interaction states:

```rust
container()
    .background(base_color)
    .hover_state(|s| s.lighter(0.1))     // Override on hover
    .pressed_state(|s| s.ripple())        // Override on press
```

See [STATE_LAYER.md](./STATE_LAYER.md) for full documentation.

## Animation System

Duration-based and spring-based animations:

```rust
// Duration with easing
.animate_background(Transition::new(200.0, TimingFunction::EaseOut))

// Spring physics
.animate_transform(Transition::spring(SpringConfig::BOUNCY))
```

## Performance Considerations

### Buffer Reuse
`PaintContext` uses pre-allocated buffers that are cleared and reused each frame, avoiding per-frame allocations.

### Reactive Efficiency
Signals only notify dependents when values actually change. The render loop reads current signal values without recreating the widget tree.

### GPU Batching
Shapes are batched into vertex/index buffers for efficient GPU submission. Text is rendered via glyphon's atlas system.

### Future: Relayout Boundaries
Planned optimization to limit layout recalculation scope. See TODO.md for details.

## Key Files

| File | Purpose |
|------|---------|
| `src/lib.rs` | App entry, main event loop |
| `src/widgets/container.rs` | Container widget implementation |
| `src/widgets/state_layer.rs` | State layer types and logic |
| `src/renderer/mod.rs` | Main renderer, GPU setup |
| `src/renderer/primitives.rs` | Shape types, vertex generation |
| `src/renderer/shader.wgsl` | GPU shaders for SDF rendering |
| `src/reactive/signal.rs` | Signal implementation |
| `src/transform.rs` | Transform matrix operations |
| `src/platform/mod.rs` | Wayland layer shell integration |

## Adding New Features

### New Widget Property
1. Add field to widget struct
2. Add builder method returning `Self`
3. If reactive, use `MaybeDyn<T>` type
4. Handle in `paint()` method

### New State Layer Override
1. Add field to `StateStyle` in `state_layer.rs`
2. Add builder method on `StateStyle`
3. Handle override resolution in container's paint logic

### New Shape Type
1. Add struct in `primitives.rs`
2. Implement vertex generation
3. Add shader support if needed
4. Add `draw_*` method to `PaintContext`
