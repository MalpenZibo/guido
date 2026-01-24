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
    let view = container()
        .layout(
            Flex::row()
                .spacing(8.0)
                .main_axis_alignment(MainAxisAlignment::SpaceBetween),
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
        );

    App::new()
        .height(32)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
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

### Building the View

The view is built using **containers** - the primary building block in Guido:

```rust
let view = container()
    .layout(Flex::row().spacing(8.0).main_axis_alignment(MainAxisAlignment::SpaceBetween))
    // ... children
```

This creates a container with a horizontal flex layout. The `SpaceBetween` alignment spreads children across the available space.

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

### Running the App

```rust
App::new()
    .height(32)
    .background_color(Color::rgb(0.1, 0.1, 0.15))
    .run(view);
```

The `App` configures the window:
- **Height** - 32 pixels tall (width defaults to full screen for layer shell)
- **Background color** - Dark background for the bar
- **run()** - Starts the event loop with our view

## Adding Interactivity

Let's make it interactive with a click counter. Update your code:

```rust
use guido::prelude::*;

fn main() {
    let count = create_signal(0);

    let view = container()
        .layout(
            Flex::row()
                .spacing(8.0)
                .main_axis_alignment(MainAxisAlignment::SpaceBetween),
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
        );

    App::new()
        .height(32)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
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
