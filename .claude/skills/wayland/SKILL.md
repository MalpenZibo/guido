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

**Half of this is covered now, and half is not.**

`tests/headless_app.rs` drives the real loop — `iterate`, the function
`App::run` calls — with a `guido::testing::Headless` standing where the
compositor would be: a recorder implementing `Platform` and `Surface` that
answers for as many surfaces as the loop will carry and keeps what each was
asked. So a surface configuring, a frame opening, input routing, layout, paint,
*what the surface asks the compositor for*, a second surface, `spawn_surface`
and `close` at runtime, and the order a popup chain is torn down in all have a
sensor. It needs the `testing` feature and a GPU adapter.

What has none: what goes out on the wire, and what a compositor does with it.
The recorder proves guido asks for an exclusive zone of 50; it cannot prove niri
reserves 50. The teardown order is no longer in that half — both `Platform`
implementations answer `popup_descendants_bottom_up` from one
`descendants_bottom_up`, so the order the test walks is the order the
compositor is given. What `src/platform/popups.rs` still answers alone is which
surfaces are popups and whose children they are.

Still verified by running an example and looking: **the session lock and output
hotplug**. They type-check against the same trait and need no redesign, but each
needs something built: the recorder leaves `Platform`'s four lock methods at
their defaults, and `outputs::sync_outputs` is `pub(crate)`.

When you change this layer:

- run at least one example that exercises it (`cargo run --example status_bar`,
  `text_input_example`, and the popup and lock examples where relevant)
- say in the pull request which compositor, which examples, and what you saw
- `WAYLAND_DEBUG=1` prints the protocol traffic; `RUST_LOG=guido=debug` prints
  the library's own view of it

The second of those two — an application driven with no compositor — is built,
in `src/testing.rs`. What would close the rest is a headless compositor, which
would answer a different question: not whether guido asks correctly, but whether
the other end of the socket agrees.
