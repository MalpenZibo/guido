# Context

Context provides a way to share app-wide state (config, theme, services) across widgets without passing values through every level of the widget tree.

## When to Use Context

Use context for **cross-cutting concerns** that many widgets need:

- Application configuration
- Theme or styling data
- Service handles (loggers, API clients)
- User preferences

For state that only a few nearby widgets share, passing signals directly is simpler and preferred.

## Providing Context

Call `provide_context` in your `App::run()` setup to make a value available everywhere:

```rust
use guido::prelude::*;

App::new().run(|app| {
    provide_context(Config::load());

    app.add_surface(config, || build_ui());
});
```

## Retrieving Context

### use_context (fallible)

Returns `Option<T>` — useful when the context is optional:

```rust
if let Some(cfg) = use_context::<Config>() {
    println!("threshold: {}", cfg.warn_threshold);
}
```

### expect_context (infallible)

Panics with a helpful message if the context was not provided:

```rust
let cfg = expect_context::<Config>();
```

### with_context (zero-clone)

Borrows the value without cloning — ideal for large structs when you only need one field:

```rust
let threshold = with_context::<Config, _>(|cfg| cfg.cpu.warn_threshold);
```

### has_context (existence check)

Check if a context has been provided without retrieving it:

```rust
if has_context::<Logger>() {
    expect_context::<Logger>().info("ready");
}
```

## Reactive Context

For mutable shared state, store a `Signal<T>` as context. This is the most powerful pattern — any widget reading the signal during paint/layout auto-tracks it for reactive updates.

### provide_signal_context

Creates a signal and provides it as context in one step:

```rust
App::new().run(|app| {
    // Creates Signal<Theme> and stores it as context
    let theme = provide_signal_context(Theme::default());

    app.add_surface(config, || build_ui());
});
```

### Reading a signal context

```rust
fn themed_box() -> Container {
    let theme = expect_context::<Signal<Theme>>();

    container()
        .background(move || theme.get().bg_color)
        .child(text(move || theme.get().title.clone()))
}
```

When the signal is updated anywhere, all widgets reading it automatically repaint.

## Combining with SignalFields

For config structs with many fields, use `#[derive(SignalFields)]` with context so each widget only repaints when the specific field it reads changes:

```rust
#[derive(Clone, PartialEq, SignalFields)]
pub struct AppConfig {
    pub cpu_warn: f64,
    pub mem_warn: f64,
    pub title: String,
}

App::new().run(|app| {
    let config = AppConfigSignals::new(AppConfig {
        cpu_warn: 80.0,
        mem_warn: 90.0,
        title: "My App".into(),
    });
    provide_context(config);

    app.add_surface(surface_config, || build_ui());
});

// In a widget — only repaints when cpu_warn changes
fn cpu_indicator() -> Container {
    let config = expect_context::<AppConfigSignals>();
    let threshold = config.cpu_warn;  // Signal<f64>

    container()
        .background(move || {
            if current_cpu() > threshold.get() {
                Color::RED
            } else {
                Color::GREEN
            }
        })
}
```

## Context vs Passing Signals

| Approach | Best for |
|----------|----------|
| **Pass signals directly** | Parent-child, 1-2 levels deep, few consumers |
| **Context** | App-wide state, many consumers across modules |

Since `Signal<T>` is `Copy`, passing them directly is zero-cost. Context adds a Vec scan + downcast, which is negligible but unnecessary when only a few widgets need the value.

## API Reference

```rust
// Store a value (one per type, replaces if exists)
pub fn provide_context<T: 'static>(value: T);

// Retrieve (clones)
pub fn use_context<T: Clone + 'static>() -> Option<T>;
pub fn expect_context<T: Clone + 'static>() -> T;

// Borrow without cloning
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R>;

// Existence check
pub fn has_context<T: 'static>() -> bool;

// Create signal + provide as context
pub fn provide_signal_context<T: Clone + PartialEq + Send + 'static>(
    value: T
) -> Signal<T>;
```
