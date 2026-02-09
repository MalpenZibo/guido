# Reactive Model

Guido uses a fine-grained reactive system inspired by SolidJS. This enables efficient updates where only the affected parts of the UI change.

## Signals

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

### Key Properties

- **Copy** - Signals implement `Copy`, so you can use them in multiple closures without cloning
- **Background updates** - Use `.writer()` to get a `WriteSignal<T>` for background task updates
- **Automatic tracking** - Dependencies are tracked when reading inside reactive contexts

## Memos

Eagerly computed values that automatically update when their dependencies change. Memos only notify downstream subscribers when the result actually differs (`PartialEq`), preventing unnecessary updates:

```rust
let count = create_signal(0);
let doubled = create_memo(move || count.get() * 2);

count.set(5);
println!("{}", doubled.get()); // Prints: 10
```

Memos are `Copy` like signals and can be used directly as widget properties:

```rust
let label = create_memo(move || format!("Count: {}", count.get()));
text(label)  // Only repaints when the formatted string changes
```

## Effects

Side effects that re-run when tracked signals change:

```rust
let name = create_signal("World".to_string());

create_effect(move || {
    println!("Hello, {}!", name.get());
});

name.set("Guido".to_string()); // Effect re-runs, prints: Hello, Guido!
```

Effects are useful for logging, syncing with external systems, or triggering actions.

## Using Signals in Widgets

Most widget properties accept either static values or reactive sources:

### Static Value

```rust
container().background(Color::RED)
```

### Signal

```rust
let bg = create_signal(Color::RED);
container().background(bg)
```

### Closure

```rust
let is_active = create_signal(false);
container().background(move || {
    if is_active.get() { Color::GREEN } else { Color::RED }
})
```

## Reactive Text

Text content can be reactive using closures:

```rust
let count = create_signal(0);

text(move || format!("Count: {}", count.get()))
```

The text automatically updates when `count` changes.

## The MaybeDyn Pattern

Under the hood, Guido uses `MaybeDyn<T>` to accept static or dynamic values:

```rust
pub enum MaybeDyn<T> {
    Static(T),
    Dynamic(Rc<dyn Fn() -> T>),
}
```

You don't need to use this directly - the `impl IntoMaybeDyn<T>` trait accepts:
- Static values: `T`
- Signals: `Signal<T>`
- Closures: `impl Fn() -> T`

## Per-Field Signals

When multiple widgets depend on different fields of the same struct, `#[derive(SignalFields)]` generates per-field signals so each widget only re-renders when its specific field changes:

```rust
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

```rust
let writers = state.writers();

let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
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
```

Generic structs are supported — the generated types carry the same generic parameters:

```rust
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
let count = create_signal(0);

// Normal read - creates dependency
let value = count.get();

// Untracked read - no dependency
let value = count.get_untracked();
```

This is useful in effects where you want to read initial values without re-running on changes.

## Ownership & Cleanup

Signals and effects created inside dynamic children are automatically cleaned up when the child is removed. Use `on_cleanup` to register custom cleanup logic:

```rust
container().children(move || {
    items.get().into_iter().map(|id| (id, move || {
        // These are automatically owned and disposed
        let count = create_signal(0);
        create_effect(move || println!("Count: {}", count.get()));

        // Register custom cleanup for non-reactive resources
        on_cleanup(move || {
            println!("Child {} removed", id);
        });

        container().child(text(move || count.get().to_string()))
    }))
})
```

See [Dynamic Children](../advanced/dynamic-children.md) for more details on automatic ownership.

## Best Practices

### Read Close to Usage

Read signals where the value is needed, not at the top of functions:

```rust
// Good: Read in closure where it's used
text(move || format!("Count: {}", count.get()))

// Less optimal: Read early, pass static value
let value = count.get();
text(format!("Count: {}", value))  // Won't update!
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

## API Reference

### Signal Creation

```rust
pub fn create_signal<T: Clone + 'static>(value: T) -> Signal<T>;
pub fn create_memo<T: Clone + PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T>;
pub fn create_effect(f: impl Fn() + 'static);
```

### Signal Methods

```rust
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn get_untracked(&self) -> T; // Read without tracking
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow with tracking
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow without tracking
    pub fn set(&self, value: T);      // Set new value
    pub fn update(&self, f: impl FnOnce(&mut T)); // Update in place
    pub fn writer(&self) -> WriteSignal<T>; // Get Send handle for background threads
}
```

### Memo Methods

```rust
impl<T: Clone + PartialEq> Memo<T> {
    pub fn get(&self) -> T;           // Read with tracking
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R; // Borrow with tracking
}
```

### Cleanup

```rust
// Register cleanup callback (for use in dynamic children)
pub fn on_cleanup(f: impl FnOnce() + 'static);
```

### Background Services

```rust
// Create an async background service with automatic cleanup
pub fn create_service<Cmd, F, Fut>(f: F) -> Service<Cmd>
where
    Cmd: Send + 'static,
    F: FnOnce(UnboundedReceiver<Cmd>, ServiceContext) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static;
```

See [Background Tasks](../advanced/background-threads.md) for detailed usage.
