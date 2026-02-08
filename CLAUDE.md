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

## Architecture

### Core Modules

**`reactive/`** - Thread-safe reactive system inspired by SolidJS
- `Signal<T>`: Thread-safe reactive values with automatic dependency tracking
- `MaybeDyn<T>`: Enum allowing widget properties to accept either static values or reactive signals/closures
- `Memo<T>`: Eager computed values that recompute when dependencies change, only notify on actual changes (`PartialEq`)
- `Effect`: Side effects that re-run when tracked signals change
- `Owner`: Ownership system for automatic resource cleanup (signals, effects, custom callbacks)
- Runtime uses thread-local storage for automatic dependency tracking on the main thread
- Container paint/layout auto-tracks signal reads via `with_signal_tracking()` — closures work as reactive properties
- Background threads can update signal values; effects only run on main thread

**`widgets/`** - Composable UI primitives implementing the `Widget` trait
- `Container`: Handles padding, background colors, gradients, borders, corner radius, and event handlers (click, hover, scroll)
- `Row` / `Column`: Flexbox-style layouts with alignment and spacing
- `Text`: Text rendering with reactive content and styling
- All widget properties can be static values or reactive (via `IntoMaybeDyn` trait)

**`renderer/`** - GPU rendering using wgpu
- SDF-based shape rendering with custom shader pipeline
- Supports rounded rectangles with superellipse corners (CSS K-value system)
- SDF-based border rendering for crisp anti-aliased borders with uniform width
- Supports circles, gradients, and clipping
- Text rendering via glyphon library
- HiDPI-aware with automatic scaling
- Layered rendering: shapes → text → overlay shapes (for effects like ripples)

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
- Flexbox layout logic for Row/Column widgets

### Reactive System Details

The reactive system allows widget properties to be either static or dynamic:

```rust
// Static value
container().background(Color::rgb(0.2, 0.2, 0.3))

// Reactive signal
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

Signals are thread-safe and can be updated from background threads. The main render loop re-layouts and re-paints each frame, reading current signal values.

### Widget Trait

All widgets implement:
- `layout(constraints) -> Size`: Calculate size given constraints
- `paint(ctx)`: Draw to the PaintContext
- `event(event) -> EventResponse`: Handle input events
- `set_origin(x, y)`: Position the widget
- `bounds() -> Rect`: Get bounding box for hit testing

### Rendering Pipeline

Each frame follows this pipeline:
1. `drain_pending_jobs()` - Collect reactive jobs (paint, layout, reconcile, unregister, animation)
2. Process jobs: unregister dropped widgets, mark paint/layout dirty flags, reconcile dynamic children
3. Partial layout from `layout_roots` - Only dirty subtrees re-layout
4. **Skip frame** if root widget doesn't need paint (animations still advance)
5. `widget.paint(tree, ctx)` - Build render tree (clean children reuse cached `RenderNode`s)
6. `cache_paint_results()` - Store paint output per widget, clear `needs_paint` flags
7. `flatten_tree_into()` - Flatten render tree to draw commands (incremental: clean subtrees reuse cached commands)
8. GPU rendering with instanced SDF shapes, HiDPI scaling, layer ordering (shapes → images → text → overlay)
9. Report damage region to Wayland via `wl_surface.damage_buffer()`
10. `handle_animation_jobs()` - Advance animations for next frame
11. `wl_surface.commit()` - Present frame

### Event System

Events flow from Wayland → platform layer → widgets:
- `MouseMove`, `MouseEnter`, `MouseLeave`: Cursor tracking
- `MouseDown`, `MouseUp`: Button clicks with coordinates
- `Scroll`: Wheel or touchpad scrolling with delta values

Containers provide callback builders (`.on_click()`, `.on_hover()`, `.on_scroll()`) that widgets can use to respond to events.

## Important Patterns

### State Layer API

Use the state layer API for hover and pressed visual feedback:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(8.0)
    .hover_state(|s| s.lighter(0.1))      // Lighten on hover
    .pressed_state(|s| s.ripple())         // Ripple on press
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
            .hover_state(|s| s.lighter(0.1))
            .pressed_state(|s| s.ripple())
            .on_click(move || count.update(|c| *c += 1))
            .child(text("Click me"))
    ]);
```

### App Configuration

```rust
let (app, surface_id) = App::new().add_surface(
    SurfaceConfig::new()
        .height(32)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .layer(Layer::Top)
        .keyboard_interactivity(KeyboardInteractivity::OnDemand)
        .namespace("my-app")
        .background_color(Color::rgb(0.1, 0.1, 0.15)),
    move || view,
);
app.run();
```

### Dynamic Surface Properties

Modify surface properties at runtime via `SurfaceHandle`:

```rust
// Get handle for a surface added via add_surface()
let handle = surface_handle(surface_id);
handle.set_layer(Layer::Overlay);
handle.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
```

### Integrating Background Threads

Use `create_service` for background tasks that are automatically cleaned up:

```rust
let data = create_signal(String::new());

// Read-only service (ignore receiver)
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        data.set(fetch_data());
        std::thread::sleep(Duration::from_secs(1));
    }
});

// Bidirectional service (with commands)
let service = create_service(move |rx, ctx| {
    while ctx.is_running() {
        while let Ok(cmd) = rx.try_recv() {
            // handle commands
        }
    }
});
service.send(MyCommand::DoSomething);
```

### Corner Curvature

Corner styles use CSS K-values for superellipse rendering:

```rust
container().corner_radius(12.0).squircle()  // K=2, iOS-style smooth
container().corner_radius(12.0)              // K=1, standard circular (default)
container().corner_radius(12.0).bevel()      // K=0, diagonal cut
container().corner_radius(12.0).scoop()      // K=-1, concave inward
container().corner_radius(12.0).corner_curvature(1.5)  // Custom K value
```

See [docs/STYLING.md](docs/STYLING.md) for full styling reference.

## Development Workflow

### Git Workflow

**IMPORTANT: Never commit directly to the main branch.**

- Always create a feature branch for any changes
- Open a Pull Request (PR) for review
- **CRITICAL: Check that all CI checks pass before merging the PR**
- Merge to main only through PRs after CI is green

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
- Text input widget with full editing support
- Image widget with raster and SVG support

Planned features (see TODO.md):
- Additional widget types (toggle, checkbox)
