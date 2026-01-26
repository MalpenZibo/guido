<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://github.com/MalpenZibo/guido/blob/main/assets/logo_text_light.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://github.com/MalpenZibo/guido/blob/main/assets/logo_text_dark.svg">
    <img alt="Guido Logo" src="https://github.com/MalpenZibo/guido/blob/main/assets/logo_text_dark.svg" width="300">
  </picture>
</p>

# Guido

A reactive Rust GUI library for Wayland layer shell widgets.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Overview

Guido is a GPU-accelerated GUI library for building Wayland layer shell applications like status bars, panels, and overlays. It features a fine-grained reactive programming model inspired by [Floem](https://github.com/lapce/floem), with automatic dependency tracking and efficient re-rendering.

**Note:** This project is developed collaboratively using AI agents.

## Features

- **Fine-Grained Reactivity** - Thread-safe reactive signals with automatic dependency tracking. Signals are `Copy`, eliminating manual cloning
- **State Layer System** - Declarative hover/pressed style overrides with animations and Material-style ripple effects
- **GPU Rendering** - Hardware-accelerated rendering via wgpu with SDF-based shapes
- **Transform System** - Translate, rotate, and scale widgets with proper hit testing and animations
- **Multi-Surface Apps** - Create multiple layer shell surfaces that share reactive state
- **Superellipse Corners** - Configurable corner curvature from squircle (iOS-style) to bevel to scoop
- **SDF Borders** - Crisp anti-aliased borders using signed distance field rendering
- **Composable Widgets** - Build UIs from minimal primitives with pluggable Flex layout
- **Layer Shell Support** - Native Wayland layer shell integration for status bars and panels
- **HiDPI Support** - Automatic scaling for high-resolution displays
- **Image Widget** - Display raster images (PNG, JPEG, WebP) and SVGs with GPU texture caching
- **Scrollable Containers** - Vertical/horizontal scrolling with customizable scrollbars and momentum

## Quick Start

```rust
use guido::prelude::*;

fn main() {
    let count = create_signal(0);

    App::new()
        .add_surface(
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
                            .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .padding_xy(16.0, 0.0)
                    .child(text(move || format!("Count: {}", count.get())).color(Color::WHITE))
                    .child(
                        container()
                            .background(Color::rgb(0.3, 0.3, 0.4))
                            .corner_radius(8.0)
                            .padding(8.0)
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
                            .on_click(move || count.update(|c| *c += 1))
                            .child(text("Click me").color(Color::WHITE)),
                    )
            },
        )
        .run();
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

- **status_bar** - Basic status bar layout demonstration
- **reactive_example** - Interactive features with signals and state layers
- **multi_surface** - Multiple surfaces with shared reactive state
- **state_layer_example** - Hover, pressed states, and ripple effects
- **transform_example** - Rotation, scale, and animated transforms
- **animation_example** - Spring and eased animations
- **showcase** - Corner curvature variations (squircle, circle, bevel, scoop)
- **component_example** - Reusable components with reactive props
- **children_example** - Dynamic lists with keyed reconciliation
- **image_example** - Raster and SVG images with content fit modes
- **scroll_example** - Scrollable containers with custom scrollbar styling

## Documentation

- [Architecture Overview](docs/ARCHITECTURE.md) - System design and module structure
- [State Layer API](docs/STATE_LAYER.md) - Hover/pressed styles and ripple effects
- [Transform System](docs/TRANSFORMS.md) - Translate, rotate, scale with animations
- [Reactive System](docs/REACTIVE.md) - Signals, computed values, and effects
- [Styling Guide](docs/STYLING.md) - Colors, gradients, borders, and corners
- [Image Widget](docs/IMAGES.md) - Displaying raster and SVG images

## Requirements

- Rust 1.70+
- Wayland compositor with layer shell support (e.g., Sway, Hyprland)
- GPU with Vulkan or OpenGL support

## License

MIT License - see [LICENSE](LICENSE) for details.
