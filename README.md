<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/MalpenZibo/guido/main/assets/wordmark-dark.svg">
    <img alt="GUIdo" src="https://raw.githubusercontent.com/MalpenZibo/guido/main/assets/wordmark-light.svg" height="140">
  </picture>
</p>

# Guido

A reactive Rust GUI library for Wayland layer shell widgets.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Overview

Guido is a GPU-accelerated GUI library for building Wayland layer shell applications like status bars, panels, popups, overlays and lock screens. Its state lives in fine-grained reactive signals: a widget property reads them and updates by itself when they change, with nothing rebuilt that did not read them.

**Note:** This project is developed collaboratively using AI agents.

## Features

- **Fine-grained reactivity** - `Copy` signals that any property takes as readily as a value; a change updates only what read it
- **Wayland surfaces** - Layer shell bars and panels, popups, lock screens and a surface per monitor, all sharing one reactive state
- **Composable widgets** - A handful of primitives, flex and stack layouts, and `#[component]` for widgets of your own
- **GPU rendering** - wgpu with SDF shapes: superellipse corners, borders, shadows, gradients, blur and paths
- **Interaction** - Hover and pressed states, ripples, transforms with correct hit testing, scrolling and text input
- **Animation** - Transitions, springs and keyframes on every animatable property
- **Text and images** - Font families and weights, PNG, JPEG, WebP and SVG
- **HiDPI** - Scales automatically on high-resolution displays

## Quick Start

```rust,no_run
use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        let count = create_signal(0);

        app.add_surface(
            SurfaceConfig::new()
                .height(48)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Top)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .height(fill())
                    .layout(
                        Flex::row()
                            .spacing(16.0)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .padding([0.0, 16.0])
                    .child(
                        text(move || format!("Count: {}", count.get())).color(Color::WHITE),
                    )
                    .child(
                        container()
                            .background(Color::rgb(0.3, 0.3, 0.4))
                            .corners(8.0)
                            .padding(8.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(move || count.update(|c| *c += 1))
                            .child(text("Click me").color(Color::WHITE)),
                    )
            },
        );
    });
}
```

## Building

```bash
# Build the library
cargo build

# Run the status bar example
cargo run --example status_bar

# Run the reactive example (demonstrates signals and events)
cargo run --example reactive_example

# Run the showcase (demonstrates various curvature options)
cargo run --example showcase

# Run the component example (demonstrates reusable components)
cargo run --example component_example
```

## Examples

There are over sixty under `examples/`; the book's [Running Examples](https://malpenzibo.github.io/guido/getting-started/examples.html) page walks through them. A few to start with:

- **status_bar** - Basic status bar layout demonstration
- **reactive_example** - Interactive features with signals and state layers
- **multi_surface** - Multiple surfaces with shared reactive state
- **surface_properties_example** - Dynamic surface property modification (layer, keyboard interactivity)
- **state_layer_example** - Hover, pressed states, and ripple effects
- **transform_example** - Rotation, scale, and animated transforms
- **animation_example** - Spring and eased animations
- **showcase** - Corner curvature variations (squircle, circle, bevel, scoop)
- **component_example** - Reusable components with reactive props
- **children_example** - Dynamic lists with keyed reconciliation
- **image_example** - Raster and SVG images with content fit modes
- **scroll_example** - Scrollable containers with custom scrollbar styling
- **scroll_mixed_content** - Scrolling with text, text input, and images combined
- **text_styling_example** - Font families and weights for text styling
- **service_example** - Background services with automatic cleanup
- **render_stats_test** - Render performance debugging (requires `--features render-stats`)

## Documentation

- [The Guido book](https://malpenzibo.github.io/guido) - Guides and examples for building with Guido
- [API reference](https://docs.rs/guido) - Every public type and method

For working on Guido itself:

- [Architecture Overview](docs/ARCHITECTURE.md) - System design and module structure
- [State Layer API](docs/STATE_LAYER.md) - Hover/pressed styles and ripple effects
- [Transform System](docs/TRANSFORMS.md) - Translate, rotate, scale with animations
- [Reactive System](docs/REACTIVE.md) - Signals, computed values, and effects
- [Styling Guide](docs/STYLING.md) - Colors, gradients, borders, and corners
- [Image Widget](docs/IMAGES.md) - Displaying raster and SVG images

## Requirements

- Rust 1.90 or newer
- A Wayland compositor with layer shell support (`wlr-layer-shell`): Sway, Hyprland, niri, KDE Plasma, river, labwc and others. GNOME does not support it
- A GPU with Vulkan or OpenGL support

## License

MIT License - see [LICENSE](LICENSE) for details.
