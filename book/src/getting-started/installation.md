# Installation

This guide walks you through setting up Guido for development.

## System Requirements

### Wayland Compositor

Guido requires a Wayland compositor with layer shell protocol support. Compatible compositors include:

- **Sway** - i3-compatible Wayland compositor
- **Hyprland** - Dynamic tiling Wayland compositor
- **river** - Dynamic tiling Wayland compositor
- **wayfire** - 3D Wayland compositor

> **Note:** X11 is not supported. Guido is designed specifically for Wayland layer shell applications.

### System Dependencies

Install the required development libraries for your distribution:

**Arch Linux:**
```bash
sudo pacman -S wayland wayland-protocols libxkbcommon
```

**Debian/Ubuntu:**
```bash
sudo apt install libwayland-dev libxkbcommon-dev
```

**Fedora:**
```bash
sudo dnf install wayland-devel libxkbcommon-devel
```

## Adding Guido to Your Project

### New Project

Create a new Rust project and add Guido:

```bash
cargo new my-app
cd my-app
cargo add guido
```

### Existing Project

Add Guido to your `Cargo.toml`:

```toml
[dependencies]
guido = "0.1"
```

Or use cargo:

```bash
cargo add guido
```

## Verifying Installation

Create a minimal test application to verify everything works:

```rust
// src/main.rs
use guido::prelude::*;

fn main() {
    let view = container()
        .padding(20.0)
        .background(Color::rgb(0.2, 0.3, 0.4))
        .child(text("Hello, Guido!").color(Color::WHITE));

    App::new()
        .width(200)
        .height(100)
        .run(view);
}
```

Run the application:

```bash
cargo run
```

If you see a small window with "Hello, Guido!" text, the installation is successful.

## Troubleshooting

### "No Wayland display" Error

Ensure you're running in a Wayland session, not X11:
```bash
echo $XDG_SESSION_TYPE  # Should output "wayland"
```

### Compositor Doesn't Support Layer Shell

Some Wayland compositors (like GNOME's Mutter) don't support the layer shell protocol. Use a compatible compositor listed above.

### Missing Libraries

If you get linker errors about missing libraries, ensure the system dependencies are installed. The error messages usually indicate which library is missing.

## Next Steps

Now that Guido is installed, continue to the [Hello World](hello-world.md) tutorial to build your first application.
