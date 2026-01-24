# Getting Started

This section will guide you through setting up Guido and creating your first application.

## What You'll Learn

- [Installation](installation.md) - Add Guido to your project and set up dependencies
- [Hello World](hello-world.md) - Build your first Guido application step by step
- [Running Examples](examples.md) - Explore the included examples to learn different features

## Prerequisites

Before you begin, ensure you have:

- **Rust** (1.70 or later) - Install via [rustup](https://rustup.rs/)
- **Wayland compositor** - A compositor that supports the layer shell protocol (Sway, Hyprland, etc.)
- **System dependencies** - Development libraries for Wayland and graphics

## Quick Start

If you're eager to get started, here's the fastest path:

```bash
# Create a new project
cargo new my-guido-app
cd my-guido-app

# Add Guido dependency
cargo add guido

# Run the app
cargo run
```

Then replace `src/main.rs` with a simple Guido application. See the [Hello World](hello-world.md) guide for the complete code.
