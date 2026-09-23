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
- `GlobalSignal<T>` (internal) - a signal whose owner is the *application*, declared as a `static`. See "Owners" below.
- `Prop<T>` - what a widget *property field* holds: `Unset`, `Const(T)` or `Reactive(Signal<T>)`. A constant keeps the constant and claims no signal slot, which a `create_stored()` did anyway. Produced by `IntoSignal::into_prop()`, which reads the case off the marker.

**How it works:**
```rust
let count = create_signal(0);           // Returns RwSignal<T> (read-write)
let doubled = create_memo(move ||      // Create derived value
    count.get() * 2
);
count.set(5);                           // doubled automatically becomes 10
```

The runtime uses thread-local storage for automatic dependency tracking. When a signal is read inside a `Memo`, `Effect`, or during widget `paint()`/`layout()`, it registers itself as a dependency.

**Owners:** a signal belongs to whatever scope is current when it is created,
and that scope is ambient and time-dependent — inside a widget factory it is
that surface's, inside a click handler the root, on an effect's first run
whoever created the effect. For state with one instance per process — the
keyboard modifiers, the output list, the compositor's capabilities, the session
lock, the focus path — none of those is the answer: the owner is the
application, and drawing it by lot from whichever scope read it first is how
"signal was disposed" panics get made (#175).

`GlobalSignal` says so at the declaration:

```rust
static MODIFIERS: GlobalSignal<Modifiers> = GlobalSignal::new(Modifiers::default);
```

Created under the root owner on first use, and rebuilt if its signal is gone —
so a teardown that reads one cannot panic, and forgetting to clear the registry
costs a stale entry rather than a crash. The identity is the `static` taken by
address, so two globals of the same type are two globals.

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
- Shadows: offset, blur, spread and colour
- Transforms (translate, rotate, scale)
- Opacity over the whole subtree, multiplied into every draw in it
- State layers (hover/pressed styles)
- Ripple effects
- Event handlers (click, hover, scroll)
- Pluggable layouts via `Layout` trait

**Text** (`widgets/text.rs`)
Text rendering with:
- Reactive content (static string or `Signal<String>`)
- Font size, color, weight styling
- Text wrapping or `nowrap()` mode
- Line alignment within the text's own box (`TextAlign`)
- A line limit, `max_lines()`, marked by `overflow()` — measured and drawn by one shaping function, `shape` in `renderer/text.rs`, so every path cuts on the same line and aligns across the same box

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
5. `distribute_jobs()` sorts the pending work by surface, `drain_surface_jobs()` takes each surface's own, and `process_jobs()` applies them - Unregister → Animation (advance values) → Reconcile → Paint → Layout marking
6. Process follow-up jobs pushed by animation advances and reconciliation
7. Partial layout from `layout_roots` - Only dirty subtrees re-layout
8. Force full repaint on resize, scale change, or initialization
9. **Skip frame** if root widget doesn't need paint
10. `Tree::paint_widget()` - Build render tree via PaintContext (clean children reuse Rc-shared cached nodes)
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
- Fractional scaling via `wp_fractional_scale_v1` and `wp_viewporter`, with the
  integer `wl_surface.set_buffer_scale` path as the fallback

**Module layout.** `wayland.rs` holds the connection, the surface registry and
the layer shell; everything else is one file per concern, each owning its own
state and the protocol handlers that drive it. The `delegate_*` macros need
those handlers implemented on `WaylandState`, which is why the `impl` blocks
live beside their state rather than all in one file.

| File | Concern |
|------|---------|
| `platform/wayland.rs` | Connection, surfaces, layer shell, compositor handler |
| `platform/input.rs` | Seat: pointer, touch, keyboard, cursor shape and hiding, key repeat |
| `platform/selections.rs` | Clipboard and primary selection, async prefetch |
| `platform/outputs.rs` | Stable `OutputId` per `wl_output`, hotplug |
| `platform/popups.rs` | xdg popups: positioning, grabs, ordered teardown |
| `platform/lock.rs` | `ext-session-lock-v1` grant and lifecycle events |
| `platform/backdrop.rs` | `ext-background-effect-v1` compositor-side blur |
| `platform/scaling.rs` | `wp_fractional_scale_v1` and `wp_viewporter`: the real output scale, and the logical size a buffer stands for |

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
// What an application declares, on a Container:
container().translate((x, y))   // Move
container().rotate(deg)         // Rotate, in degrees
container().scale(s)            // Uniform scale
container().scale((sx, sy))     // Non-uniform scale

// What they compose into. `Transform` is not in `guido::prelude`; a widget
// written outside the crate reaches it through `guido::widget_prelude`:
t1.then(&t2)                    // Compose transforms
t.inverse()                     // Invert, or None where it collapsed
t.center_at(cx, cy)             // Apply around point
```

### `pivot.rs` - Pivot Points

Define rotation/scale pivot points:
```rust
Pivot::CENTER       // Default
Pivot::TOP_LEFT
Pivot::BOTTOM_RIGHT
Pivot::percent(25.0, 75.0)  // 25% from left, 75% from top
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

There are two ways in, and `ingress::sender` is private so there is no third.
`notify()` sends in the same call that queued the work, for a producer whose
payload is ready then and there. `IngressSender` is taken *before* the work
starts and binds the send to the loop that was running at that moment — a
selection read has three seconds to finish, and a result that arrives after a
restart would otherwise land in a loop whose generation counters have started
over. Both hand the wakeup to the ping if the receiver has gone.

**Main thread → `jobs::wake_loop()`.**
The frame-request ping is coalesced per loop iteration through a dedicated
`PING_SENT` flag cleared once per wakeup (`mark_loop_awake`, right after
dispatch returns). It is intentionally NOT coalesced via `WAKE_REQUESTED`:
that flag is consumed mid-iteration by `take_wake_request()`, and gating
the ping on it once lost wakeups entirely (a request landing while the flag
was set sent no ping and was then absorbed by the take — the loop blocked
with work queued until an unrelated Wayland event arrived). Additionally,
the loop refuses to block indefinitely while `WAKE_REQUESTED` is still set
at iteration start (`wake_request_pending`).

When adding a new deferred-work queue, either drain it in the loop after
the flush point AND wake through one of the two mechanisms above, or make
it a calloop source of its own.

**Main thread, but due later → `jobs::request_job_at()`.**
Work owed to a *clock* rather than to a frame — the blinking caret — is held in
`AppState::scheduled_jobs`, outside the queues, and deliberately does not make
`has_pending_jobs()` true. The wakeup is the dispatch timeout itself: the loop
asks `jobs::next_deadline()`, blocks exactly that long, and `promote_due_jobs()`
turns whatever is due into an ordinary job at the top of the next iteration.

This is why a scheduled job owes no ping: the loop is not late — it is waiting on purpose, with a bounded
timeout. The alternative is what the caret used to do: ask for an animation frame,
which means "advance me every frame", pinning the loop at 60 fps for a square wave
that changes twice a second and repainting the same pixels 113 frames out of 114.
A focused input is the resting state of a lock screen, so that ran all night.

**The contract is structural, not policed.** A deferred queue and its wakeup
are one object (`src/deferred.rs`): `DeferredQueue::push` and
`DeferredSlot::set` *are* the wakeup, the cell inside is private, and there is
no way to reach it that does not ask for the pass that empties it. Disposals
and surface commands are queues; the cursor, the clipboard and the primary
selection are slots, where two values in one frame means the second is the
answer.

That replaced a `debug_assert!` before the blocking dispatch which named every
queue and panicked if one was non-empty. It was the wrong shape twice over. It
fired on healthy states — the drains sit in the middle of the iteration and
effects, event handlers and background threads all run after them, so work
riding the next pass is what a working application looks like — and answering
that properly meant tracking, from the outside, whether a wakeup existed:
a coalescing flag for the ping and a count for messages in flight in the
calloop channel. That is a lot of apparatus to verify at runtime a rule the
type can make unbreakable.

Three producers stay outside `deferred`, each for a reason worth knowing:

- **background writes** wake through the ingress channel rather than the ping,
  and there is exactly one of them (`queue_bg_write`), so the pairing is one
  function rather than a pattern;
- **widget jobs** carry their own machinery — dedup, ownership resolution,
  per-surface lanes — and wake from inside `request_job`;
- **a parked focus request** is the opposite invariant: it waits for a widget
  that may not be laid out for many frames, so *still full* is its resting
  state, not a failure. It is applied at the end of `layout_pass`, after the
  tree has resolved and before the paint that shows it.

The session lock's request is *in* `deferred`, as a `DeferredSlot<LockRequest>`:
`lock_session` and `unlock_session` are two answers to one question, and the
pair of independent booleans they used to set let both be true at once — the
loop then started a lock and undid it three steps later in the same iteration.

Everything in `deferred` is drained unconditionally, once per iteration, in
the loop body. That is what makes "the loop will get to it on the next pass"
true regardless of which surfaces exist or whether any of them had a frame to
draw — the clipboard and the cursor used to be drained inside the per-surface
pass, which meant an application with no surface configured yet could queue a
copy that nothing would ever take.

## Ambient state

State nothing passes: a `thread_local!` cell; a `static` `GlobalSignal`, which
is a thread's signal reached through a `static`; or a process-wide `static`
with interior mutability — behind a lock or an atomic. The last is usually what
a background thread writes into, having no reach into either of the other two,
but the kind is the declaration and not the reason: `WRITE_EPOCH` below is
written on the main thread alone and is a row all the same. It reaches whoever
reads it without appearing in any signature, and it outlives the `App` that
filled it unless `App::drop` resets it. Each has a row saying why neither the
`Tree`, a pass nor an existing struct carries it, and
`tests/ambient_state_inventory.rs` fails on one without.

Most follow from one choice: a signal read inside a `move ||` closure subscribes
by itself, and an event handler is a closure with no arguments, so the reactive
system has to know who is reading and a handler needs somewhere to leave its
requests.

A row is a *cell*, not a value. `APP` and `REACTIVE` are two rows and
thirty-four values, in two structs: `AppState` is what the running application
has queued, mirrored or declared, and `ReactiveState` is the machinery that
makes a `move ||` closure reactive at all. Each is forgotten as one value, by a
`reset` that destructures it, rather than through a list somebody has to keep
true — which is what #372 was: `DEFAULT_FONT_FAMILY` was missing from one, and
nothing could say so. New state either struct could hold is a field in one of
them, reviewed beside the others and reset with them, and not a new cell with a
row of its own — being reset with the struct is the test, and it is what the
process-wide rows below say they cannot pass.
`tests/ambient_state_inventory.rs` holds both structs to the promise this
paragraph makes.

What stays out: a `static` with no interior mutability, which is a constant
written the long way, and a `static` declared *inside a function* —
`SurfaceId::next`'s counter, `shared_device`'s cached adapter,
`service_runtime`'s handle, `wakeup_test_lock`'s mutex. That second line is
syntactic, and it is not a claim that those four are harmless: the counter
mints ids that outlive every `App`, which is the property `FAMILIES` has a row
for. It is a claim about who reads them. A declaration inside a function is
reviewed by whoever changes that function and is reachable from nowhere else,
so it is not state arriving with no signature naming it, which is what this
register is of. Sweeping either in would make the register noise.

| cell | file | why nothing explicit carries it |
| --- | --- | --- |
| `REACTIVE` | `src/reactive/state.rs` | The reactive runtime: the arena signals live in, the owners that dispose them, who is reading right now, and who has to be told when a value changes. A signal read inside a reactive closure subscribes by itself, so none of it can be an argument — floem and leptos keep a runtime for the same reason |
| `APP` | `src/app_state.rs` | The application's own state — what it has queued for the loop, what it and the compositor last told each other, the fonts, and the raster images being decoded (`decoded_images`, one signal per source, the `image_decoder` thread that writes them, and the `image_events` the renderer and the widgets report about them) — in one struct, because a handler is a closure with no arguments and the loop is not reachable from one. The decode cache is here rather than a cell of its own because it is reset with the rest: its signals die with the `App`, and dropping its worker's channel ends the thread |
| `BATCHING` | `src/platform/wayland.rs` | Which surface a `batch_layer_requests` group is open on. Not a field of `WaylandState`: the closure holds `&mut WaylandState`, so a guard could not restore a field if it panics, and a scope left open would hold every later commit |
| `DEPTH` | `src/reactive/diagnostics.rs` | Debug builds: nesting of `snapshot_zone`, inside which a read with no reactive scope is not warned about |
| `REPORTED` | `src/reactive/diagnostics.rs` | Debug builds: call sites already warned about, so a hot path warns once |
| `REPORTS` | `src/reactive/diagnostics.rs` | Debug builds: the number of warnings, for the diagnostic's own tests |
| `NON_FINITE` | `src/reactive/diagnostics.rs` | Debug builds: widgets already warned about a non-finite value, with a rate limit of their own |
| `CLOCKS` | `src/reactive/diagnostics.rs` | Debug builds: call sites already warned about reading a clock outside its pass |
| `STATS` | `src/render_stats.rs` | The `render-stats` feature only: counters bumped from every pass, compiled out otherwise |
| `MODIFIERS` | `src/keyboard.rs` | `GlobalSignal`: the keyboard modifiers, read by any handler, which has no platform |
| `OUTPUTS` | `src/outputs.rs` | `GlobalSignal`: the connected outputs, read by application code that decides which surfaces to spawn |
| `SURFACE_OUTPUTS` | `src/outputs.rs` | `GlobalSignal`: which output each surface is on, read through `surface_output` by code that holds only a `SurfaceId` |
| `EFFECTS` | `src/compositor.rs` | `GlobalSignal`: what the compositor supports (blur), learned by the platform and read by widget code that has none |
| `STATE` | `src/session_lock.rs` | `GlobalSignal`: the lock lifecycle, read from widget scopes that come and go while the platform's lock bookkeeping lives in `AppState::lock` |
| `FOCUS` | `src/reactive/focus.rs` | `GlobalSignal`: the focused widget and its ancestors, so resolving a `when_focused` subscribes to it |
| `POPUP_DISMISSAL` | `src/surface.rs` | `GlobalSignal`: the notifier that makes reading `AppState::live_popups` reactive, owned by the application rather than by whichever popup opened first |
| `WRITE_QUEUE` | `src/reactive/runtime.rs` | Process-wide: the writes a `Send` `WriteSignal` made off the main thread, pushed by whichever thread holds the writer and drained by the loop. The one place it cannot be is a thread's cell, `REACTIVE` and `APP` included — the producer would push into its own and the loop would drain an empty one |
| `WRITE_EPOCH` | `src/reactive/runtime.rs` | Process-wide: the number each queued write is tagged with, so writers an `App` left behind cannot write into the next one. It has to outlive what it retires, which a field of `ReactiveState` cannot: `reset` takes every field back to its default, and an epoch that returns to zero revives the writers the increment was there to discard |
| `INGRESS_SENDER` | `src/ingress.rs` | Process-wide: the loop's calloop channel, reached from whichever thread sends on it — a service queues a write from its own; a selection reader takes its handle on the main thread and sends from the reader's, seconds later. `with_app_state` on a worker mints that thread its own empty `AppState`, so a field there would read back as "no loop" and silently take the fallback |
| `EXIT_REQUEST` | `src/jobs.rs` | Process-wide: `restart_app` is documented as callable from any thread and `quit_app` sets the same flag, which the main loop reads once a pass. A thread's cell would record the request on the thread that made it and leave the loop running |
| `WAKE_REQUESTED` | `src/jobs.rs` | Process-wide: "someone poked the loop, make a pass", raised by whoever queued the work and taken by the loop. `ingress::notify` falls back to `wake_loop` when there is no channel to send on, and that fallback runs on the background thread that queued the write |
| `PING_SENT` | `src/jobs.rs` | Process-wide: whether a ping is already armed for the current blocked period, which is what stops every producer writing one. Coalescing is only correct if all of them read the same flag, which is exactly what a thread's cell is not |
| `WAKEUP_PING` | `src/jobs.rs` | Process-wide: the calloop `Ping` the flag above arms, installed by `App::run` and pinged from whichever thread has work. `Mutex<Option<Ping>>` rather than a `OnceLock` so `App::drop` can take it back out |
| `FAMILIES` | `src/widgets/font.rs` | Process-wide: interned font family names. Not `APP`, whose `reset` empties it whole and would recycle a slot under a `FontFamily` an application still holds; not a thread's, because `FontFamily` is `Copy` and `create_signal` requires `Send`, so a name minted on a worker would be read on the main thread as a different family or none. Blink's `AtomicString` table is per-process for the same reason, and the declaration carries the argument |

## Widget Trait

All widgets implement this trait:

```rust
pub trait Widget {
    /// Advance animations for this widget and children.
    /// Returns true if any animations are still active.
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool { false }

    /// Reconcile dynamic children. Returns true if children changed.
    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool { false }

    /// Removed from a dynamic list: play the exit it declared, if any, and
    /// say whether it did. `false` is torn down in the same pass.
    fn begin_exit(&mut self, tree: &mut Tree, id: WidgetId) -> bool { false }
    /// Whether that exit is still playing; once not, it is disposed.
    fn is_exiting(&self) -> bool { false }
    /// Its key came back mid-exit: stay, and go home from where it is.
    fn cancel_exit(&mut self, tree: &mut Tree, id: WidgetId) {}
    /// The reactive scope it owns, if any: a leaving subtree pauses the
    /// effects in it, and a reclaimed one resumes them. Hidden from the docs.
    fn owned_scope(&self) -> Option<OwnerId> { None }

    /// Publish how far this widget's paint lands outside its bounds, before
    /// anything decides whether to paint it. Called from the Paint job, and by
    /// the layout entry point after every layout — both inside this widget's
    /// own Paint scope, so what it reads belongs to it.
    fn refresh_paint_bounds(&self, tree: &mut Tree, id: WidgetId) {}

    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size;
    fn paint(&self, ctx: &mut PaintContext);
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse;

    /// Check if a descendant has the given ID (for focus tracking)
    fn has_focus_descendant(&self, tree: &Tree, id: WidgetId) -> bool { false }

    /// Register this widget's pending children with the tree.
    fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {}
}
```

**Note:** Widget bounds and origins are stored in the `Tree`, not on individual widgets. Use `tree.get_bounds(id)` to retrieve a widget's bounds and `tree.set_origin(id, x, y)` to position widgets during layout.

**Note:** So is what time it is, and there are two answers because they are two
questions. A widget **advancing** something over time asks `tree.frame_instant()`
— a frame declares its instant once, around the jobs, the layout and the paint,
so everything moving in that frame is asked about the same moment. A surface's
*first* layout declares it too: that layout is where an enter animation is
seeded and the frame that follows it in the same iteration is what advances it,
so the two are handed one instant rather than two. Every surface gets that
layout from `init_pending_gpu`, whether `add_surface` declared it before the
loop started or `spawn_surface` asked for it while the loop ran — one birth
path, so there is one answer to when a birth happens. A widget
handling an **event** asks `tree.event_instant()`, which is when the compositor
saw it happen, not when the handler ran: the two differ by however long the
event sat in the queue, and that difference is what a velocity or a
double-keystroke window would otherwise measure by mistake.

The two answer different types — `FrameInstant` and `EventInstant`, from
`clock` — so the difference is the compiler's business rather than a reader's
memory. Neither carries `elapsed()`, and neither can be differenced against the
other: a moment belongs to the sequence its pass owns, and crossing between them
takes `EventInstant::as_frame_start`, which is named so the crossings can be
found. What that costs when it is invisible is #265: a flick was cancelled on
its first frame because the loop's own input latency had been differenced
against a staleness threshold.

Neither is `Instant::now()`. Reading the clock inside a widget makes one frame
several instants, and makes the middle of an animation — or the gap between two
keystrokes — something no test can ask about, only sleep towards.
`set_frame_instant` and `set_event_instant` are how a test names the moment it
is asking about, and reading either clock outside the pass that sets it reports
a diagnostic in a debug build rather than quietly handing back the wall clock.

### One way in, for every child

A child is laid out through `LayoutCtx::layout_child`, and a root through
`Tree::layout_widget` (or `measure_widget`). Nothing calls a widget's `layout`
directly. That call is what decides whether the widget runs at all — the same
constraints and nothing dirty is nothing to redo — what its reads are
attributed to, which pass it is in, and what becomes of the size it answers
with.

So a widget's `layout` measures and places; it opens no tracking scope, writes
no skip check and caches nothing. `Container` and `Text` used to write their own
check and `TextInput` and `Image` never had one, which is how they ran again
every time their parent did.

Which pass is running travels on the context: `ctx.measuring()` is true while a
natural size is being measured — the popup path, and a content-sized surface —
and it is what makes an animated value read as where it is going rather than
where it is. Only the reads that ask it are told, through
`AnimationState::displayed_in`, which is also where one says
`ctx.answer_depends_on_the_pass()` for a value in flight. A paint asks nothing:
`displayed` has one answer.

What that decides is the cache. A widget's answer is kept under the pass that
laid it out, and the pass that did not may read it only where the subtree is
settled — so a measure reads a layout's work rather than repeating it (42
widget layouts a frame rather than 242, on a bar of 121), and a subtree with an
animation in flight, or one a measure placed without letting it appear, is
never handed across.

### Widgets written outside the crate

The trait is implementable from anywhere, and a leaf needs only `layout` and
`paint`. Neither is the widget's own to scope: the framework calls both through
one entry point each — `LayoutCtx::layout_child` and
`PaintContext::paint_child` — and the scope is opened there, so a widget's
reads belong to it and a change to its own content re-lays-out or repaints it
rather than its parent and every sibling with it.

Paint was the widget's own until #390, and a widget that forgot did not merely
attribute its reads to its parent — it subscribed to nothing, drew once, and
stayed as it was. That is what `a_paint_that_opens_no_scope` in
`tests/external_widget.rs` is.

`tests/external_widget.rs` is a leaf written against the public API only;
`a_leaf_with_no_check_of_its_own_is_not_laid_out_twice_for_one_answer` is what
says the framework's check covers it, and
`a_layout_is_attributed_to_the_widget_that_ran_it` in `src/lib.rs` — with
`the_innermost_scope_owns_the_read` in `reactive/invalidation.rs` beneath it —
pins the ownership rule.

## Event Flow

```
Wayland → Platform → App → Widget Tree
                              │
                              ├─ MouseMove/Enter/Leave
                              ├─ MouseDown/MouseUp
                              └─ Scroll/ScrollEnd
```

`ScrollEnd` is the end of a gesture, from `wl_pointer.axis_stop`. It carries no
delta, and it is what decides when momentum scrolling may begin — guaranteed
only for `ScrollSource::Finger`.

`MouseMove` and `MouseDown` carry a `PointerKind`. Touch is folded into the
pointer pipeline (`src/platform/input.rs`), which is what lets a widget alias
the two in one arm; the kind is the one thing the fold cannot alias, and a
scroller is what reads it — a finger dragging content scrolls it, a mouse does
not.

Events propagate down the widget tree. Each widget can:
- Handle the event (`EventResponse::Handled`)
- Ignore and let parent continue (`EventResponse::Ignored`)

A `Container` that scrolls does two things around that dispatch rather than
one. Going down, a finger's press arms the watch that a drag needs — before a
child takes the press, because the press is still the child's until the slop is
crossed. Coming back up, the move that crosses it claims the gesture, which is
where a list inside a page beats the page, exactly as the innermost scroller
already takes the wheel. That claim is asked whether or not a child answered
`Handled`, because a child taking a move is not a *scroller* taking it; what
says a scroller did is a flag on the `Tree`, beside the one a widget uses to
say a press landed on the focus it draws.

Once the gesture has been claimed it is answered on the way down again, with
the children not asked at all: nothing below can take a drag back, and a
scroller that kept asking would walk the subtree it is scrolling once a frame
for the length of the gesture.

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

The timing rides with the value, so a property cannot be animated without
being set and the two cannot disagree about which property they mean:

```rust
// A bare number is milliseconds
.background(theme.surface.transition(200.0))

// Duration with easing
.background(theme.surface.transition(Transition::new(200.0, TimingFunction::EaseOut)))

// Spring physics
.scale(open.transition(Transition::spring(SpringConfig::BOUNCY)))

// A sequence played on a trigger, resting on the declared value between plays
.rotate(0.0.timeline(shake.played_by(rejections)))
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
- **Partial propagation**: a node that did not paint all of itself (`partial`) poisons its
  ancestors in the cache walk — incomplete paints are never cached, so reuse can never
  resurrect a subtree with missing children. Refusing to cache is only half of it: a
  partial paint also *drops* the entry the last complete one left, because the widget
  painted this frame and painted something else. Keeping it as a picture of "how this
  looks with nothing culled" is how a scrolled list came back at rest.
- **Skip frame**: If the root widget doesn't need paint after job processing and layout,
  the entire paint→flatten→render cycle is skipped.
- **Damage regions**: `mark_needs_paint()` accumulates surface-relative bounds into a
  per-surface `DamageRegion` (None/Partial/Full), keyed by the surface's root widget so
  multi-surface apps can't consume each other's damage. Damage is set as pending state
  BEFORE presenting, so it rides the commit that `present()` performs internally.
- **Vacated rects**: that rect always describes the widget as it is *now*, so anything
  that makes a widget cover *less* has to name what it is leaving before it changes —
  `set_origin` and `cache_layout` damage the old rect and then the new one, and
  `set_own_paint_reach` damages the ring a shrinking reach gives up (a transform coming
  back to rest, a shadow falling to nothing). Without it the buffer is redrawn correctly
  and the compositor is never told to re-composite the pixels the widget has left, so the
  old position survives on screen as a fringe.
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
- What those frames asked of the allocator, if anything installed the counter

The feature has zero overhead when disabled (code is completely compiled out).

### What a Frame Costs the Heap

`src/heap.rs` holds `CountingAllocator`, a `GlobalAlloc` that forwards to the
system allocator and counts on the way through. **The library never installs
it**: a `#[global_allocator]` may only be set by the binary that links the
program, so the two benchmarks and the tests that read the figures install it
themselves, the line `dhat` and `stats_alloc` draw for the same reason.

The counters are the whole process's, because the heap is — wgpu's threads and
the graphics driver's allocate inside a frame too. `render_stats` samples them
at `reset_stats` and at every `end_frame` and reports the delta, which is what
makes a process-wide number a frame's; `Region` does the same subtraction over
a wider stretch, and brings the live-byte high-water mark down to the level it
starts at so two runs in one process each report their own peak.

Without `render-stats` the three hooks the allocator calls are empty, so the
counters never move and every reader answers zero. The readers are one
implementation either way: a second set returning literal zeroes, compiled only
when the feature is off, is a copy that no build under test contains — five of
its mutants survived on the branch that introduced this, which is how the
duplicate was found.

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
| `src/renderer/clip.rs` | The frame's clip tree, which draw commands name rather than copy |
| `src/renderer/shader.wgsl` | GPU shaders for instanced SDF rendering |
| `src/reactive/signal.rs` | Signal implementation |
| `src/reactive/prop.rs` | `Prop<T>`: what a property field holds, so a constant costs the constant |
| `src/reactive/global.rs` | `GlobalSignal`: state whose owner is the application |
| `src/image_decode.rs` | Raster images decoded off the frame: one signal per source, a worker that writes them, and the handle that frees the pixels with the last image |
| `src/heap.rs` | `CountingAllocator`: what a frame asks of the allocator, for the binary that installs it |
| `src/transform.rs` | Transform matrix operations |
| `src/shape.rs` | A rounded rect and the transform that places it — one type for clips, compositor regions and the backdrop mask |
| `src/region.rs` | A placed shape tessellated into the rectangles a `wl_region` is made of |
| `src/platform/wayland.rs` | Wayland connection, surfaces and layer shell |
| `src/platform/input.rs` | Seat input: pointer, touch, keyboard |

## Adding New Features

### New Widget Property
1. Add field to widget struct
2. Add builder method returning `Self`
3. Give the field type `Prop<T>` and have the builder method take `impl IntoSignal<T, M>` and store `value.into_prop()`. Read it with `get_or` / `get_or_untracked`, or `get_finite_or` where a bad number has to be coerced. Do **not** store `Option<Signal<T>>`: `into_signal()` allocates an arena slot for a constant, which is the ~112 bytes per property #450 removed.
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
