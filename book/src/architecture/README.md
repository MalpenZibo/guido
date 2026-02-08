# Architecture

This section covers Guido's internal architecture for developers who want to understand how the library works or contribute to it.

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                          Application                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Widgets   │  │  Reactive   │  │       Platform          │  │
│  │  Container  │  │   Signals   │  │   Wayland Layer Shell   │  │
│  │    Text     │  │    Memo     │  │   Event Loop (calloop)  │  │
│  │   Layout    │  │   Effects   │  │                         │  │
│  └──────┬──────┘  └──────┬──────┘  └───────────┬─────────────┘  │
│         │                │                     │                 │
│         └────────────────┼─────────────────────┘                 │
│                          │                                       │
│                    ┌─────┴─────┐                                 │
│                    │  Renderer │                                 │
│                    │   wgpu    │                                 │
│                    │  glyphon  │                                 │
│                    └───────────┘                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Module Structure

| Module | Purpose |
|--------|---------|
| `reactive/` | Signals, memos, effects |
| `widgets/` | Container, Text, Layout trait |
| `renderer/` | wgpu rendering, shaders, text |
| `platform/` | Wayland layer shell integration |
| `transform.rs` | 2D transformation matrices |

## In This Section

- [System Overview](overview.md) - Module structure and key types
- [Rendering Pipeline](rendering.md) - How shapes and text are drawn
- [Event System](events.md) - Input event flow and handling

## Key Design Decisions

### Fine-Grained Reactivity

Guido uses signals rather than virtual DOM diffing. Widgets are created once; their properties update automatically through signal subscriptions.

### Builder Pattern

All configuration uses the builder pattern with method chaining:

```rust
container()
    .padding(16.0)
    .background(Color::RED)
    .child(text("Hello"))
```

### SDF Rendering

Shapes use Signed Distance Fields for resolution-independent rendering with crisp anti-aliasing at any scale.

### Layer Shell Native

Guido is built specifically for Wayland layer shell, not as a general windowing toolkit with layer shell support added later.
