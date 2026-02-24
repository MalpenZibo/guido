# Context API: `provide_context` / `use_context`

## Motivation

After porting ashell's config system, we hit three pain points:

1. **Theme colors changed from `const` to `fn()`** — required updating 17 files (`theme::TEXT` → `theme::TEXT()`)
2. **Config structs cloned everywhere** — closures need `'static`, so `cfg.system_info.clone()` proliferates through main.rs
3. **Hand-rolled thread-local Cell** — we built our own `thread_local! { static COLORS: Cell<ThemeColors> }` in the theme module

All of these stem from guido lacking a way to store app-wide state that any widget can access without explicit parameter passing. Adding a **context system** (like React/Leptos) solves all three.

## Design

A thread-local `Vec<(TypeId, Box<dyn Any>)>` that stores one value per type with linear scan. At ~3-8 entries (config, theme, services), this fits in 1-2 cache lines and avoids HashMap overhead. `TypeId` comparison is a single `u64` eq. Scoped to the app lifetime — `App::drop()` resets it via `reset_contexts()`.

## API

```rust
// Core
pub fn provide_context<T: 'static>(value: T);
pub fn use_context<T: Clone + 'static>() -> Option<T>;
pub fn expect_context<T: Clone + 'static>() -> T;

// Zero-clone borrow
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R>;

// Existence check
pub fn has_context<T: 'static>() -> bool;

// Signal helper (create + provide in one step)
pub fn provide_signal_context<T: Clone + PartialEq + Send + 'static>(value: T) -> Signal<T>;

// Internal cleanup
pub(crate) fn reset_contexts();
```

## Implementation

### `src/reactive/context.rs`

Thread-local Vec storage with:
- `provide_context`: replaces existing entry if same TypeId exists, otherwise pushes
- `use_context`: linear scan + downcast + clone
- `expect_context`: delegates to `use_context` with `type_name::<T>()` in panic message
- `with_context`: linear scan + downcast + borrow (no clone)
- `has_context`: linear scan, returns bool
- `provide_signal_context`: `create_signal(value)` + `provide_context(signal)` + return signal
- `reset_contexts`: `Vec::clear()`

### `src/reactive/mod.rs`

- `pub mod context;`
- Re-exports: `provide_context`, `use_context`, `expect_context`, `with_context`, `has_context`, `provide_signal_context`
- `reset_reactive()` calls `context::reset_contexts()`

### `src/lib.rs` (prelude)

All public context functions exported.

## Usage

### Static config

```rust
App::new().run(|app| {
    provide_context(Config::load());
    // ...
});

// In any widget module:
let cfg = expect_context::<Config>();
```

### Reactive context (recommended for mutable state)

```rust
// Setup: create signal + provide as context in one step
let theme = provide_signal_context(Theme::default());

// Any widget: retrieve signal, auto-tracks during paint/layout
let theme = expect_context::<Signal<Theme>>();
container().background(move || theme.get().bg_color)
```

### Zero-clone access

```rust
// Borrow without cloning — good for large config structs
let threshold = with_context::<Config, _>(|cfg| cfg.cpu.warn_threshold);
```

## Design Decisions

### Why global instead of owner-scoped?

Layer shell widgets (status bars, panels) are 3-5 levels deep, not 20. Tree scoping adds complexity for no benefit. Owner-scoped context can be added later without breaking the global API.

### Why Vec instead of HashMap?

Context stores ~3-8 values. At this scale, `Vec` with linear scan beats `HashMap`: fits in 1-2 cache lines, no hash computation, no bucket overhead. Matches the `storage.rs` pattern.

### Why keep theme as separate Cell?

Theme is read 100s of times per frame. `Cell` memcpy beats Vec scan + RefCell borrow + downcast. Correct separation of concerns.

## Verification

1. `cargo build` in guido
2. `cargo test` in guido (unit tests for all 6 public functions + reset)
3. `cargo clippy --all-targets --all-features -- -D warnings` — clean
4. `cargo fmt --all` — formatted
5. `cargo build` in ashell-guido
6. Run ashell-guido — config values should take effect
