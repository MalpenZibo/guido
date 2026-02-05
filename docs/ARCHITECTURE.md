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
pub trait Layout: Send + Sync {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size;
}
```

Built-in implementation:
- `Flex` - Flexbox-style row/column layout with spacing and alignment

### `renderer/` - GPU Rendering

Hardware-accelerated rendering using wgpu.

**Components:**
- `Renderer` - Main renderer managing GPU resources and render passes
- `PaintContext` - Build render tree nodes during widget painting
- `RenderTree` / `RenderNode` - Hierarchical render tree with local coordinates
- Custom WGSL shaders for SDF-based instanced rendering

**Rendering Pipeline:**
1. `drain_pending_jobs()` - Collect pending reactive jobs
2. `handle_unregister_jobs()` - Cleanup dropped widgets
3. `handle_reconcile_jobs()` - Reconcile dynamic children
4. `handle_layout_jobs()` - Mark widgets needing layout
5. Partial layout from `layout_roots` - Only dirty subtrees re-layout
6. `widget.paint(tree, ctx)` - Build render tree via PaintContext
7. `flatten_tree()` - Flatten render tree to draw commands with inherited transforms
8. `handle_animation_jobs()` - Advance animations
9. Instanced GPU rendering with HiDPI scaling

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
- Keyboard interactivity modes (None, OnDemand, Exclusive)
- Exclusive zones for panels
- Event loop via calloop
- Dynamic surface property modification via `SurfaceHandle`

### `surface.rs` - Surface Management

Handles surface creation, configuration, and runtime modification.

**Key Types:**
- `SurfaceConfig` - Configuration for new surfaces (size, anchor, layer, keyboard mode)
- `SurfaceId` - Unique identifier for each surface
- `SurfaceHandle` - Control handle for modifying surface properties

**Dynamic Properties:**
Surfaces can be modified at runtime through `SurfaceHandle`:
```rust
let handle = surface_handle(surface_id);
handle.set_layer(Layer::Overlay);
handle.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
handle.set_anchor(Anchor::TOP | Anchor::RIGHT);
handle.set_size(400, 300);
```

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

## Tree System

Guido uses an arena-based widget storage system where all widgets are stored centrally in a `Tree`.

**Key Types:**
- `Tree` - Central widget storage with layout metadata
- `WidgetId` - Unique identifier for each widget
- `Node` - Hierarchy info (parent/children) and dirty tracking

**How It Works:**
- Containers hold child `WidgetId`s rather than owned widgets
- The `Tree` provides widget access via `with_widget()` and `with_widget_mut()`
- Dirty flags bubble up to relayout boundaries for efficient partial layout
- `layout_roots` tracks which boundaries need layout

## Jobs System

The jobs system connects reactive signals to widget invalidation.

**Job Types:**
- `Layout` - Widget needs layout recalculation
- `Paint` - Widget needs repaint (future: partial repaint)
- `Reconcile` - Dynamic children need reconciliation (implies layout)
- `Unregister` - Widget needs cleanup (deferred Drop)
- `Animation` - Widget has active animations

**How It Works:**
- Signals push jobs to a global queue when values change
- `request_job()` is thread-safe and wakes the event loop
- For animations, `request_job()` with `JobRequest::Animation(RequiredJob)` adds both the Animation job and any required follow-up job (Paint or Layout)
- Main loop drains jobs and processes by type in order
- Animation jobs run after paint to advance state for next frame

## Widget Trait

All widgets implement this trait:

```rust
pub trait Widget: Send + Sync {
    /// Advance animations for this widget and children.
    /// Returns true if any animations are still active.
    fn advance_animations(&mut self, tree: &Tree) -> bool { false }

    /// Reconcile dynamic children. Returns true if children changed.
    fn reconcile_children(&mut self, tree: &mut Tree) -> bool { false }

    fn layout(&mut self, tree: &mut Tree, constraints: Constraints) -> Size;
    fn paint(&self, tree: &Tree, ctx: &mut PaintContext);
    fn event(&mut self, tree: &Tree, event: &Event) -> EventResponse;
    fn set_origin(&mut self, x: f32, y: f32);
    fn bounds(&self) -> Rect;

    /// Get the widget's unique identifier
    fn id(&self) -> WidgetId;

    /// Check if a descendant has the given ID (for focus tracking)
    fn has_focus_descendant(&self, tree: &Tree, id: WidgetId) -> bool { false }

    /// Check if this widget is a relayout boundary.
    /// Widgets with fixed size are boundaries - layout changes
    /// inside don't affect their own size or parent layout.
    fn is_relayout_boundary(&self) -> bool { false }

    /// Register this widget's pending children with the tree.
    fn register_children(&mut self, tree: &mut Tree) {}
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

### Relayout Boundaries
Widgets with fixed width and height (e.g., `width(100.0).height(100.0)`) are automatically
marked as relayout boundaries. Layout changes inside a boundary don't propagate to the
parent, reducing layout recalculation scope.

### Paint-Only Scrolling
Scroll is implemented as a paint-only transform operation. When content scrolls, the layout
doesn't run again - instead, a scroll transform is applied during the paint phase. This
significantly reduces CPU overhead for scrolling.

### Layout Caching
The layout system caches results and uses per-widget layout subscribers to track signal dependencies.
During layout, any signal reads are recorded as dependencies. When those signals change, only the
affected widgets are marked dirty for re-layout - not the entire tree.

Layout only recalculates when:
- Constraints change
- Animations are active
- A tracked signal dependency changes (widget is marked dirty)

### Text Measurement Caching
Text measurement results are cached to avoid redundant computation when text content
hasn't changed.

### Layout Stats (Debug Feature)
Enable the `layout-stats` feature to get real-time statistics about layout performance:
```bash
cargo run --example your_example --features layout-stats
```

This prints per-second statistics showing:
- Total layout calls and skip rate
- Breakdown of reasons layouts were executed (constraints, animations, reactive changes)

The feature has zero overhead when disabled (code is completely compiled out).

## Key Files

| File | Purpose |
|------|---------|
| `src/lib.rs` | App entry, main event loop |
| `src/tree.rs` | Widget tree storage and layout metadata |
| `src/jobs.rs` | Job-based reactive invalidation system |
| `src/surface.rs` | Surface config, handles, dynamic properties |
| `src/widgets/container.rs` | Container widget implementation |
| `src/widgets/children.rs` | Dynamic children with keyed reconciliation |
| `src/widgets/state_layer.rs` | State layer types and logic |
| `src/renderer/mod.rs` | Module exports |
| `src/renderer/render.rs` | Main renderer, GPU setup |
| `src/renderer/paint_context.rs` | PaintContext API for building render tree |
| `src/renderer/tree.rs` | RenderNode, RenderTree structures |
| `src/renderer/flatten.rs` | Tree flattening with transform inheritance |
| `src/renderer/shader.wgsl` | GPU shaders for instanced SDF rendering |
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
1. Add variant to `DrawCommand` in `commands.rs`
2. Implement rendering in `render.rs`
3. Add shader support if needed
4. Add `draw_*` method to `PaintContext`
