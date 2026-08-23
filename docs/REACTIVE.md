# Reactive System

Guido uses a fine-grained reactive system inspired by SolidJS and Floem. This enables efficient updates where only the affected parts of the UI re-render.

## Core Concepts

### Signals

Signals are reactive values that notify dependents when they change:

```rust
use guido::prelude::*;

let count = create_signal(0);  // Returns RwSignal<T> (read-write)

// Read the current value
let value = count.get();

// Set a new value
count.set(5);

// Update based on current value
count.update(|c| *c += 1);
```

**Key types:**
- `RwSignal<T>` (8 bytes) — read-write signal returned by `create_signal()`. Supports `.get()`, `.set()`, `.update()`, `.writer()`
- `Signal<T>` (12 bytes) — read-only signal. Created via `create_stored()` (static), `create_derived()` (closure-backed), or by coercing an `RwSignal<T>`

**Key properties:**
- Both `Signal<T>` and `RwSignal<T>` are `Copy` — no cloning needed
- Main-thread only reads/writes — use `.writer()` on `RwSignal<T>` to get a `WriteSignal<T>` for background thread updates
- Automatic dependency tracking

### Memos

Eager computed values that recompute immediately when dependencies change. Memos only notify downstream subscribers when the result actually differs (`PartialEq`), preventing unnecessary updates:

```rust
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);

count.set(5);
println!("{}", doubled.get()); // Prints: 10
```

Memos are `Copy` like signals and can be used directly as widget properties:

```rust
let count = create_signal(0);
let label = create_memo(move || format!("Count: {}", count.get()));
text(label)  // Only repaints when the formatted string changes
```

A memo is a **tracking barrier**: what it reads is its own business. Creating
one inside something that is itself tracked — a dynamic-children closure
building a component, a paint or layout pass, another effect — registers no
dependency on the surrounding scope. That is what makes the pattern work:

```rust
// Inside a dynamic-children closure. `levels` is written 60 times a second.
container().child(move || {
    let active = create_memo(move || levels.with(|l| !l.is_empty()));
    // The closure depends on `active` (a bool that rarely flips), NOT on
    // `levels` — the subtree is rebuilt when the bool changes, not per frame.
    active.get().then(|| expensive_module())
})
```

Only the value read *through* the memo (`active.get()` above) creates a
dependency, and it only fires when the memoized value actually changes.

### Snapshot Reads (and the debug warning)

A tracked read (`get`/`with`) registers the *currently active* reactive scope
as a subscriber. With no scope active there is nobody to register, so the value
is a snapshot — frozen into whatever it was used for:

```rust
text(format!("{}", count.get()))          // snapshot: never updates again
text(move || format!("{}", count.get()))  // reactive: the closure re-runs
```

The two lines differ only in *where* the read happens, which Rust cannot check
at compile time. So debug builds warn at the read site instead, naming the
exact line:

```
guido: src/main.rs:42:31: signal read with no reactive scope — this value is a
snapshot and will not update. Pass a closure instead …
```

One warning per call site, nothing in release builds. Two ways to say a
snapshot is intended:

- `get_untracked()` / `with_untracked()` for a single read
- `reactive::diagnostics::snapshot_zone(|| …)` for a whole callback region

Reads that are already correct stay silent: inside a widget's layout/paint
scope, inside effects and memos, inside event handlers and animation
completions (guido marks those regions itself), and reads of `create_stored`
values, which cannot change in the first place.

### Per-Field Signals (`SignalFields`)

When a `Signal<AppState>` changes, **all** subscribers re-render — even if only one field changed. The `#[derive(SignalFields)]` macro solves this by generating per-field signal decomposition with zero-clone updates.

```rust
use guido::prelude::*;

#[derive(Clone, PartialEq, SignalFields)]
pub struct AppState {
    pub count: i32,
    pub name: String,
    pub items: Vec<Item>,
}
```

This generates:
- `AppStateSignals` — struct with `Signal<T>` per field (`Copy`)
- `AppStateWriters` — struct with `WriteSignal<T>` per field (`Copy + Send`)

**Usage:**

```rust
// Create per-field signals from initial values
let state = AppStateSignals::new(AppState {
    count: 0,
    name: "foo".into(),
    items: vec![],
});

// Widgets subscribe to individual signals — only re-render when their field changes
text(move || format!("Count: {}", state.count.get()))  // ignores name/items changes
text(move || state.name.get())                          // ignores count/items changes
```

**Background task integration:**

```rust
// Get writer handles (Send + Copy) for background services
let writers = state.writers();

create_task(move |ctx| async move {
    while ctx.is_running() {
        let new_state = fetch_state().await;
        writers.set(new_state);  // Decomposes struct, sets each field individually
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});
```

**Zero-clone decomposition:** `writers.set(state)` destructures the struct and moves each field into its signal. Each field's `set()` uses `PartialEq` to skip unchanged values — only the actually-changed widgets re-render. The call is batched so that shared effects (depending on multiple fields) run only once.

**Generic structs** are supported — the generated types carry the same generic parameters:

```rust
#[derive(Clone, PartialEq, SignalFields)]
pub struct Pair<A: Clone + PartialEq + Send + 'static, B: Clone + PartialEq + Send + 'static> {
    pub first: A,
    pub second: B,
}

let pair = PairSignals::new(Pair { first: 1i32, second: "hello".to_string() });
```

**When to use what:**
- `Signal<T>` — single reactive value
- `create_memo` — derived value from other signals
- `#[derive(SignalFields)]` — struct with independently-changing fields (e.g., backend state with many independent pieces)

### Effects

Side effects that re-run when tracked signals change:

```rust
let name = create_signal("World".to_string());

create_effect(move || {
    println!("Hello, {}!", name.get());
});

name.set("Guido".to_string()); // Effect re-runs, prints: Hello, Guido!
```

## Using Signals in Widgets

### Static vs Reactive Properties

Most widget properties accept either static values or reactive sources:

```rust
// Static background
container().background(Color::RED)

// Reactive background (signal)
let bg = create_signal(Color::RED);
container().background(bg)

// Reactive background (closure)
container().background(move || {
    if is_active.get() { Color::GREEN } else { Color::RED }
})
```

### Reactive Text

```rust
let count = create_signal(0);

text(move || format!("Count: {}", count.get()))
```

### Reactive Children

A closure child re-runs when a signal it reads changes, and its result
**replaces** the previous widget (returning `None` removes it). Tracking is
per segment: only the closure whose signals changed re-runs.

```rust
let items = create_signal(vec![
    (1u64, "Item A".to_string()),
    (2, "Item B".to_string()),
]);

// Single reactive child — rebuilt when `items` changes
container().child(move || text(format!("{} items", items.with(|l| l.len()))))

// Reactive list, unkeyed — all rows replaced when `items` changes
container().children(move || {
    items.with(|l| l.iter().map(|(_, label)| text(label.clone())).collect::<Vec<_>>())
})

// Keyed list: identity via key, per-item content diffing via PartialEq —
// rows keep their state across reorders, only changed rows rebuild
container().children(keyed(
    move || items.get(),
    |(id, _)| *id,
    |(_, label)| text(label),
))
```

To narrow when a closure re-runs, read a `Memo` instead of the raw signal —
memos notify only when their computed value actually changes. See the book's
Dynamic Children chapter for the full semantics.

## IntoSignal Pattern

The `IntoSignal<T>` trait allows properties to accept any of:

- **Static values**: `container().background(Color::RED)` → calls `create_stored()`
- **Closures**: `container().background(move || if dark { Color::BLACK } else { Color::WHITE })` → calls `create_derived()`
- **Signals**: `container().background(my_signal)` → passed through directly (accepts both `Signal<T>` and `RwSignal<T>`)

For numeric properties (`f32`, `Length`, `Padding`), integer and float literals both coerce to the target type: `container().corner_radius(12)` and `.corner_radius(12.5)` are equivalent to `12.0_f32`. Float coercion is lossy (`f64 as f32`), matching the precision of the destination.

All produce a `Signal<T>` (which is `Copy`). `create_signal` returns `RwSignal<T>` (read-write, Clone+PartialEq+Send) which has `.set()`, `.update()`, and `.writer()`. `Signal<T>` is read-only — use it when you only need to read values.

## Background Task Updates

`RwSignal<T>` is `!Send` — it can only be read and written on the main thread. To update a signal from a background task, use `.writer()` on `RwSignal<T>` to obtain a `WriteSignal<T>`, which is `Send`. Writes from `WriteSignal` are queued and applied on the next frame.

Use `create_service` to spawn an async background service that is automatically cleaned up when the component unmounts:

```rust
let data = create_signal(String::new());
let data_w = data.writer();  // WriteSignal<T> — Send, for background tasks

// Spawn a background service - automatically cleaned up on unmount
create_task(move |ctx| async move {
    while ctx.is_running() {
        let new_data = fetch_data();
        data_w.set(new_data);  // Queued, applied next frame
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});
```

**Note:** Capturing `data` (an `RwSignal`) directly in a service closure will **not compile** because `RwSignal` is `!Send`. Always use `.writer()` to get a `WriteSignal` for background tasks.

For bidirectional communication (sending commands to the service):

```rust
enum Cmd { Refresh, Stop }

let status = create_signal("idle".to_string());
let status_w = status.writer();  // WriteSignal for bg task

let service = create_service(move |mut rx, ctx| async move {
    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    Cmd::Refresh => {
                        status_w.set("refreshing".to_string());
                        // ... do work ...
                        status_w.set("idle".to_string());
                    }
                    Cmd::Stop => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if !ctx.is_running() { break; }
            }
        }
    }
});

// Send commands from UI callbacks
service.send(Cmd::Refresh);
```

## State vs. Events (Write-Site Equality)

A signal is **state**: a cell holding a current value, notifying when the
value *changes*. `set` compares with `PartialEq` and deduplicates equal
writes — that is what keeps an idle bar idle when services republish
unchanged data.

An **event** is different: two identical emissions are two emissions.
Rather than a separate signal variant, equality is a property of the
**write site** — the writer knows whether it is publishing state or
firing a trigger:

```rust
volume.set(50);          // state: equal values deduplicate
osd.set_always(info);    // trigger: every emission notifies
refresh.notify();        // Trigger (create_trigger) = RwSignal<()> + set_always
```

`set_always` / `update_always` (and their `WriteSignal` counterparts)
have no `PartialEq` bound — types without meaningful equality can only
be written this way, and the compiler steers them there. `create_signal`
itself requires only `Clone + Send`.

Two caveats define the boundary of this tool:

- A signal still holds ONE value: two `set_always` calls before the
  consumer runs coalesce to the last value. That is correct for
  frame-coalescing triggers (an OSD flash, an animation kick) — the
  screen only shows the last frame anyway. **Event streams that must not
  lose emissions belong in an async channel** (a service's command
  queue), not in the reactive graph.
- Never fake events by giving a type a lying `PartialEq` (a version
  counter compared instead of the data): it breaks `==` for every other
  use and hides the intent. `set_always` says the same thing honestly.

For comparison: Solid deduplicates by default with per-signal opt-out
(`equals: false`); Leptos and Floem never deduplicate on `set` and rely
on `Memo` for cutoff. Guido keeps dedup as the default (idle cost
matters here) and opts out per-write.

## Copy Handles

Signals, [`Service`](#background-task-updates) and `Callback` are all `Copy`:
each is an id into the reactive arena, not the thing itself. That is what lets
them be dropped into as many closures as a view needs without a clone, and what
keeps a struct that groups them `Copy` too:

```rust
#[derive(Clone, Copy)]
struct AudioHandles {
    data: RwSignal<Option<AudioService>>,   // Copy
    svc: Service<AudioCommand>,             // Copy
    on_toggle: Callback,                    // Copy
}
```

The `Send` halves are separate by design, since the arena is main-thread only:
`RwSignal::writer()` for a background write, `Service::sender()` for a command
from a background task.

```rust
let cb = Callback::new(move |v: i32| println!("{v}"));
cb.run(7);          // arity stays flat: run(), run(a), run(a, b)
```

## Signal Internals

Signals are lightweight handles that index into thread-local storage:

```rust
#[derive(Clone, Copy)]
pub struct RwSignal<T> {
    id: SignalId,       // 8 bytes — read-write handle
}

#[derive(Clone, Copy)]
pub struct Signal<T> {
    id: SignalId,
    kind: SignalKind,   // 12 bytes total — read-only, supports stored/mutable/derived
}
```

`SignalId` is generational — `{ index: u32, generation: u32 }`. Storage slots
are recycled with a bumped generation, so a stale `Copy` handle held after its
owner was disposed can never silently alias the slot's next occupant: reads of
disposed signals fail loudly. Effect and owner ids use the same scheme.

The actual value is stored in `thread_local! { RefCell<SignalStorage> }`, accessed by `id`. This design allows:
- Both types to be `Copy`
- Zero-lock access on the main thread (thread-local `RefCell` only)
- Automatic dependency tracking via thread-local runtime

`RwSignal<T>` is the compact read-write handle (8 bytes). `Signal<T>` is the read-only wrapper (12 bytes) that also supports static (`create_stored`) and derived (`create_derived`) values via its `SignalKind` discriminant.

Effect callbacks execute with no internal borrow held: writing a signal inside
an effect notifies its subscribers normally, so effect → effect and memo → memo
chains work. Panics inside effects or `batch()` are unwind-safe — scope state
restores via Drop guards and the effect stays alive and re-runnable.

`WriteSignal<T>` is a separate `Send` handle that queues writes through a thread-safe channel, which the main thread drains each frame:

```rust
pub struct WriteSignal<T> { /* Send */ }
```

## Dependency Tracking

When a signal is read inside a `Memo` or `Effect`, the runtime automatically registers the dependency:

```rust
let a = create_signal(1);
let b = create_signal(2);

// This memo depends on both `a` and `b`
let sum = create_memo(move || a.get() + b.get());
```

Changing either `a` or `b` will cause `sum` to recompute.

Widget properties also participate in auto-tracking. During `paint()` and `layout()`, any signal reads (including inside closures passed as properties) are automatically tracked, so the widget is repainted or relaid out when dependencies change.

### Cached Copies of Signal State (Invariant)

Widget-side tracking is **lazy**: a subscription exists only after the first
tracked read. This is safe for plain properties because layout/paint always
pull the *current* signal value — a write that lands before the first read
is not lost, the first read simply sees it.

It is NOT safe for state that keeps a **copy** of a signal-derived value and
updates it via subscription-triggered jobs (push). A write landing before
the subscription exists notifies nobody, and the copy goes stale forever.
`AnimationState` (animation targets) is exactly this shape, and any future
cache of signal-derived state will be too. Such caches MUST do both of:

1. **Register their subscriptions on the widget's first `layout()`** —
   creation and first layout run in one synchronous block, so no write can
   land in between (see the first-layout Animation tracking pass in
   `Container::layout`).
2. **Reconcile pull-style at each use** — compare the stored copy against
   the current effective value and schedule a catch-up job on drift (see
   the target-drift pass in `Container::paint`). This also converges
   dependencies read through conditional closure branches and values that
   change without any signal write (state-layer overrides).

Regression tests: `write_between_first_layout_and_first_paint_starts_animation`
and `padding_write_after_first_layout_schedules_animation`.

## Best Practices

### Minimize Signal Reads

Read signals as close to where the value is needed:

```rust
// Good: Read in closure where it's used
text(move || format!("Count: {}", count.get()))

// Less optimal: Read early, pass static value
let value = count.get();
text(format!("Count: {}", value))  // Won't update when count changes
```

### Use Memo for Derived State

Instead of manually syncing values:

```rust
// Bad: Manual sync
let count = create_signal(0);
let doubled = create_signal(0);
// Must remember to update doubled when count changes

// Good: Use memo
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);
```

## Reactive Ownership (Resource Cleanup)

Signals and effects persist in memory by default. The **reactive owner** system provides automatic cleanup when components are removed.

### Automatic Ownership for Dynamic Children

**Dynamic children automatically get owner scopes.** Reactive child closures
and `keyed()` builders run inside an owner scope: any signals, effects, or
cleanup callbacks created there are automatically owned and cleaned up when
the child is removed or rebuilt:

```rust
let items = create_signal(vec![1u64, 2, 3]);

container().children(keyed(
    move || items.get(),
    |id| *id,
    |id| {
        // ========================================================
        // Everything created inside the builder is AUTOMATICALLY
        // owned by the child's owner scope. When the child is
        // removed, all these resources are automatically cleaned up!
        // ========================================================

        // This signal is owned by the child
        let local_count = create_signal(0);

        // This effect is also owned - disposed when child is removed
        create_effect(move || {
            println!("Child {} count: {}", id, local_count.get());
        });

        // Register cleanup for non-reactive resources
        on_cleanup(move || {
            println!("Child {} was removed!", id);
        });

        container()
            .on_click(move || local_count.update(|c| *c += 1))
            .child(text(move || format!("Child {} ({})", id, local_count.get())))
    },
));

// When an item is removed from the list:
// 1. The child's OwnedWidget is dropped
// 2. dispose_owner() is called automatically
// 3. on_cleanup callbacks run
// 4. Effects are disposed
// 5. Signals are disposed
```

**Important:** resources must be created inside the builder. Signals created
while producing the data won't be owned:

```rust
// WRONG - signal not owned (created in the data closure)
keyed(
    move || items.get().into_iter().map(|id| {
        let signal = create_signal(0);  // NOT OWNED!
        (id, signal)
    }).collect::<Vec<_>>(),
    |(id, _)| *id,
    |(_, signal)| container().child(text(move || signal.get().to_string())),
)

// CORRECT - signal owned (created inside the builder)
keyed(
    move || items.get(),
    |id| *id,
    |id| {
        let signal = create_signal(0);  // OWNED!
        container().child(text(move || signal.get().to_string()))
    },
)
```

You can also extract the child creation into a function:
```rust
fn create_child(id: u64) -> impl Widget {
    let signal = create_signal(0);  // OWNED!
    on_cleanup(|| println!("Child {} cleaned up", id));
    container().child(text(move || signal.get().to_string()))
}

container().children(keyed(move || items.get(), |id| *id, create_child))
```

### Disposing Owner Scopes: `dispose_owner`

`guido::reactive::dispose_owner(id)` disposes an owner created with
`with_owner`: all its signals, effects, and cleanup callbacks.

Disposal is **deferred**: the main loop runs pending disposals once per
iteration, at a point where no user closure is on the stack. That makes
it safe to call from anywhere — including code the owner itself owns
(an effect observing a popup's dismissal, a dialog's own close button)
and code whose widgets outlive the current instant (a popup surface that
lives until its Close command is processed). Deferral is a mechanism,
not a second API: there is nothing to choose between.

```rust
use guido::reactive::dispose_owner;

// The owner id only exists after with_owner returns, so code inside the
// scope reaches it through app state (a slot, a thread-local registry):
let owner_slot = Rc::new(Cell::new(None));
let slot = owner_slot.clone();
let (popup, owner_id) = guido::reactive::owner::with_owner(move || {
    spawn_popup(bar, config, move || {
        menu_view(move || {
            // Close button: this callback is owned by the very scope it
            // is tearing down — safe, disposal runs at the next loop
            // iteration:
            if let Some(id) = slot.get() {
                dispose_owner(id);
            }
        })
    })
});
owner_slot.set(Some(owner_id));
```

Disposing twice, or disposing an already-disposed owner, is harmless.
(The library's own teardown paths — surface close, reconcile discard,
component Drop — use an internal synchronous variant whose ordering
guarantees they control.)

### Custom Cleanup Callbacks

Use `on_cleanup` inside dynamic children or component render methods to register cleanup logic for non-reactive resources:

```rust
container().children(move || {
    items.get().into_iter().map(|id| (id, move || {
        // Start a background thread
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        std::thread::spawn(move || {
            while running_clone.load(Ordering::SeqCst) {
                // ... do work
            }
        });

        // Register cleanup to stop the thread when child is removed
        on_cleanup(move || {
            running.store(false, Ordering::SeqCst);
        });

        container().child(text(format!("Child {}", id)))
    }))
});
```

### Nested Owners

Owner scopes are automatically nested. When a parent owner is disposed, children are disposed first (depth-first). This happens automatically when removing nested dynamic children.

### Component Macro Integration

Components created with `#[component]` automatically wrap their render body in an owner scope. When the component is dropped, all its reactive resources are cleaned up:

```rust
#[component]
pub fn counter(initial: i32) -> impl Widget {
    // This signal is owned by the component
    let count = create_signal(initial.get());

    // This effect is also owned
    create_effect(move || {
        println!("Count: {}", count.get());
    });

    // When Counter is dropped, signal and effect are disposed
    container()
        .on_click(move || count.update(|c| *c += 1))
        .child(text(move || count.get().to_string()))
}
```

### Accessing Disposed Signals

Attempting to read or write a disposed signal will panic with a clear error message. This typically happens if you store a signal reference outside its owner scope and try to use it after the child is removed:

```rust
// DON'T DO THIS - signal may be accessed after disposal
let leaked_signal: Option<Signal<i32>> = None;

container().children(keyed(
    move || items.get(),
    |id| *id,
    |id| {
        let signal = create_signal(0);
        // WRONG: Don't leak signals outside their owner
        // leaked_signal = Some(signal);

        container().child(text(move || signal.get().to_string()))
    },
));

// If you access leaked_signal after the child is removed,
// you'll get a panic: "Signal was disposed - cannot read after owner cleanup."
```

This behavior helps catch bugs where signals are used after their owner has been disposed.

## API Reference

### Signal Creation

```rust
pub fn create_signal<T: Clone + PartialEq + Send + 'static>(value: T) -> RwSignal<T>;
pub fn create_memo<T: Clone + PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T>;
pub fn create_effect(f: impl Fn() + 'static);
```

`create_signal` returns `RwSignal<T>` (8 bytes, read-write). It requires `Send` because `WriteSignal<T>` must be able to queue values from background threads.

### Cleanup Functions

```rust
/// Register a cleanup callback for the current owner.
/// Use this inside dynamic children or component render() methods
/// to clean up non-reactive resources (timers, threads, connections).
pub fn on_cleanup(f: impl FnOnce() + 'static);
```

**Note:** `with_owner` and `dispose_owner` are internal functions used by the framework. User code should rely on automatic ownership via dynamic children and the `#[component]` macro.

### RwSignal Methods (main-thread only)

```rust
impl<T: Clone> RwSignal<T> {
    pub fn get(&self) -> T;                // Read with tracking
    pub fn get_untracked(&self) -> T;      // Read without tracking
    pub fn set(&self, value: T);           // Set immediately
    pub fn update(&self, f: impl FnOnce(&mut T));  // Mutate in-place
    pub fn writer(&self) -> WriteSignal<T>;  // Get a Send handle for bg threads
}
```

`RwSignal<T>` is `!Send` — all methods above must be called on the main thread.

### Signal Methods (read-only, main-thread only)

```rust
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T;                // Read with tracking
    pub fn get_untracked(&self) -> T;      // Read without tracking
}
```

`Signal<T>` is read-only — no `.set()`, `.update()`, or `.writer()` methods.

### WriteSignal Methods (Send — background threads)

```rust
impl<T: Clone + Send> WriteSignal<T> {
    pub fn set(&self, value: T);              // Queue a write (applied next frame)
    pub fn update(&self, f: impl FnOnce(&mut T));  // Queue a mutation (applied next frame)
}
```

`WriteSignal<T>` is `Send` and can be moved into background threads (e.g., `create_service` closures). Writes are queued and applied on the main thread at the start of the next frame.

### Memo Methods

```rust
impl<T: Clone + PartialEq> Memo<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow with tracking
}
```
