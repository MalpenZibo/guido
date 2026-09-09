# Context

Context shares state across widgets without passing it through every level of
the tree: a value is declared for a **scope**, and every widget built below that
scope can read it back by type.

## When to Use Context

Use context for **cross-cutting concerns** that many widgets need:

- Application configuration
- Theme or styling data
- Service handles (loggers, API clients)
- User preferences

For state that only a few nearby widgets share, passing signals directly is
simpler and preferred.

## Scopes

A scope is a lifetime, and guido opens one wherever it invokes a closure of
yours: the `App::run` setup, each surface, each popup, and each dynamic children
factory. A value declared with `provide_context` belongs to the scope that is
current when the call runs, and a read walks from the reader's scope upward and
takes the nearest declaration.

Three things follow, and they are the whole model:

- **An inner declaration shadows an outer one** for its own scope only. A popup
  that declares its own `RwSignal<Theme>` reads that one rather than the
  application's, and leaves the application's alone for everything else.
- **A declaration dies with its scope.** When the popup closes, what it declared
  goes with it; nothing is left behind for a later read to find.
- **Declaring the same type twice in one scope panics.** Two of a type is what
  shadowing is for, and shadowing needs two scopes — a second declaration in one
  scope would replace the first with nothing to say it had.

The scopes are one level deep. A surface, a popup and a surface spawned at
runtime are all children of the root — the loop opens each one's scope from the
root, not from the surface it belongs to — so a popup reads what the
application declared and not what the surface that opened it did, and two
surfaces are siblings that cannot see each other's declarations.

A plain widget factory is an ordinary function call, not a scope: a
`provide_context` written inside one declares its value for whatever scope
called it — the whole surface — and not for the subtree the factory is
building. A closure guido *invokes* is a scope, and that is the difference: the
factory you hand to `.child(…)` for dynamic children is called by the library,
which opens a scope around it, so a declaration made there belongs to that row.

## Where a Read Resolves

In the body of the factory that builds the widgets. That is where the scope the
value was declared for is the one that is current, so it is where guido knows
who is asking.

A **property closure** resolves where it was written, whenever it is read. The
closure you hand a builder becomes a derived signal, and the scope that was
current when the builder ran travels with it — so a property declared in a row
of a dynamic list reads that row's declarations on every phase of every frame,
and not the row's while laying out and the application's while painting.

An **event handler** and a **spawned task** open no scope and carry none, so a
read inside one resolves against whatever is current, which in a running
application is the root: `App::run` enters the root scope before your setup
closure and never leaves it. The application's declarations are readable from a
handler; a surface's are not.

Reading the value once in the factory body and capturing it works everywhere,
and is still the clearest thing to write:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, PartialEq, Default)]
# struct Theme { bg_color: Color, title: String }
# fn main() {
fn themed_box() -> Container {
    let theme = expect_context::<RwSignal<Theme>>();   // here: a scope is current

    container()
        .background(move || theme.get().bg_color)      // or here: the closure keeps this scope
        .child(text(move || theme.get().title.clone()))
}
# }
```

## Providing Context

Call `provide_context` in your `App::run()` setup to make a value available to
every surface:

```rust,no_run
# extern crate guido;
# use guido::prelude::*;
# fn build_ui() -> Container { container() }
# #[derive(Clone, Default)]
# struct Config { warn_threshold: f64 }
# impl Config { fn load() -> Self { Self::default() } }
# fn main() {
# let config = SurfaceConfig::new();
App::new().run(|app| {
    provide_context(Config::load());

    app.add_surface(config, || build_ui());
});
# ;
# }
```

## Retrieving Context

### use_context (fallible)

Returns `Option<T>` — `None` when no scope at or above this one declared it:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Default)]
# struct Config { warn_threshold: f64 }
# fn main() {
if let Some(cfg) = use_context::<Config>() {
    println!("threshold: {}", cfg.warn_threshold);
}
# ;
# }
```

### expect_context (infallible)

Panics if nothing declared it, naming the type and both of the reasons it can be
missing:

```rust,no_run
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Default)]
# struct Config { warn_threshold: f64 }
# fn main() {
let cfg = expect_context::<Config>();
# ;
# }
```

### with_context (zero-clone)

Borrows the value without cloning — ideal for large structs when you only need
one field:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Default)]
# struct Config { warn_threshold: f64 }
# fn main() {
let threshold = with_context::<Config, _>(|cfg| cfg.warn_threshold);
# ;
# }
```

### has_context (existence check)

Check whether a type is declared without retrieving it:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Default)]
# struct Logger;
# impl Logger { fn info(&self, _: &str) {} }
# fn main() {
if has_context::<Logger>() {
    expect_context::<Logger>().info("ready");
}
# ;
# }
```

## Reactive Context

For mutable shared state, declare an `RwSignal<T>`. Any widget reading the
signal during paint or layout tracks it, so a write anywhere repaints everything
that reads it.

### provide_signal_context

Creates an `RwSignal` and declares it in one step:

```rust,no_run
# extern crate guido;
# use guido::prelude::*;
# fn build_ui() -> Container { container() }
# #[derive(Clone, PartialEq, Default)]
# struct Theme { bg_color: Color, title: String }
# fn main() {
# let config = SurfaceConfig::new();
App::new().run(|app| {
    // Creates RwSignal<Theme> and declares it for the root scope
    let theme = provide_signal_context(Theme::default());

    app.add_surface(config, || build_ui());
});
# ;
# }
```

What it declares is an `RwSignal<T>`, and that is the type to ask for. Asking
for a `Signal<T>` finds nothing: a different type is a different key.

## Combining with SignalFields

For config structs with many fields, use `#[derive(SignalFields)]` with context
so each widget only repaints when the specific field it reads changes:

```rust,ignore
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

Since `Signal<T>` is `Copy`, passing them directly is zero-cost. A context read
scans the declarations of the current scope and then of each scope above it,
which is negligible but unnecessary when only a few widgets need the value.

## API Reference

```text
// Declare a value for the current scope
pub fn provide_context<T: 'static>(value: T);

// Retrieve (clones)
pub fn use_context<T: Clone + 'static>() -> Option<T>;
pub fn expect_context<T: Clone + 'static>() -> T;

// Borrow without cloning
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R>;

// Existence check
pub fn has_context<T: 'static>() -> bool;

// Create signal + declare it
pub fn provide_signal_context<T: Clone + PartialEq + Send + 'static>(
    value: T
) -> RwSignal<T>;
```
