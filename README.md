# Guido

A reactive Rust GUI library for Wayland layer shell widgets.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Overview

Guido is a GPU-accelerated GUI library for building Wayland layer shell applications like status bars, panels, and overlays. It features a fine-grained reactive programming model inspired by [Floem](https://github.com/lapce/floem), with automatic dependency tracking and efficient re-rendering.

**Note:** This project is developed collaboratively using AI agents.

## Features

- **Fine-Grained Reactivity** - Thread-safe reactive signals with automatic dependency tracking and efficient updates. Signals are `Copy`, eliminating the need for manual cloning
- **Reusable Components** - Define custom components with the `#[component]` macro and fluent builder API
- **GPU Rendering** - Hardware-accelerated rendering via wgpu
- **Superellipse Corners** - Smooth iOS-style "squircle" corners with configurable curvature
- **SDF Borders** - Crisp anti-aliased borders using signed distance field rendering
- **Composable Widgets** - Build UIs from minimal primitives (Container, Row, Column, Text)
- **Layer Shell Support** - Native Wayland layer shell integration for status bars and panels
- **HiDPI Support** - Automatic scaling for high-resolution displays

## Quick Start

```rust
use guido::prelude::*;

fn main() {
    let count = create_signal(0);

    let view = container()
        .layout(Flex::row().spacing(16.0))
        .child(text(move || format!("Count: {}", count.get())).color(Color::WHITE))
        .child(
            container()
                .background(Color::rgb(0.3, 0.3, 0.4))
                .corner_radius(8.0)
                .padding(8.0)
                .ripple()
                .on_click(move || count.update(|c| *c += 1))
                .child(text("Click me").color(Color::WHITE))
        );

    App::new()
        .width(400)
        .height(48)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .layer(Layer::Top)
        .run(view);
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
- **reactive_example** - Interactive features with signals, click handlers, and ripple effects
- **showcase** - Comprehensive feature demo showing different corner curvatures
- **component_example** - Reusable components (Button, Card) with reactive props

## Requirements

- Rust 1.70+
- Wayland compositor with layer shell support (e.g., Sway, Hyprland)
- GPU with Vulkan or OpenGL support

## License

MIT License - see [LICENSE](LICENSE) for details.
