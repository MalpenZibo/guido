# Reactive Model

Guido uses a fine-grained reactive system inspired by SolidJS. This enables efficient updates where only the affected parts of the UI change.

## RwSignal (Read-Write)

`create_signal()` returns an `RwSignal<T>` — a read-write reactive value (8 bytes, `Copy`):

```rust
# extern crate guido;
# fn main() {
use guido::prelude::*;

let count = create_signal(0); // RwSignal<i32>

// Read the current value
let value = count.get();

// Set a new value
count.set(5);

// Update based on current value
count.update(|c| *c += 1);
# ;
# }
```

### Key Properties

- **Copy** - `RwSignal` implements `Copy`, so you can use it in multiple closures without cloning
- **Background updates** - Use `.writer()` to get a `WriteSignal<T>` for background task updates
- **Automatic tracking** - Dependencies are tracked when reading inside reactive contexts
- **Converts to Signal** - Call `.read_only()` or `.into()` to get a read-only `Signal<T>`

## State vs. Triggers: `set` and `set_always`

`set` publishes **state**: equal values are deduplicated (`PartialEq`),
so republishing unchanged data costs nothing. `set_always` fires a
**trigger**: every write notifies, no comparison, no `PartialEq` bound —
the natural write for "something happened" values like an OSD flash:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn osd() -> Container { container() }
# fn main() {
# let volume = create_signal(0.5f32);
volume.set(50);          // state: setting 50 twice notifies once
osd.set_always(info);    // trigger: every flash notifies
# ;
# }
```

For a data-less pulse there is `Trigger`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rebuild = move || {};
let refresh = create_trigger();
create_effect(move || {
    refresh.track();
    rebuild();
});
refresh.notify(); // rebuild() runs every time
# ;
# }
```

Signals hold one value, so rapid `set_always` calls coalesce to the last
one per frame — perfect for UI triggers, wrong for event streams that
must not lose emissions (those belong in an async channel).

## Signal (Read-Only)

`Signal<T>` is a read-only reactive value (12 bytes, `Copy`). It cannot be written to — calling `.set()` is a compile-time error. There are two ways to create one:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let count = create_signal(0);
// Stored: wraps a static value
let name = create_stored("hello".to_string()); // Signal<String>

// Derived: closure-backed, re-evaluates on each read
let doubled = create_derived(move || count.get() * 2); // Signal<i32>
# ;
# }
```

You can also convert an `RwSignal` to a `Signal`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);
let read_only: Signal<i32> = count.read_only(); // or count.into()
# ;
# }
```

## Memos

Eagerly computed values that automatically update when their dependencies change. Memos only notify downstream subscribers when the result actually differs (`PartialEq`), preventing unnecessary updates:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);

count.set(5);
println!("{}", doubled.get()); // Prints: 10
# ;
# }
```

Memos are `Copy` like signals and can be used directly as widget properties:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let count = create_signal(0);
let label = create_memo(move || format!("Count: {}", count.get()));
text(label)  // Only repaints when the formatted string changes
# ;
# }
```

## Effects

Side effects that re-run when tracked signals change:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let name = create_signal("World".to_string());

create_effect(move || {
    println!("Hello, {}!", name.get());
});

name.set("Guido".to_string()); // Effect re-runs, prints: Hello, Guido!
# ;
# }
```

Effects are useful for logging, syncing with external systems, or triggering actions.

## Using Signals in Widgets

Most widget properties accept either static values or reactive sources:

### Static Value

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().background(Color::RED)
# ;
# }
```

### Signal

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let bg = create_signal(Color::RED);
container().background(bg)
# ;
# }
```

### Closure

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let is_active = create_signal(false);
container().background(move || {
    if is_active.get() { Color::GREEN } else { Color::RED }
})
# ;
# }
```

## Reactive Text

Text content can be reactive using closures:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);

text(move || format!("Count: {}", count.get()))
# ;
# }
```

The text automatically updates when `count` changes.

## The IntoSignal Pattern

Widget properties accept `Signal<T>` via the `IntoSignal<T>` trait. Both `RwSignal<T>` and `Signal<T>` work, along with raw values and closures:

- **Static values** → creates a stored `Signal<T>` via `create_stored()`
- **Closures** → creates a derived `Signal<T>` via `create_derived()`
- **`Signal<T>`** → passed through directly
- **`RwSignal<T>`** → automatically converted to `Signal<T>`

You don't need to create signals manually for widget properties — just pass values, closures, or signals directly.

## Per-Field Signals

When multiple widgets depend on different fields of the same struct, `#[derive(SignalFields)]` generates per-field signals so each widget only re-renders when its specific field changes:

```rust,ignore
#[derive(Clone, PartialEq, SignalFields)]
pub struct AppState {
    pub cpu: f64,
    pub memory: f64,
    pub title: String,
}

// Creates individual Signal<T> for each field
let state = AppStateSignals::new(AppState {
    cpu: 0.0, memory: 0.0, title: "App".into(),
});

// Each widget subscribes to only the field it reads
text(move || format!("CPU: {:.0}%", state.cpu.get()))
text(move || format!("MEM: {:.0}%", state.memory.get()))
text(move || state.title.get())
```

Use `.writers()` to get `Send` handles for background task updates:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let writers = state.writers();

create_task(move |ctx| async move {
    while ctx.is_running() {
        // Each field is set individually with PartialEq change detection.
        // Effects that depend on multiple fields run only once (batched).
        writers.set(AppState {
            cpu: read_cpu(),
            memory: read_memory(),
            title: get_title(),
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});
# ;
# }
```

Generic structs are supported — the generated types carry the same generic parameters:

```rust,ignore
#[derive(Clone, PartialEq, SignalFields)]
pub struct Pair<A: Clone + PartialEq + Send + 'static, B: Clone + PartialEq + Send + 'static> {
    pub first: A,
    pub second: B,
}

let pair = PairSignals::new(Pair { first: 1i32, second: "hello".to_string() });
```

## Untracked Reads

Sometimes you want to read a signal without creating a dependency:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);

// Normal read - creates dependency
let value = count.get();

// Untracked read - no dependency
let value = count.get_untracked();
# ;
# }
```

This is useful in effects where you want to read initial values without re-running on changes.

## Ownership & Cleanup

Signals and effects created inside dynamic children are automatically cleaned up when the child is removed. Use `on_cleanup` to register custom cleanup logic:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, PartialEq)]
# struct Item { id: u32, label: String }
# fn main() {
# let items = create_signal(vec![Item { id: 1, label: String::from("one") }]);
container().children(keyed(
    move || items.get(),
    |id| *id,
    |id| {
        // These are automatically owned and disposed
        let count = create_signal(0);
        create_effect(move || println!("Count: {}", count.get()));

        // Register custom cleanup for non-reactive resources
        on_cleanup(move || {
            println!("Child {} removed", id);
        });

        container().child(text(move || count.get().to_string()))
    },
))
# ;
# }
```

See [Dynamic Children](../advanced/dynamic-children.md) for more details on automatic ownership.

## Best Practices

### Read Close to Usage

Read signals where the value is needed, not at the top of functions:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let count = create_signal(0);
// Good: Read in closure where it's used
text(move || format!("Count: {}", count.get()));

// Less optimal: Read early, pass static value
let value = count.get();
text(format!("Count: {}", value))  // Won't update!
# ;
# }
```

### Use Context for App-Wide State

For values that many widgets across different modules need (config, theme, services), use the [Context API](../advanced/context.md) instead of passing signals through every function:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Default)]
# struct Config;
# impl Config { fn load() -> Self { Self } }
# fn main() {
// Setup
provide_context(Config::load());

// Any widget, any module
let cfg = expect_context::<Config>();
# ;
# }
```

For mutable shared state, use `provide_signal_context` to combine context with reactivity.

### Use Memo for Derived State

Instead of manually syncing values:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Bad: Manual sync
let count = create_signal(0);
let doubled = create_signal(0);
// Must remember to update doubled when count changes

// Good: Use memo
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);
# ;
# }
```

A memo is also a **tracking barrier**, which is what makes it the tool for
narrowing a rebuild. Signals read inside it belong to it alone, even when the
memo is created inside something that is itself tracked — a dynamic-children
closure, a paint pass, an effect:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn expensive_module() -> Container { container() }
# fn main() {
# let levels = create_signal(vec![0.2f32, 0.5, 0.9]);
// `levels` is written 60 times a second by an audio service
container().child(move || {
    let active = create_memo(move || levels.with(|l| !l.is_empty()));
    // This closure depends on the bool, not on `levels`: the subtree is
    // rebuilt when the bool flips, not on every write
    active.get().then(|| expensive_module())
})
# ;
# }
```

## Snapshots vs. Reactive Reads

Reading a signal subscribes whoever is *currently* running. Inside a closure
that guido re-runs — a widget property, a child closure, an effect — that means
the UI follows the value. Outside one, there is nobody to subscribe, so the
read is a snapshot:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let count_as_string = create_memo(move || String::new());
# let count = create_signal(0);
text(format!("{}", count.get()));          // snapshot: never updates again
text(move || format!("{}", count.get()));  // reactive
text(count_as_string)                     // reactive: pass the signal itself
# ;
# }
```

Rust cannot tell those apart at compile time, so **debug builds print a warning
at the offending line**:

```text
guido: src/main.rs:42:31: signal read with no reactive scope — this value is a
snapshot and will not update. Pass a closure instead …
```

If the snapshot is what you meant, say so with `get_untracked()` and the
warning goes away. Release builds contain none of this.

## API Reference

### Signal Creation

```rust,ignore
pub fn create_signal<T: Clone + PartialEq + Send + 'static>(value: T) -> RwSignal<T>;
pub fn create_stored<T: Clone + 'static>(value: T) -> Signal<T>;
pub fn create_derived<T: Clone + 'static>(f: impl Fn() -> T + 'static) -> Signal<T>;
pub fn create_memo<T: Clone + PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T>;
pub fn create_effect(f: impl Fn() + 'static);
```

### RwSignal Methods

```rust,ignore
impl<T: Clone> RwSignal<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn get_untracked(&self) -> T; // Read without tracking
    pub fn set(&self, value: T);      // Set new value
    pub fn update(&self, f: impl FnOnce(&mut T)); // Update in place
    pub fn writer(&self) -> WriteSignal<T>; // Get Send handle for background threads
    pub fn read_only(&self) -> Signal<T>;   // Convert to read-only Signal
}
```

### Signal Methods

```rust,ignore
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn get_untracked(&self) -> T; // Read without tracking
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow with tracking
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow without tracking
    // No set/update/writer — Signal is read-only
}
```

### Memo Methods

```rust,ignore
impl<T: Clone + PartialEq> Memo<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow with tracking
}
```

### Cleanup

```rust,ignore
// Register cleanup callback (for use in dynamic children)
pub fn on_cleanup(f: impl FnOnce() + 'static);
```

### Background Services

```rust,ignore
// Create an async background service with automatic cleanup
pub fn create_service<Cmd, F, Fut>(f: F) -> Service<Cmd>
where
    Cmd: Send + 'static,
    F: FnOnce(UnboundedReceiver<Cmd>, ServiceContext) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static;
```

See [Background Tasks](../advanced/background-threads.md) for detailed usage.
