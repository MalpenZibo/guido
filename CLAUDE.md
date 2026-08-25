# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Guido is a reactive Rust GUI library using wgpu for rendering Wayland layer shell widgets (status bars, panels, etc.). The library emphasizes composition from minimal primitives, reactive properties, and GPU-accelerated rendering with animations.

**Note: Backward compatibility is NOT a concern for this project.** Feel free to remove legacy code, refactor APIs, and make breaking changes when it improves the codebase. The library is under active development and not yet stable.

## Documentation

### Developer Reference (`docs/`)

Quick-reference documentation for developers:

- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System design, module structure, and code organization
- **[docs/STATE_LAYER.md](docs/STATE_LAYER.md)** - Hover/pressed state overrides, ripple effects, animations
- **[docs/TRANSFORMS.md](docs/TRANSFORMS.md)** - Translate, rotate, scale with transform origins and animations
- **[docs/REACTIVE.md](docs/REACTIVE.md)** - Signals, computed values, effects, and reactive patterns
- **[docs/STYLING.md](docs/STYLING.md)** - Colors, gradients, borders, corners, shadows, and layout

Read these docs before making significant changes to understand existing patterns.

### User Documentation (`book/`)

The `book/` directory contains an mdbook-based documentation website with tutorials, guides, and screenshots.

```bash
# Build the book
mdbook build book

# Serve locally with live reload
mdbook serve book
```

**IMPORTANT: Keep the book updated when making changes.**

When adding new features or changing APIs:
1. Update relevant chapters in `book/src/`
2. Add new screenshots if the feature has visual components (use `grim` to capture)
3. Build and verify the book renders correctly: `mdbook build book`

Key sections to update based on change type:
- **New widget methods** → `book/src/concepts/container.md` or relevant chapter
- **New styling options** → `book/src/building-ui/`
- **New state layer features** → `book/src/interactivity/`
- **New animation options** → `book/src/animations/`
- **New transform features** → `book/src/transforms/`
- **API changes** → Update all affected chapters and code examples

## Build and Development Commands

```bash
# Build the project
cargo build

# Run an example (status bar on Wayland layer shell)
cargo run --example status_bar

# Run the reactive example (demonstrates signals and events)
cargo run --example reactive_example

# Check for errors without building
cargo check

# Format code
cargo fmt

# Lint with clippy
cargo clippy

# Run tests
cargo test
```

### Cargo Features

- `svg` (default) — SVG image support via resvg. Without it, `ImageSource::Svg*` sources fail to decode with a logged warning.
- `webp` (default) — WebP raster decoding. PNG and JPEG are always available.
- `gif` (opt-in) — GIF raster decoding.
- `render-stats` (opt-in) — per-second rendering statistics on stdout; zero overhead when disabled.

## Architecture

### Core Modules

**`reactive/`** - Single-threaded reactive system inspired by SolidJS
- `RwSignal<T>`: Read-write reactive signal (8 bytes). Created via `create_signal` (requires Clone+PartialEq+Send). Has `.get()`, `.set()`, `.update()`, `.writer()`. Converts to `Signal<T>` via `.read_only()` or `.into()`
- `Signal<T>`: Read-only reactive signal (12 bytes). Created via `create_stored` (static, requires Clone) or `create_derived` (closure-backed). Has `.get()`, `.with()` — no mutation methods. Widget props accept `Signal<T>` via `IntoSignal<T, M>` (marker-type disambiguation — integers accepted where `f32`/`Length`/`Padding` expected)
- `Memo<T>`: Eager computed values that recompute when dependencies change, only notify on actual changes (`PartialEq`)
- `create_effect`: side effects that re-run when tracked signals change. Returns nothing — an effect's lifetime is its scope's
- `create_task` / `create_service`: async background work, aborted with the scope. `create_task` for push-only, `create_service` when the UI sends commands
- `Owner`: Ownership system for automatic resource cleanup (signals, effects, custom callbacks)
- Runtime uses thread-local storage for automatic dependency tracking on the main thread
- Container paint/layout auto-tracks signal reads via `with_signal_tracking()` — closures work as reactive properties
- Background threads update signals via `WriteSignal` (queued writes, flushed each frame)

**`widgets/`** - Composable UI primitives implementing the `Widget` trait
- `Container`: Handles padding, background colors, gradients, borders, corner radius, and event handlers (click, hover, scroll)
- **Reactivity rule**: everything that survives to paint takes `impl IntoSignal<T, M>` (background, gradient, backdrop_blur, overflow, corners, border, translate, rotate, scale, pivot, …); structural declarations do not (`layout`, `scrollable`, `scrollbar`, `scrollbar_visibility`, `control`, `animate_*`)
- `Flex` / `ZStack`: layouts plugged into a container via `.layout(Flex::row())`; the `Layout` trait is public, so an app can write its own
- `Text`: Text rendering with reactive content and styling
- `AnyWidget`: Type alias for `Box<dyn Widget>` with `Widget::into_any()` for type erasure
- All widget properties can be static values or reactive (via `IntoSignal` trait)
- `#[component]` macro: Creates reusable widgets from functions with reactive props, callbacks, children, and slots

**`renderer/`** - GPU rendering using wgpu
- SDF-based shape rendering with custom shader pipeline
- Supports rounded rectangles with superellipse corners (CSS K-value system)
- SDF-based border rendering for crisp anti-aliased borders with uniform width
- Supports circles, gradients, and clipping
- Text rendering via glyphon library
- HiDPI-aware with automatic scaling
- Layered rendering: shapes → images → text → overlay, per draw group; a group is
  split whenever the tree paints a lower layer over a higher one, so batching
  never reorders drawing

**`platform/`** - Wayland layer shell integration
- Uses smithay-client-toolkit for Wayland protocol handling
- Supports layer shell positioning (top, bottom, overlay) and anchoring
- Keyboard interactivity modes (None, OnDemand, Exclusive)
- Event loop integration via calloop
- Mouse, scroll, and keyboard event handling

**`surface.rs`** - Multi-surface management
- `SurfaceConfig`: Configuration for surfaces (size, anchor, layer, keyboard mode)
- `SurfaceHandle`: Control handle for modifying surface properties at runtime
- `spawn_surface()`: Create surfaces dynamically
- `surface_handle()`: Get a handle for any surface by ID

**`layout/`** - Constraint-based layout system
- `Constraints`: min/max width/height bounds for sizing
- `Size`: layout results
- `Flex` (row/column) and `ZStack` layout implementations

### Reactive System Details

The reactive system allows widget properties to be either static or dynamic:

```rust
// Static value
container().background(Color::rgb(0.2, 0.2, 0.3))

// Reactive signal (RwSignal auto-converts to Signal via IntoSignal)
let color = create_signal(Color::rgb(0.2, 0.2, 0.3));
container().background(color)

// Reactive closure
container().background(move || {
    if condition.get() {
        Color::RED
    } else {
        Color::BLUE
    }
})
```

Both `Signal<T>` and `RwSignal<T>` are main-thread only (`!Send`). Background threads use `rw_signal.writer()` to get a `WriteSignal<T>` that queues writes for the next frame. The main render loop re-layouts and re-paints each frame, reading current signal values.

### Widget Trait

Two methods are required, the rest have defaults:
- `layout(&mut self, tree, id, constraints) -> Size`: measure, then `tree.cache_layout(..)`
- `paint(&self, tree, id, ctx)`: draw into the PaintContext
- `event(&mut self, tree, id, event) -> EventResponse`: handle input (defaults to `Ignored`)
- `advance_animations`, `reconcile_children`, `layout_hints`, `register_children`: defaults

Position and bounds live in the `Tree`, not on the widget. Writing one from
outside the crate needs `guido::widget_prelude::*` alongside the ordinary
prelude — see `tests/external_widget.rs`.

### Rendering Pipeline

Rendering is paced by the compositor's `wl_surface.frame` callbacks: an
animating surface renders once per callback, an idle surface renders nothing.

**Main loop (once per iteration):**
1. `flush_bg_writes()` - Drain queued background-thread signal writes
2. `take_frame_request()` - Check if a frame was requested

**Per-surface rendering:**
3. Dispatch events to widgets (queued `MouseMove`s are coalesced to the latest position)
4. **Frame-pacing gate**: if the previous frame's callback hasn't fired yet, return
   before draining jobs (animation continuations stay queued; init/resizes bypass)
5. `drain_pending_jobs()` + `process_jobs()` - Unregister → advance animations → reconcile dynamic children → mark paint/layout dirty flags
6. Partial layout from `layout_roots` - Only dirty subtrees re-layout
7. **Skip frame** if root widget doesn't need paint
8. `widget.paint(tree, ctx)` - Build render tree (clean children reuse Rc-shared cached `RenderNode`s — reuse is a refcount bump, not a clone)
9. `flatten_root_into()` - Flatten render tree to draw commands (incremental: clean subtrees reuse cached commands)
10. Re-arm the frame callback and report per-surface damage via `wl_surface.damage_buffer()` — both BEFORE presenting so they ride the commit performed inside `present()`
11. GPU rendering with instanced SDF shapes, HiDPI scaling, and per-group layer ordering (shapes → images → text → overlay, one group per draw-order regression); on a lost/outdated swapchain the dirty state is kept and a retry frame is requested
12. `cache_paint_results()` - Rc-share paint output per widget, clear `needs_paint` flags (partial paints poison their ancestors and are never cached)

### Event System

Events flow from Wayland → platform layer → widgets:
- `MouseMove`, `MouseEnter`, `MouseLeave`: Cursor tracking
- `MouseDown`, `MouseUp`: Button clicks with coordinates
- `Scroll`: Wheel or touchpad scrolling with delta values

Containers provide callback builders (`.on_click()`, `.on_hover()`, `.on_scroll()`) that widgets can use to respond to events. `on_click` accepts a closure, a `Callback`, or the `Option<Callback>` a `#[component]` prop holds.

### Preludes

- `guido::prelude` — applications
- `guido::widget_prelude` — implementing `Widget` or `Layout` (`Tree`, `WidgetId`, `Constraints`, `PaintContext`, `RenderNode`, `LayoutHints`, `with_signal_tracking`, `JobType`). Import alongside the ordinary prelude; see `tests/external_widget.rs` and `book/src/advanced/custom-widgets.md`

## Important Patterns

### State Layer API

Use the state layer API for hover and pressed visual feedback:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corners(8.0)
    .when_hovered(|s| s.lighter(0.1))      // Lighten on hover
    .when_pressed(|s| s.ripple())         // Ripple on press
    .on_click(move || count.update(|c| *c += 1))
    .child(text("Click me"))
```

See [docs/STATE_LAYER.md](docs/STATE_LAYER.md) for full documentation.

### Creating Reactive UIs

```rust
let count = create_signal(0);
let view = container()
    .layout(Flex::row().spacing(8.0))
    .children([
        text(move || format!("Count: {}", count.get())),
        container()
            .background(Color::rgb(0.3, 0.3, 0.4))
            .when_hovered(|s| s.lighter(0.1))
            .when_pressed(|s| s.ripple())
            .on_click(move || count.update(|c| *c += 1))
            .child(text("Click me"))
    ]);
```

### App Configuration

```rust
App::new().run(|app| {
    let count = create_signal(0);
    let view = container()
        .layout(Flex::row().spacing(8.0))
        .children([
            text(move || format!("Count: {}", count.get())),
            container()
                .background(Color::rgb(0.3, 0.3, 0.4))
                .on_click(move || count.update(|c| *c += 1))
                .child(text("Click me"))
        ]);

    app.add_surface(
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .layer(Layer::Top)
            .keyboard_interactivity(KeyboardInteractivity::OnDemand)
            .namespace("my-app")
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || view,
    );
});
```

### Dynamic Surface Properties

Modify surface properties at runtime via `SurfaceHandle`:

```rust
// Get handle for a surface added via add_surface()
let handle = surface_handle(surface_id);
handle.set_layer(Layer::Overlay);
handle.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
```

### Integrating Background Tasks

Use `create_task` for async background work that only pushes, `create_service` when the UI also sends commands. Both are cleaned up automatically. Use `.writer()` to get a `WriteSignal<T>` that is `Send` and can be moved into the async task:

```rust
let data = create_signal(String::new());
let data_w = data.writer();  // WriteSignal<T> — Send, for background tasks

// Push-only: no command type, no receiver
create_task(move |ctx| async move {
    while ctx.is_running() {
        data_w.set(fetch_data());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});

// Bidirectional service (with commands)
let status = create_signal(String::new());
let status_w = status.writer();
let service = create_service(move |mut rx, ctx| async move {
    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                status_w.set(format!("Processing: {:?}", cmd));
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if !ctx.is_running() { break; }
            }
        }
    }
});
service.send(MyCommand::DoSomething);
```

### A reactive property has three spellings, and they must agree

`IntoSignal` accepts a value, a closure, or a signal. A property is only
properly reactive if the *same expression* compiles in all three positions:

```rust
container().width(100.0)             // value
container().width(move || w.get())   // closure
container().width(w)                 // signal
```

The three are served by three different sets of impls, and they can drift apart
silently — nothing fails to build when one is missing, it just refuses at some
call site months later. Every property with a conversion had exactly this hole
until #225: `.width(signal_of_f32)`, `.padding(..)`, `.corners(..)`,
`.backdrop_blur(..)` and `Flex::spacing(..)` all refused a signal of the type
their own value form converts from.

**When adding a conversion to a property type, add all three:**

| spelling | impl to write | where it lives |
|---|---|---|
| value | `From<S> for T` | beside `T` |
| closure | `IntoVal<T> for S` | beside `T` |
| signal | `converting_signals!(S => T)` | beside `T` |

The lists must match one for one. They cannot be collapsed into a single
blanket impl: `IntoVal` is reflexive, so a blanket `S: IntoVal<T>` also covers
`Signal<T> -> T`, collides with the passthrough impl and leaves the marker
generic undecidable — it stops existing call sites compiling. Excluding the
reflexive case needs negative bounds or specialisation, neither stable.

So this stays a hand-kept list until something generates all three from one
declaration. **Until then, treat "did I add the signal form too?" as part of
adding any conversion**, and add a spelling to `tests/signal_conversions.rs`,
which is the only thing standing between the lists and drift.

Tracked in #226.

### Corners

Radius and curvature are one property: a bare size means rounded corners and
takes what `padding` takes, a constructor names another shape.

```rust
container().corners(12.0)                        // circular, all four
container().corners([16.0, 0.0])                 // rounded on top, square below
container().corners(Corners::squircle(12.0))     // K=2, iOS-style
container().corners(Corners::bevel(12.0))        // K=0, diagonal cut
container().corners(Corners::scoop(12.0))        // K=-1, concave
container().corners(Corners::superellipse(12.0, 1.5))
```

See [docs/STYLING.md](docs/STYLING.md) for full styling reference.

## Development Workflow

### Architectural changes are agreed before they are written

**Before introducing a new architectural structure, explain why it is needed
and get the design validated. Do not implement first and present after.**

This covers anything other code will then be written against: a new core type
or trait, a new cross-cutting mechanism, a new ownership or lifetime rule, a
new registry, or a change to how a whole family of call sites is spelled.
Ordinary work does not — fixing a bug at its definition, adding a widget
method, following a pattern that already exists.

"Explain why" means: the problem as it stands in the code, the sites that have
it, why the existing pieces cannot answer it, the alternatives weighed, and a
measurement wherever performance is claimed. The decision is the user's; the
implementation starts after they have made it.

### Git Workflow

**IMPORTANT: Never commit directly to the main branch.**

- Always create a feature branch for any changes
- Open a Pull Request (PR) for review
- **CRITICAL: Check that all CI checks pass before merging the PR**
- Merge to main only through PRs after CI is green

**Render-tree snapshots.** `tests/render_snapshots.rs` lays out and paints widget
trees taken from `examples/` (no compositor, no GPU) and diffs the render tree
against golden files in `tests/snapshots/`. A change in geometry or in what gets
drawn shows up there without anyone having had to predict the assertion. When a
change is intended, re-bless and **read the diff** — it is the review:

```bash
UPDATE_SNAPSHOTS=1 cargo test --test render_snapshots
```

**CRITICAL: Always run `cargo clippy` and `cargo fmt` before committing code changes.**
- Fix all clippy errors (compilation will fail)
- Address clippy warnings when reasonable
- Use `cargo clippy --fix --allow-dirty` to auto-fix simple warnings
- Run `cargo fmt --all` to ensure proper formatting

**IMPORTANT: Use atomic commits.**
- Each commit should be a single, focused change that can be reviewed and reverted independently
- Separate new features from refactoring or bug fixes
- When adding a new feature, commit in logical increments (e.g., data structures first, then rendering, then widget API)
- This makes it easier to identify and revert regressions without losing unrelated work
- Run and verify examples/tests after each commit to catch issues early

```bash
# Create a feature branch
git checkout -b feature/my-feature

# Make changes, then run formatting and clippy BEFORE committing
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --fix --allow-dirty  # Auto-fix warnings if needed

# Then commit
git add .
git commit -m "Add my feature"

# Push and create PR
git push -u origin feature/my-feature
gh pr create --title "Add my feature" --body "Description of changes"

# IMPORTANT: Check CI status before merging
gh pr view --web  # Check that all CI checks pass (Format, Clippy, Test, Build)

# Only merge after all CI checks are green
gh pr merge <pr-number> --squash --delete-branch
```

### Visual Changes

When making visual changes to the renderer:
- Always take screenshots to verify rendering results
- Do not ask for permission when taking screenshots - just take them to check the result
- Use `grim` for taking screenshots on Wayland

## Project Status

This is a work-in-progress GUI library. Current implemented features:
- Reactive widget system with signals, computed values, and effects
- Unified Container widget with pluggable Flex layout
- State layer API for hover/pressed styles with ripple effects
- Transform system (translate, rotate, scale) with animations
- SDF-based rendering with superellipse corners and crisp borders
- Mouse event handling with proper transform hit testing
- Multi-surface support with shared reactive state
- Dynamic surface property modification (layer, keyboard interactivity, anchor, size, margins)
- Multi-output support: reactive `outputs()` enumeration, per-output surface pinning, `surface_output()` tracking
- Input regions: per-surface clickable rectangles / click-through surfaces (`input_region`, `click_through`, `set_input_region`)
- Backdrop blur via `container().backdrop_blur(radius)`: filters both the
  surface's own drawn content (offscreen target, downsample + separable
  gaussian, masked to the container's rounded shape) and the compositor's
  backdrop (`ext-background-effect-v1`, shaped by bounds + corner radius).
  Restrict with `BackdropSources`; check availability with `compositor_effects()`
- Session lock (`ext-session-lock-v1`): `lock_session(factory)` / `unlock_session()` / reactive `lock_state()`, one lock surface per output with hotplug handling
- Touch input (`wl_touch`): first finger drives the pointer pipeline — tap = click, works with hover/pressed state layers
- Clipboard: async prefetch (paste never blocks the UI thread) + primary selection (select-to-copy, middle-click paste)
- xdg popups (`spawn_popup`): compositor-positioned menus anchored to a surface, grab semantics (outside click dismisses, reactive `dismissed()`), nested popups
- Text input widget with full editing support
- Image widget with raster and SVG support

Planned features (see TODO.md):
- Additional widget types (toggle, checkbox)
