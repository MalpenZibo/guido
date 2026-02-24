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
}
```
