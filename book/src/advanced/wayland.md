# Wayland Layer Shell

Guido uses the Wayland layer shell protocol for positioning widgets on the desktop. This enables status bars, panels, overlays, and multi-surface applications.

## Surface Configuration

Each surface is configured using `SurfaceConfig`:

```rust
App::new().run(|app| {
    let _surface_id = app.add_surface(
        SurfaceConfig::new()
            .width(1920)
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .layer(Layer::Top)
            .keyboard_interactivity(KeyboardInteractivity::OnDemand)
            .namespace("my-status-bar")
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        || view,
    );
});
```

Note: `run()` takes a setup closure where you add surfaces. `add_surface()` returns a `SurfaceId` that can be used to get a `SurfaceHandle` for dynamic property modification.

## Layers

Control where your surface appears in the stacking order:

```rust
SurfaceConfig::new().layer(Layer::Top)
```

| Layer | Description |
|-------|-------------|
| `Background` | Below all windows |
| `Bottom` | Above background, below windows |
| `Top` | Above windows (default) |
| `Overlay` | Above everything |

### Use Cases

- **Background**: Desktop widgets, wallpaper effects
- **Bottom**: Dock bars (below windows but above background)
- **Top**: Status bars, panels (above windows)
- **Overlay**: Notifications, lock screens

## Keyboard Interactivity

Control how the surface receives keyboard focus:

```rust
SurfaceConfig::new().keyboard_interactivity(KeyboardInteractivity::OnDemand)
```

| Mode | Description |
|------|-------------|
| `None` | Surface never receives keyboard focus |
| `OnDemand` | Surface receives focus when clicked (default) |
| `Exclusive` | Surface grabs keyboard focus exclusively |

### Use Cases

- **None**: Status bars that only respond to mouse
- **OnDemand**: Panels with text input fields
- **Exclusive**: Lock screens, app launchers, modal dialogs

## Anchoring

Control which screen edges the surface attaches to:

```rust
SurfaceConfig::new().anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
```

| Anchor | Effect |
|--------|--------|
| `TOP` | Attach to top edge |
| `BOTTOM` | Attach to bottom edge |
| `LEFT` | Attach to left edge |
| `RIGHT` | Attach to right edge |

### Common Patterns

**Top status bar (full width):**
```rust
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .height(32)
```

**Bottom dock (full width):**
```rust
SurfaceConfig::new()
    .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
    .height(48)
```

**Left sidebar (full height):**
```rust
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT)
    .width(64)
```

**Corner widget (top-right):**
```rust
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::RIGHT)
    .width(200)
    .height(100)
```

**Centered floating (no anchors):**
```rust
// No anchor = centered on screen
SurfaceConfig::new()
    .width(400)
    .height(300)
```

## Size Behavior

Size depends on anchoring:

- **Anchored dimension**: Expands to fill (e.g., width when LEFT+RIGHT anchored)
- **Unanchored dimension**: Uses specified size
- **No anchors**: Uses exact size, centered on screen

```rust
// Width fills screen, height is 32px
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .height(32)

// Both dimensions specified, widget is 200x100
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::RIGHT)
    .width(200)
    .height(100)
```

## Namespace

Identify your surface to the compositor:

```rust
SurfaceConfig::new().namespace("my-app-name")
```

Some compositors use this for:
- Workspace rules
- Blur effects
- Per-app settings

## Exclusive Zones

Reserve screen space (windows won't overlap):

```rust
SurfaceConfig::new()
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .height(32)
    .exclusive_zone(32)  // Reserve 32px at top
```

Without exclusive zone, windows can cover the surface.

## Multi-Surface Applications

Guido supports creating multiple surfaces within a single application. All surfaces share the same reactive state, allowing for coordinated updates.

### Multiple Static Surfaces

Define multiple surfaces at startup:

```rust
fn main() {
    App::new().run(|app| {
        // Shared reactive state
        let count = create_signal(0);

        // Top status bar
        app.add_surface(
            SurfaceConfig::new()
                .height(32)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Top)
                .namespace("status-bar")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .height(fill())
                    .layout(
                        Flex::row()
                            .main_alignment(MainAlignment::SpaceBetween)
                            .cross_alignment(CrossAlignment::Center)
                    )
                    .padding([0.0, 16.0])
                    .child(text("Status Bar"))
                    .child(text(move || format!("Count: {}", count.get())))
            },
        );

        // Bottom dock
        app.add_surface(
            SurfaceConfig::new()
                .height(48)
                .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Top)
                .namespace("dock")
                .background_color(Color::rgb(0.15, 0.15, 0.2)),
            move || {
                container()
                    .height(fill())
                    .layout(
                        Flex::row()
                            .spacing(16.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center)
                    )
                    .child(
                        container()
                            .padding([8.0, 16.0])
                            .background(Color::rgb(0.3, 0.3, 0.4))
                            .corner_radius(8.0)
                            .hover_state(|s| s.lighter(0.1))
                            .on_click(move || count.update(|c| *c += 1))
                            .child(text("+").color(Color::WHITE))
                    )
            },
        );
    });
}
```

### Key Points

- **Shared State**: All surfaces share the same reactive signals
- **Independent Widget Trees**: Each surface has its own widget tree
- **Fill Layout**: Use `height(fill())` to make containers expand to fill the surface

### Dynamic Surfaces

Create and destroy surfaces at runtime using `spawn_surface()`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    App::new().run(|app| {
        let popup_handle: Rc<RefCell<Option<SurfaceHandle>>> = Rc::new(RefCell::new(None));
        let popup_clone = popup_handle.clone();

        app.add_surface(
            SurfaceConfig::new()
                .height(32)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                container()
                    .child(
                        container()
                            .padding(8.0)
                            .hover_state(|s| s.lighter(0.1))
                            .on_click({
                                let popup_handle = popup_clone.clone();
                                move || {
                                    let mut handle = popup_handle.borrow_mut();
                                    if let Some(h) = handle.take() {
                                        // Close existing popup
                                        h.close();
                                    } else {
                                        // Create new popup
                                        let new_handle = spawn_surface(
                                            SurfaceConfig::new()
                                                .width(200)
                                                .height(300)
                                                .anchor(Anchor::TOP | Anchor::RIGHT)
                                                .layer(Layer::Overlay)
                                                .keyboard_interactivity(KeyboardInteractivity::Exclusive),
                                            || {
                                                container()
                                                    .padding(16.0)
                                                    .child(text("Popup Content"))
                                            }
                                        );
                                        *handle = Some(new_handle);
                                    }
                                }
                            })
                            .child(text("Toggle Popup"))
                    )
            },
        );
    });
}
```

### SurfaceHandle API

The `SurfaceHandle` allows controlling a surface after creation:

```rust
impl SurfaceHandle {
    /// Close and destroy the surface
    pub fn close(&self);

    /// Get the surface ID
    pub fn id(&self) -> SurfaceId;

    /// Change the layer (Background, Bottom, Top, Overlay)
    pub fn set_layer(&self, layer: Layer);

    /// Change keyboard interactivity mode
    pub fn set_keyboard_interactivity(&self, mode: KeyboardInteractivity);

    /// Change anchor edges
    pub fn set_anchor(&self, anchor: Anchor);

    /// Change surface size
    pub fn set_size(&self, width: u32, height: u32);

    /// Change exclusive zone
    pub fn set_exclusive_zone(&self, zone: i32);

    /// Change margins
    pub fn set_margin(&self, top: i32, right: i32, bottom: i32, left: i32);
}
```

### Getting a Handle for Existing Surfaces

Use `surface_handle()` to get a handle for any surface by its ID:

```rust
App::new().run(|app| {
    // Store the ID when adding the surface
    let status_bar_id = app.add_surface(config, move || {
        container()
            .on_click(move || {
                // Get handle and modify properties dynamically
                let handle = surface_handle(status_bar_id);
                handle.set_layer(Layer::Overlay);
                handle.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            })
            .child(text("Click to promote to overlay"))
    });
});
```

## Multiple Outputs (Monitors)

Connected outputs are exposed as a reactive list. Reading it inside a
tracked closure (an effect, a dynamic child, a reactive property)
re-runs the closure when monitors are plugged in, unplugged, or
reconfigured:

```rust
use guido::prelude::*;

create_effect(move || {
    for info in outputs().get() {
        println!(
            "{:?}: {:?} {} ({}x{:?})",
            info.id, info.name, info.model, info.scale_factor, info.logical_size
        );
    }
});
```

Each `OutputInfo` carries a stable `OutputId` plus the connector name
(`"DP-1"`, `"eDP-1"`, …), description, make/model, integer scale factor,
and logical size/position when the compositor reports them. Ids are never
reused: a monitor that is unplugged and reconnected gets a fresh id.

### Pinning a Surface to an Output

By default the compositor picks the output a surface appears on. Pass an
`OutputId` to pin it:

```rust
spawn_surface(
    SurfaceConfig::new()
        .height(32)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .output(info.id),
    move || bar_widget(),
);
```

The output cannot be changed after creation — a layer surface is bound to
its output for its lifetime. If the output disconnects before the surface
is created, the compositor chooses one instead (a warning is logged).

### One Bar per Monitor

An app can start with **zero surfaces** and spawn one per output from an
effect — the classic multi-monitor status bar. See
`examples/multi_output.rs` for the full version:

```rust
App::new().run(|_app| {
    let bars: Rc<RefCell<HashMap<OutputId, SurfaceHandle>>> =
        Rc::new(RefCell::new(HashMap::new()));

    create_effect(move || {
        let current = outputs().get();
        let mut bars = bars.borrow_mut();

        // Drop bars for disconnected outputs
        bars.retain(|id, handle| {
            let alive = current.iter().any(|o| o.id == *id);
            if !alive {
                handle.close();
            }
            alive
        });

        // Spawn a bar on every new output
        for info in current {
            if !bars.contains_key(&info.id) {
                let handle = spawn_surface(
                    SurfaceConfig::new().height(32).output(info.id),
                    move || bar_widget(),
                );
                bars.insert(info.id, handle);
            }
        }
    });
});
```

When the compositor closes the surfaces of an unplugged monitor, the
bars keep working on the remaining outputs. Note that the app exits when
its last surface closes, so an all-monitors-disconnected event ends the
app.

### Tracking Which Output a Surface Is On

`surface_output(surface_id)` reports the output a surface is currently
shown on (`None` until the compositor maps it). It is a tracked read —
reactive inside any tracked closure:

```rust
text(move || match surface_output(my_surface_id) {
    Some(out) => format!("shown on output {}", out.raw()),
    None => "not mapped yet".to_string(),
})
```

For a surface spanning multiple outputs, this reports the one entered
most recently.

## Input Regions

By default the whole surface accepts pointer and touch input. An input
region limits input to a set of rectangles (logical surface
coordinates) — everything outside them lets clicks pass through to the
windows below. This is how transparent overlays avoid stealing clicks:

```rust
// Only the given rectangle is clickable:
SurfaceConfig::new()
    .background_color(Color::TRANSPARENT)
    .input_region([Rect::new(16.0, 20.0, 200.0, 40.0)])

// Fully click-through (e.g. a HUD or wallpaper widget):
SurfaceConfig::new().click_through()
```

At runtime, use the handle — `None` restores full-surface input, an
empty list is fully click-through:

```rust
surface_handle(id).set_input_region(Some(vec![rect]));
surface_handle(id).set_input_region(None);
```

The idiomatic pattern glues the region to a widget's bounds with a
`WidgetRef`, so it follows layout changes automatically (full version
in `examples/input_region.rs`):

```rust
let pill_ref = create_widget_ref();
// ...build the surface with .click_through() and .widget_ref(pill_ref)...

create_effect(move || {
    let rect = pill_ref.rect().get();
    if rect.width > 0.0 {
        surface_handle(id).set_input_region(Some(vec![rect]));
    }
});
```

Note: keyboard focus is unaffected — use `keyboard_interactivity` for
that. Rectangles are rounded outward to whole pixels.

## Background Blur

Containers can ask the compositor to blur whatever is behind the
surface (`ext-background-effect-v1`), shaped by their bounds and corner
radius. Pair it with a translucent background so the blurred content
shows through:

```rust
container()
    .background(Color::rgba(0.12, 0.12, 0.18, 0.55))
    .corner_radius(16.0)
    .background_blur()
    .child(text("Frosted glass"))
```

The blur region follows the container automatically: layout moves,
resizes, and (animated) corner radius changes are picked up each frame.
Rounded corners are tessellated into small rectangles because
`wl_region` has no notion of curves.

On compositors without the protocol or its blur capability this
renders a plain translucent container — no error, no fallback blur.
Surfaces that never use `background_blur()` are left untouched, so
compositor-side blur rules (e.g. blur by namespace) keep working; once
a surface has used blur, an empty region ("blur nothing") is reported
instead of withdrawing, to keep the region authoritative.

See `examples/blur_example.rs`.

## Complete Examples

### Status Bar

```rust
fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(32)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Top)
                .exclusive_zone(Some(32))
                .namespace("status-bar")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .height(fill())
                    .layout(
                        Flex::row()
                            .main_alignment(MainAlignment::SpaceBetween)
                            .cross_alignment(CrossAlignment::Center)
                    )
                    .children([
                        left_section(),
                        center_section(),
                        right_section(),
                    ])
            },
        );
    });
}
```

### Dock

```rust
fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(64)
                .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Top)
                .exclusive_zone(Some(64))
                .namespace("dock")
                .background_color(Color::rgba(0.1, 0.1, 0.15, 0.9)),
            || {
                container()
                    .height(fill())
                    .layout(
                        Flex::row()
                            .spacing(8.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center)
                    )
                    .children([
                        dock_icon("terminal"),
                        dock_icon("browser"),
                        dock_icon("files"),
                    ])
            },
        );
    });
}
```

### Floating Overlay with Keyboard Focus

```rust
fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(300)
                .height(100)
                .anchor(Anchor::TOP | Anchor::RIGHT)
                .layer(Layer::Overlay)
                .keyboard_interactivity(KeyboardInteractivity::Exclusive)
                .namespace("notification")
                .background_color(Color::TRANSPARENT),
            || {
                container()
                    .padding(20.0)
                    .background(Color::rgb(0.15, 0.15, 0.2))
                    .corner_radius(12.0)
                    .child(text("Notification").color(Color::WHITE))
            },
        );
    });
}
```

## API Reference

### SurfaceConfig

```rust
impl SurfaceConfig {
    pub fn new() -> Self;
    pub fn width(self, width: u32) -> Self;
    pub fn height(self, height: u32) -> Self;
    pub fn anchor(self, anchor: Anchor) -> Self;
    pub fn layer(self, layer: Layer) -> Self;
    pub fn keyboard_interactivity(self, mode: KeyboardInteractivity) -> Self;
    pub fn exclusive_zone(self, zone: Option<i32>) -> Self;
    pub fn namespace(self, namespace: impl Into<String>) -> Self;
    pub fn background_color(self, color: Color) -> Self;
    pub fn margin(self, top: i32, right: i32, bottom: i32, left: i32) -> Self;
    pub fn output(self, output: OutputId) -> Self;
    pub fn input_region(self, rects: impl Into<Vec<Rect>>) -> Self;
    pub fn click_through(self) -> Self;
}
```

### Outputs

```rust
/// Reactive list of connected outputs, sorted by id
pub fn outputs() -> Signal<Vec<OutputInfo>>;

/// The output a surface is currently shown on (tracked read)
pub fn surface_output(id: SurfaceId) -> Option<OutputId>;

pub struct OutputInfo {
    pub id: OutputId,
    pub name: Option<String>,        // connector, e.g. "DP-1"
    pub description: Option<String>,
    pub make: String,
    pub model: String,
    pub scale_factor: i32,
    pub logical_size: Option<(i32, i32)>,
    pub logical_position: Option<(i32, i32)>,
}
```

### App

```rust
impl App {
    pub fn new() -> Self;
    pub fn run(self, setup: impl FnOnce(&mut Self)) -> ExitReason;
    pub fn add_surface<W, F>(&mut self, config: SurfaceConfig, widget_fn: F) -> SurfaceId
    where
        W: Widget + 'static,
        F: FnOnce() -> W + 'static;
}
```

### Dynamic Surface Creation

```rust
/// Spawn a new surface at runtime
pub fn spawn_surface<W, F>(config: SurfaceConfig, widget_fn: F) -> SurfaceHandle
where
    W: Widget + 'static,
    F: FnOnce() -> W + Send + 'static;

/// Get a handle for an existing surface by ID
pub fn surface_handle(id: SurfaceId) -> SurfaceHandle;
```

### SurfaceHandle

```rust
impl SurfaceHandle {
    pub fn id(&self) -> SurfaceId;
    pub fn close(&self);
    pub fn set_layer(&self, layer: Layer);
    pub fn set_keyboard_interactivity(&self, mode: KeyboardInteractivity);
    pub fn set_anchor(&self, anchor: Anchor);
    pub fn set_size(&self, width: u32, height: u32);
    pub fn set_exclusive_zone(&self, zone: i32);
    pub fn set_margin(&self, top: i32, right: i32, bottom: i32, left: i32);
    pub fn set_input_region(&self, rects: Option<Vec<Rect>>);
}
```
