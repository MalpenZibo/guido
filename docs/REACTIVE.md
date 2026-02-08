# Reactive System

Guido uses a fine-grained reactive system inspired by SolidJS and Floem. This enables efficient updates where only the affected parts of the UI re-render.

## Core Concepts

### Signals

Signals are reactive values that notify dependents when they change:

```rust
use guido::prelude::*;

let count = create_signal(0);

// Read the current value
let value = count.get();

// Set a new value
count.set(5);

// Update based on current value
count.update(|c| *c += 1);
```

**Key properties:**
- Signals are `Copy` - no cloning needed
- Thread-safe - can be updated from background threads
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

### Field Selection

`Signal::select()` creates a derived signal that tracks a specific field of the parent signal's value. The derived signal only clones the field when it actually changes — no unnecessary clones of the entire parent object:

```rust
let data = create_signal(MyStruct { name: "Alice".into(), count: 0 });

// Derived signal that tracks only the `name` field
let name: Signal<String> = data.select(|d| &d.name);

// UI only re-renders when `name` actually changes
text(move || format!("Name: {}", name.get()))

// Changing `count` does NOT trigger `name` to update
data.update(|d| d.count += 1);

// Changing `name` DOES trigger `name` to update
data.update(|d| d.name = "Bob".into());
```

Selects can be chained for nested fields:

```rust
let inner_value = data.select(|d| &d.inner).select(|i| &i.value);
```

**Key properties:**
- Uses the invalidation system (global Mutex), not effects — works with background thread updates from `create_service`
- In-place comparison — the parent value is never cloned just for comparison
- Only clones when the selected field actually changed
- Cleanup is automatic via the `on_cleanup` ownership system
- Available on both `Signal<T>` and `ReadSignal<T>`

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

Dynamic lists with keyed reconciliation:

```rust
let items = create_signal(vec![
    ("a", "Item A"),
    ("b", "Item B"),
    ("c", "Item C"),
]);

container().children(move || {
    items.get().into_iter().map(|(id, label)| {
        let key = id.as_ptr() as u64;  // Use stable key
        (key, move || text(label))     // Closure returns widget
    })
})
```

The key ensures widget state is preserved when items are reordered.

## MaybeDyn Pattern

The `MaybeDyn<T>` enum allows properties to be either static or dynamic:

```rust
pub enum MaybeDyn<T> {
    Static(T),
    Dynamic(Box<dyn Fn() -> T>),
}
```

Properties use `impl IntoMaybeDyn<T>` to accept any of:
- Static value: `T`
- Signal: `Signal<T>`
- Closure: `impl Fn() -> T`

## Background Thread Updates

Signals are thread-safe and can be updated from background threads. Use `create_service` to spawn a background service that is automatically cleaned up when the component unmounts:

```rust
let data = create_signal(String::new());

// Spawn a background service - automatically cleaned up on unmount
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        let new_data = fetch_data();
        data.set(new_data);
        std::thread::sleep(Duration::from_secs(1));
    }
});
```

For bidirectional communication (sending commands to the service):

```rust
enum Cmd { Refresh, Stop }

let service = create_service(move |rx, ctx| {
    while ctx.is_running() {
        // Handle commands from UI
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Refresh => { /* refresh data */ }
                Cmd::Stop => break,
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
});

// Send commands from UI callbacks
service.send(Cmd::Refresh);
```

## Signal Internals

Signals use `Arc` internally for cheap copies:

```rust
#[derive(Clone, Copy)]
pub struct Signal<T> {
    id: SignalId,
}
```

The actual value is stored in a global runtime using the `id`. This design allows:
- Signals to be `Copy`
- Thread-safe access via `Arc<Mutex<T>>`
- Automatic dependency tracking via thread-local runtime

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

### Batch Updates

When updating multiple related signals, the render will naturally batch:

```rust
// Both updates happen before the next render
first_name.set("John");
last_name.set("Doe");
```

## Reactive Ownership (Resource Cleanup)

Signals and effects persist in memory by default. The **reactive owner** system provides automatic cleanup when components are removed.

### Automatic Ownership for Dynamic Children

**Dynamic children automatically get owner scopes.** Return `(key, closure)` pairs where the closure produces the widget. Any signals, effects, or cleanup callbacks created inside the closure are automatically owned and cleaned up when the child is removed:

```rust
let items = create_signal(vec![1u64, 2, 3]);

container().children(move || {
    items.get().into_iter().map(|id| {
        // Return (key, closure) - the closure runs inside an owner scope
        (id, move || {
            // ========================================================
            // Everything created inside this closure is AUTOMATICALLY
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
        })
    })
});

// When an item is removed from the list:
// 1. The child's OwnedWidget is dropped
// 2. dispose_owner() is called automatically
// 3. on_cleanup callbacks run
// 4. Effects are disposed
// 5. Signals are disposed
```

**Important:** The closure syntax `(key, move || { ... })` is required for proper ownership. Signals created outside the closure won't be owned:

```rust
// WRONG - signal not owned (created outside closure)
.map(|id| {
    let signal = create_signal(0);  // NOT OWNED!
    (id, container().child(...))
})

// CORRECT - signal owned (created inside closure)
.map(|id| (id, move || {
    let signal = create_signal(0);  // OWNED!
    container().child(...)
}))
```

You can also extract the child creation into a function:
```rust
fn create_child(id: u64) -> impl Widget {
    let signal = create_signal(0);  // OWNED!
    on_cleanup(|| println!("Child {} cleaned up", id));
    container().child(text(move || signal.get().to_string()))
}

// Call the function inside the closure
container().children(move || {
    items.get().into_iter().map(|id| (id, move || create_child(id)))
})
```

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

Components created with `#[component]` automatically wrap their `render()` in an owner scope. When the component is dropped, all its reactive resources are cleaned up:

```rust
#[component]
pub struct Counter {
    #[prop]
    initial: i32,
}

impl Counter {
    fn render(&self) -> impl Widget {
        // This signal is owned by the component
        let count = create_signal(self.initial.get());

        // This effect is also owned
        create_effect(move || {
            println!("Count: {}", count.get());
        });

        // When Counter is dropped, signal and effect are disposed
        container()
            .on_click(move || count.update(|c| *c += 1))
            .child(text(move || count.get().to_string()))
    }
}
```

### Accessing Disposed Signals

Attempting to read or write a disposed signal will panic with a clear error message. This typically happens if you store a signal reference outside its owner scope and try to use it after the child is removed:

```rust
// DON'T DO THIS - signal may be accessed after disposal
let leaked_signal: Option<Signal<i32>> = None;

container().children(move || {
    items.get().into_iter().map(|id| {
        let signal = create_signal(0);
        // WRONG: Don't leak signals outside their owner
        // leaked_signal = Some(signal);

        (id, container().child(text(move || signal.get().to_string())))
    })
});

// If you access leaked_signal after the child is removed,
// you'll get a panic: "Signal was disposed - cannot read after owner cleanup."
```

This behavior helps catch bugs where signals are used after their owner has been disposed.

## API Reference

### Signal Creation

```rust
pub fn create_signal<T: Clone + 'static>(value: T) -> Signal<T>;
pub fn create_memo<T: Clone + PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T>;
pub fn create_effect(f: impl Fn() + 'static);
```

### Cleanup Functions

```rust
/// Register a cleanup callback for the current owner.
/// Use this inside dynamic children or component render() methods
/// to clean up non-reactive resources (timers, threads, connections).
pub fn on_cleanup(f: impl FnOnce() + 'static);
```

**Note:** `with_owner` and `dispose_owner` are internal functions used by the framework. User code should rely on automatic ownership via dynamic children and the `#[component]` macro.

### Signal Methods

```rust
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T;
    pub fn get_untracked(&self) -> T;  // Read without tracking
    pub fn set(&self, value: T);
    pub fn update(&self, f: impl FnOnce(&mut T));
    pub fn select<U>(&self, f: impl Fn(&T) -> &U) -> Signal<U>;  // Field selection
}
```

### Memo Methods

```rust
impl<T: Clone + PartialEq> Memo<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn get_untracked(&self) -> T; // Read without tracking
}
```
