---
name: wayland
description: Guido's Wayland layer shell platform layer — surfaces, outputs and hotplug, popups, session lock, input (pointer, keyboard, touch), clipboard and selections, compositor effects. Use when touching src/platform/, when a surface misbehaves on a compositor, or when adding a protocol.
---

# The Wayland platform layer

`smithay-client-toolkit` for the protocol work, `wayland-protocols` for the
staging protocols sctk does not wrap. Lives in `src/platform/`.

## Surfaces

A surface is declared with `SurfaceConfig`: anchor, layer, size, margins,
namespace, keyboard interactivity, background colour. Properties can change at
runtime through a `SurfaceHandle`:

```rust
let handle = surface_handle(surface_id);
handle.set_layer(Layer::Overlay);
handle.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
```

Multiple surfaces share one reactive state and one renderer.

## What is implemented

- **Outputs** — reactive `outputs()` enumeration, per-output pinning,
  `surface_output()` tracking, hotplug
- **Input regions** — per-surface clickable rectangles, click-through surfaces
- **Popups** (`xdg_popup`) — compositor-positioned, grab semantics, outside
  click dismisses, reactive `dismissed()`, nesting
- **Session lock** (`ext-session-lock-v1`) — one lock surface per output,
  hotplug handled, reactive `lock_state()`
- **Touch** (`wl_touch`) — the first finger drives the pointer pipeline, so tap
  is click and state layers work
- **Clipboard** — async prefetch so paste never blocks the UI thread, plus
  primary selection
- **Compositor effects** — `ext-background-effect-v1` for backdrop blur, with
  `compositor_effects()` to ask what is available

## The thing to know before changing it

**None of this is covered by an automated test.** There is no headless
compositor in the harness, so protocol behaviour is verified by running an
example on a real compositor and looking. When you change this layer:

- run at least one example that exercises it (`cargo run --example status_bar`,
  `text_input_example`, and the popup and lock examples where relevant)
- say in the pull request which compositor, which examples, and what you saw
- `WAYLAND_DEBUG=1` prints the protocol traffic; `RUST_LOG=guido=debug` prints
  the library's own view of it

Closing this hole — a headless compositor, or an event-injection harness that
drives `ingress` directly — is the next thing worth building in the harness.
