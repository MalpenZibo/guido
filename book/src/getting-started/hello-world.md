# Hello World

Let's build a simple status bar to understand the basics of Guido. By the end of this tutorial, you'll have a working application like this:

![Status Bar](../images/status_bar.png)

## Creating the Project

Start with a new Rust project:

```bash
cargo new hello-guido
cd hello-guido
cargo add guido
```

## The Complete Code

Replace `src/main.rs` with:

```rust
use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        || {
            container()
                .height(fill())
                .layout(
                    Flex::row()
                        .spacing(8.0)
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.2, 0.2, 0.3))
                        .corner_radius(4.0)
                        .child(text("Guido")),
                )
                .child(container().padding(8.0).child(text("Hello World!")))
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.3, 0.2, 0.2))
                        .corner_radius(4.0)
                        .child(text("Status Bar")),
                )
        },
    );
    app.run();
}
```

Run it with `cargo run`.

## Understanding the Code

Let's break down each part:

### The Prelude

```rust
use guido::prelude::*;
```

The prelude imports everything you need: widgets, colors, layout types, and reactive primitives.

### Surface Configuration

```rust
let (app, _surface_id) = App::new().add_surface(
    SurfaceConfig::new()
        .height(32)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .background_color(Color::rgb(0.1, 0.1, 0.15)),
    || { /* widget tree */ },
);
app.run();
```

The `SurfaceConfig` defines how the window appears:
- **Height** - 32 pixels tall
- **Anchor** - Attached to top, left, and right edges (full width)
- **Background color** - Dark background for the bar

Note: `add_surface()` returns a tuple `(App, SurfaceId)`. The `SurfaceId` can be used later to get a `SurfaceHandle` for modifying surface properties dynamically.

### Building the View

The view is built using **containers** - the primary building block in Guido:

```rust
container()
    .height(fill())
    .layout(
        Flex::row()
            .spacing(8.0)
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Center),
    )
```

This creates a container that:
- Fills the available height with `fill()`
- Uses a horizontal flex layout
- Centers children vertically with `cross_axis_alignment`
- Spreads children across the space with `SpaceBetween`

### Adding Children

Each section of the status bar is a child container:

```rust
.child(
    container()
        .padding(8.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(4.0)
        .child(text("Guido")),
)
```

The container has:
- **Padding** - 8 pixels of space around the content
- **Background** - A dark purple-gray color
- **Corner radius** - Rounded corners
- **Child** - A text widget

### Text Widgets

```rust
text("Hello World!")
```

The `text()` function creates a text widget. Text inherits styling from its container by default, with white text color.

## Adding Interactivity

Let's make it interactive with a click counter. Update your code:

```rust
use guido::prelude::*;

fn main() {
    let count = create_signal(0);

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .height(fill())
                .layout(
                    Flex::row()
                        .spacing(8.0)
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.2, 0.2, 0.3))
                        .corner_radius(4.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || count.update(|c| *c += 1))
                        .child(text(move || format!("Clicks: {}", count.get()))),
                )
                .child(container().padding(8.0).child(text("Hello World!")))
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.3, 0.2, 0.2))
                        .corner_radius(4.0)
                        .child(text("Status Bar")),
                )
        },
    );
    app.run();
}
```

### What Changed?

1. **Signal** - `create_signal(0)` creates a reactive value
2. **Hover state** - `.hover_state(|s| s.lighter(0.1))` lightens on hover
3. **Pressed state** - `.pressed_state(|s| s.ripple())` adds a ripple effect
4. **Click handler** - `.on_click(...)` increments the counter
5. **Reactive text** - `text(move || format!(...))` updates when the signal changes

## Next Steps

You've built your first Guido application. Continue learning:

- [Running Examples](examples.md) - Explore more complex examples
- [Core Concepts](../concepts/README.md) - Understand the reactive system
- [Building UI](../building-ui/README.md) - Learn styling and layout
