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

### Computed Values

Derived values that automatically update when dependencies change:

```rust
let count = create_signal(0);
let doubled = create_computed(move || count.get() * 2);

count.set(5);
println!("{}", doubled.get()); // Prints: 10
```

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
let items = create_signal(vec!["A", "B", "C"]);

container()
    .children_dyn(
        move || items.get(),
        |item| item.to_string(),  // Key function
        |item| text(*item),       // View function
    )
```

The key function ensures widget state is preserved when items are reordered.

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

Signals are thread-safe and can be updated from background threads:

```rust
let data = create_signal(String::new());
let (tx, rx) = std::sync::mpsc::channel();

// Spawn background worker
std::thread::spawn(move || {
    loop {
        let new_data = fetch_data();
        tx.send(new_data).ok();
        std::thread::sleep(Duration::from_secs(1));
    }
});

// Poll for updates in the render loop
App::new()
    .on_update(move || {
        while let Ok(msg) = rx.try_recv() {
            data.set(msg);
        }
    })
    .run(view);
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

When a signal is read inside a `Computed` or `Effect`, the runtime automatically registers the dependency:

```rust
let a = create_signal(1);
let b = create_signal(2);

// This computed depends on both `a` and `b`
let sum = create_computed(move || a.get() + b.get());
```

Changing either `a` or `b` will cause `sum` to recompute.

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

### Use Computed for Derived State

Instead of manually syncing values:

```rust
// Bad: Manual sync
let count = create_signal(0);
let doubled = create_signal(0);
// Must remember to update doubled when count changes

// Good: Use computed
let count = create_signal(0);
let doubled = create_computed(move || count.get() * 2);
```

### Batch Updates

When updating multiple related signals, the render will naturally batch:

```rust
// Both updates happen before the next render
first_name.set("John");
last_name.set("Doe");
```

## API Reference

### Signal Creation

```rust
pub fn create_signal<T: Clone + 'static>(value: T) -> Signal<T>;
pub fn create_computed<T: Clone + 'static>(f: impl Fn() -> T + 'static) -> Computed<T>;
pub fn create_effect(f: impl Fn() + 'static);
```

### Signal Methods

```rust
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T;
    pub fn get_untracked(&self) -> T;  // Read without tracking
    pub fn set(&self, value: T);
    pub fn update(&self, f: impl FnOnce(&mut T));
}
```

### Computed Methods

```rust
impl<T: Clone> Computed<T> {
    pub fn get(&self) -> T;
}
```
