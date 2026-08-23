# Guido Architecture

This document provides an overview of Guido's architecture for developers working on or with the codebase.

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                          Application                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Widgets   │  │  Reactive   │  │       Platform          │  │
│  │  Container  │  │   Signals   │  │   Wayland Layer Shell   │  │
│  │    Text     │  │    Memo     │  │   Event Loop (calloop)  │  │
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

Single-threaded reactive primitives inspired by SolidJS and Floem.

**Key Types:**
- `RwSignal<T>` (8 bytes) - Read-write reactive values returned by `create_signal()`. Supports `.get()`, `.set()`, `.update()`, `.writer()`. `Copy`, `!Send`.
- `Signal<T>` (12 bytes) - Read-only reactive wrapper. Created via `create_stored()` (static), `create_derived()` (closure-backed), or by coercing an `RwSignal<T>`. `Copy`, read-only (`.get()` only).
- `Memo<T>` - Eager derived values that recompute when dependencies change, only notify on actual changes (`PartialEq`)
- `Effect` - Side effects that re-run when tracked signals change
- `WriteSignal<T>` - `Send` handle for background thread updates, obtained via `RwSignal::writer()`

**How it works:**
```rust
let count = create_signal(0);           // Returns RwSignal<T> (read-write)
let doubled = create_memo(move ||      // Create derived value
    count.get() * 2
);
count.set(5);                           // doubled automatically becomes 10
```

The runtime uses thread-local storage for automatic dependency tracking. When a signal is read inside a `Memo`, `Effect`, or during widget `paint()`/`layout()`, it registers itself as a dependency.

**Identity safety:** signal, effect, owner, and widget ids are all generational
(`index + generation`). Slots are recycled, but a stale `Copy` handle held after
disposal can never alias the slot's next occupant — reads of disposed signals
fail loudly instead of silently reading unrelated state.

**Effect execution:** effect callbacks run with no runtime borrow held, so
writing signals inside an effect works and chains (effect → effect, memo →
memo). Scope state (batch depth, owners, tracking contexts) is restored via
Drop guards, so a caught panic cannot wedge the reactive system.

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

**Type Erasure** (`widgets/widget.rs`)
- `AnyWidget` type alias (`Box<dyn Widget>`) for type-erased widgets
- `Widget::into_any()` method for boxing widgets in conditional branches

**Layout System** (`layout/`)
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

Built-in implementations:
- `Flex` - Flexbox-style row/column layout with spacing and alignment
- `ZStack` - Children share an origin and stack along the Z axis. Children
  that don't `fill()` an axis lead it (the stack takes their size); children
  that do fill it follow, laid out against the size the others established

### `renderer/` - GPU Rendering

Hardware-accelerated rendering using wgpu.

**Components:**
- `Renderer` - Main renderer managing GPU resources and render passes
- `PaintContext` - Build render tree nodes during widget painting
- `RenderNode` - Hierarchical render tree with local coordinates (one root per surface, children Rc-shared with the paint cache)
- Custom WGSL shaders for SDF-based instanced rendering

**Rendering Pipeline (per frame):**

*Main loop (once per iteration):*
1. `flush_bg_writes()` - Drain queued background-thread signal writes
2. `take_wake_request()` - Take the pending wake request

*Per-surface rendering:*
3. Dispatch events to widgets (queued `MouseMove`s are coalesced to the latest position)
4. **Frame-pacing gate**: if a `wl_surface.frame` callback is still in flight, return
   before draining jobs — the compositor hasn't shown the previous frame yet.
   The gate and the job queues have the same per-surface granularity
   (surface-owned scheduling, see Jobs System): a gated surface's queued
   jobs — including animation continuations — sit untouched in its own
   queue until the callback fires and wakes the loop. Init and resizes
   bypass the gate.
5. `drain_pending_jobs()` + `process_jobs()` - Unregister → Animation (advance values) → Reconcile → Paint → Layout marking
6. Process follow-up jobs pushed by animation advances and reconciliation
7. Partial layout from `layout_roots` - Only dirty subtrees re-layout
8. Force full repaint on resize, scale change, or initialization
9. **Skip frame** if root widget doesn't need paint
10. `widget.paint(tree, ctx)` - Build render tree via PaintContext (clean children reuse Rc-shared cached nodes)
11. `flatten_root_into()` - Flatten render tree to draw commands (incremental: clean subtrees reuse cached commands)
12. Re-arm the `wl_surface.frame` callback and report per-surface damage via
    `wl_surface.damage_buffer()` — both BEFORE presenting, so they ride the
    commit that `present()` performs internally
13. GPU rendering with instanced SDF shapes and HiDPI scaling; `present()` commits.
    If presentation fails (lost/outdated swapchain), dirty state is kept and a
    retry frame is requested — no stale content
14. `cache_paint_results()` - Rc-share paint output per widget into the cache,
    clear `needs_paint`/`repainted` flags (skips already-clean subtrees)

Rendering is paced by the compositor's frame callbacks: an animating surface
renders once per callback, and an idle surface renders nothing at all. There is
no post-loop animation phase — animations advance during job processing, and
their continuation jobs are throttled by the pacing gate.

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
- Exclusive zones and margins for panels
- Reactive output (monitor) enumeration via `outputs()`, per-output surface
  pinning via `SurfaceConfig::output`, per-surface output tracking via
  `surface_output()` (see `src/outputs.rs`)
- Event loop via calloop
- Dynamic surface property modification via `SurfaceHandle`

**Module layout.** `wayland.rs` holds the connection, the surface registry and
the layer shell; everything else is one file per concern, each owning its own
state and the protocol handlers that drive it. The `delegate_*` macros need
those handlers implemented on `WaylandState`, which is why the `impl` blocks
live beside their state rather than all in one file.

| File | Concern |
|------|---------|
| `platform/wayland.rs` | Connection, surfaces, layer shell, compositor handler |
| `platform/input.rs` | Seat: pointer, touch, keyboard, cursor shape, key repeat |
| `platform/selections.rs` | Clipboard and primary selection, async prefetch |
| `platform/outputs.rs` | Stable `OutputId` per `wl_output`, hotplug |
| `platform/popups.rs` | xdg popups: positioning, grabs, ordered teardown |
| `platform/lock.rs` | `ext-session-lock-v1` grant and lifecycle events |
| `platform/backdrop.rs` | `ext-background-effect-v1` compositor-side blur |

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
handle.set_margin([8, 12]);
```

### `transform.rs` - 2D Transforms

2D affine transforms stored as 6 floats `[a, b, tx, c, d, ty]` (24 bytes) —
exactly the layout the GPU shader consumes.

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
- `Paint` - Widget needs repaint (partial paint with caching)
- `Reconcile` - Dynamic children need reconciliation (implies layout)
- `Unregister` - Widget needs cleanup (deferred Drop)
- `Animation` - Widget has active animations

**How It Works — surface-owned scheduling:**
- Jobs are keyed by widget, but their *scheduling domain* is the surface:
  the frame-pacing gate is per-surface, so the queues are too.
- `request_job()` pushes into a global **inbox** (push sites have no `Tree`
  access) and wakes the event loop. For animations,
  `JobRequest::Animation(RequiredJob)` adds both the Animation job and any
  required follow-up job (Paint or Layout).
- `distribute_jobs(tree, active_roots)` is the **single place where
  ownership is resolved**: it sorts the inbox into per-surface queues keyed
  by surface root (topmost ancestor). Jobs with no live owning surface go
  to the **orphan lane**, processed once per loop iteration (deferred
  Unregister cleanup); queues of destroyed surfaces are retired there too.
- Each surface's render pass drains **only its own queue** and processes by
  type in order. A frame-gated surface's animation continuations sit in its
  own queue until its callback fires — no other surface can advance them
  (this exact bug once spun the loop at ~260k iterations/s).
- `layout_roots` are per-surface as well, so one surface's render pass
  never lays out another surface's subtrees.

## Event Loop Wakeup Contract

The main loop blocks in `calloop::EventLoop::dispatch` when idle. Anything
that queues work for the loop must guarantee a wakeup that survives until
its consumer runs. Two mechanisms exist, and which one to use depends on
the producer's thread:

**Background threads → calloop ingress channel (`src/ingress.rs`).**
Cross-thread producers (services via `WriteSignal`, reader threads) send an
`IngressMessage` through a calloop channel registered as an event source.
calloop guarantees a send wakes the next dispatch — the message's existence
*is* the wakeup, so a lost-wakeup is impossible by construction. Messages
either carry their payload or act as doorbells for data queued elsewhere
(e.g. `BgWritesQueued` for the reactive write queue, drained at the loop's
flush point). Never call `jobs::wake_loop()` directly from a background
thread as the *only* wakeup for queued work.

**Main thread → `jobs::wake_loop()`.**
The frame-request ping is coalesced per loop iteration through a dedicated
`PING_SENT` flag cleared once per wakeup (`mark_loop_awake`, right after
dispatch returns). It is intentionally NOT coalesced via `WAKE_REQUESTED`:
that flag is consumed mid-iteration by `take_frame_request()`, and gating
the ping on it once lost wakeups entirely (a request landing while the flag
was set sent no ping and was then absorbed by the take — the loop blocked
with work queued until an unrelated Wayland event arrived). Additionally,
the loop refuses to block indefinitely while `WAKE_REQUESTED` is still set
at iteration start (`frame_request_pending`).

When adding a new deferred-work queue, either drain it in the loop after
the flush point AND wake through one of the two mechanisms above, or make
it a calloop source of its own.

**Main thread, but due later → `jobs::request_job_at()`.**
Work owed to a *clock* rather than to a frame — the blinking caret — is held in
`SCHEDULED_JOBS`, outside the queues, and deliberately does not make
`has_pending_jobs()` true. The wakeup is the dispatch timeout itself: the loop
asks `jobs::next_deadline()`, blocks exactly that long, and `promote_due_jobs()`
turns whatever is due into an ordinary job at the top of the next iteration.

This is why a scheduled job is not named in `queued_but_unwoken()`: nobody owes it
a ping, because the loop is not late — it is waiting on purpose, with a bounded
timeout. The alternative is what the caret used to do: ask for an animation frame,
which means "advance me every frame", pinning the loop at 60 fps for a square wave
that changes twice a second and repainting the same pixels 113 frames out of 114.
A focused input is the resting state of a lock screen, so that ran all night.

**The contract is checked, not just written down.** Debug builds assert it at
the one moment where breaking it is fatal: `queued_but_unwoken()` in
`src/lib.rs` names every queue the loop drains, and nothing may be
outstanding when the loop is about to block indefinitely. The reasoning is
that each queue is drained unconditionally once per iteration, so anything
still queued was produced *after* its own drain — and the only thing that
brings the loop back is a frame request, which would have kept it from
blocking. A non-empty queue there means nobody asked to be woken.

So a new queue needs one line in `queued_but_unwoken()` alongside its drain.
Forget the wakeup and the next idle moment panics with the queue's name,
instead of the app going quietly deaf until an unrelated compositor event
happens along. Two of the bugs documented above were exactly that.

## Widget Trait

All widgets implement this trait:

```rust
pub trait Widget {
    /// Advance animations for this widget and children.
    /// Returns true if any animations are still active.
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool { false }

    /// Reconcile dynamic children. Returns true if children changed.
    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool { false }

    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size;
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext);
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse;

    /// Check if a descendant has the given ID (for focus tracking)
    fn has_focus_descendant(&self, tree: &Tree, id: WidgetId) -> bool { false }

    /// Register this widget's pending children with the tree.
    fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {}
}
```

**Note:** Widget bounds and origins are stored in the `Tree`, not on individual widgets. Use `tree.get_bounds(id)` to retrieve a widget's bounds and `tree.set_origin(id, x, y)` to position widgets during layout.

### Widgets written outside the crate

The trait is implementable from anywhere, and a leaf needs only `layout` and
`paint`. What it also needs is `with_signal_tracking(id, JobType::Layout, ..)`
around whatever it measures from, and the same with `JobType::Paint` around
whatever it draws from — both exported from the prelude for this reason.

Scopes nest and the innermost wins, so a widget that opens its own claims its
reads back from its parent. One that does not is not unreactive: its reads
register against the nearest ancestor that opened a scope, usually the enclosing
container, so a change to its own content marks *that* for layout and every
sibling is re-laid-out with it. Reactive, but imprecise, and silently so.

`tests/external_widget.rs` is a leaf written against the public API only, and
`the_innermost_scope_owns_the_read` in `reactive/invalidation.rs` pins the
ownership rule.

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
    .when_hovered(|s| s.lighter(0.1))     // Override on hover
    .when_pressed(|s| s.ripple())        // Override on press
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

### Partial Paint and Damage Tracking

The paint system tracks which widgets need repainting:

- **`needs_paint` flag**: Each widget in the Tree has a `needs_paint` flag that propagates
  upward to ancestors (like `needs_layout`). Only widgets marked dirty are repainted.
- **Rc-shared paint cache**: After painting, each widget's `RenderNode` is cached as an
  `Rc` to the same node in the frame's render tree — a refcount bump, not a clone. On
  subsequent frames, a clean child whose position didn't change is reused via `Rc::clone`
  (zero copies); if it moved, only the node header is cloned (children and commands stay
  shared) with the position recomposed from the decomposed parent/user transforms.
- **Partial propagation**: a node whose children were culled (`partial`) poisons its
  ancestors in the cache walk — incomplete paints are never cached, so reuse can never
  resurrect a subtree with missing children.
- **Skip frame**: If the root widget doesn't need paint after job processing and layout,
  the entire paint→flatten→render cycle is skipped.
- **Damage regions**: `mark_needs_paint()` accumulates surface-relative bounds into a
  per-surface `DamageRegion` (None/Partial/Full), keyed by the surface's root widget so
  multi-surface apps can't consume each other's damage. Damage is set as pending state
  BEFORE presenting, so it rides the commit that `present()` performs internally.
- **Incremental flatten**: `RenderNode` caches its flattened commands. Clean subtrees
  (with `repainted == false`) reuse cached commands with a translation offset, skipping
  the full recursive flatten.

### Focus Paint Invalidation

When focus changes between widgets, the focus system (`request_focus`, `release_focus`,
`clear_focus`) automatically queues a Paint job for the previously focused widget. This
ensures parent containers with `when_focused` styling repaint to drop their focused
border/background.

### Text Measurement Caching
Text measurement results are cached to avoid redundant computation when text content
hasn't changed.

### Render Stats (Debug Feature)
Enable the `render-stats` feature to get real-time statistics about rendering performance:
```bash
cargo run --example your_example --features render-stats
```

This prints per-second statistics showing:
- Frame counts (painted vs skipped)
- Layout calls, skip rate, and execution reasons
- Paint child cache hits/misses
- Flatten cache hits/misses
- Damage region distribution (none, partial, full)

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
| `src/renderer/tree.rs` | RenderNode structure and paint-cache sharing |
| `src/renderer/flatten.rs` | Tree flattening with transform inheritance |
| `src/renderer/shader.wgsl` | GPU shaders for instanced SDF rendering |
| `src/reactive/signal.rs` | Signal implementation |
| `src/transform.rs` | Transform matrix operations |
| `src/platform/wayland.rs` | Wayland connection, surfaces and layer shell |
| `src/platform/input.rs` | Seat input: pointer, touch, keyboard |

## Adding New Features

### New Widget Property
1. Add field to widget struct
2. Add builder method returning `Self`
3. If reactive, use `Signal<T>` type (via `IntoSignal<T>` in builder methods)
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
