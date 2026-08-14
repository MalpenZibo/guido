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
                            .child(container().text_color(Color::WHITE).child(text("+")))
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

## Backdrop Blur

`backdrop_blur` blurs whatever is behind a container. There are two
things behind it, on opposite sides of the Wayland surface, and both
are filtered:

- what **this surface** already drew — a wallpaper, a photo, the panel
  under a card;
- what the **compositor** composites below the surface, through
  `ext-background-effect-v1`, wherever the surface is translucent.

```rust
container()
    .background(Color::rgba(0.12, 0.12, 0.18, 0.55))
    .corner_radius(16.0)
    .backdrop_blur(32.0)
    .child(text("Frosted glass"))
```

Filtering both is not a compromise between mechanisms. Blur is a linear
operator, so over a region of uniform alpha `a`:

```text
blur(a·ours + (1−a)·theirs) = a·blur(ours) + (1−a)·blur(theirs)
```

Blurring each layer separately and compositing gives exactly the same
result — which is why neither is chosen over the other. A translucent
panel is neither "ours" nor "theirs" at any pixel, so a per-box choice
could only be right for some of them.

Restrict it when only one side should soften:

```rust
// Desktop behind the panel softens; the panel's own islands stay crisp.
container().backdrop_blur(
    BackdropBlur::new(24.0).sources(BackdropSources::COMPOSITOR),
)
```

### What each side costs

The **compositor** side is a region handed over once per change: the
region follows the container automatically as layout moves, resizes and
(animated) corner radii change. Rounded corners are tessellated into
small rectangles because `wl_region` has no notion of curves, and the
protocol carries no radius — the compositor picks its own, so `radius`
does not apply there. Check availability with `compositor_effects()`.

The **surface** side is guido's own: the frame is drawn into an
offscreen target, each region is downsampled, blurred with a separable
gaussian and composited back masked to the container's rounded shape.
Frames with no backdrop blur skip all of it and draw straight to the
swapchain.

Two skips keep that honest, and both are unobservable — they drop work
that could not have changed a pixel, so they may switch on and off
between frames without anything showing:

- a region with nothing drawn beneath it is not filtered at all;
- surfaces that never asked for a compositor region are left untouched,
  so compositor-side rules (e.g. blur by namespace) keep working.

### The seam

Where alpha *varies* within the blur radius — at the edges of opaque
content inside the box — the two blurs do not bleed into each other:
the desktop does not smear into the surface's own content or the other
way round. That cannot be fixed from guido. `set_blur_region` is
fire-and-forget: it takes a region and returns nothing, so the
compositor's pixels are never ours to filter across.

See `examples/blur_example.rs` and `examples/draw_order_example.rs`.

## Popups (Menus, Dropdowns)

`spawn_popup` creates an **xdg popup** anchored to a surface: the
compositor positions it relative to an anchor rectangle, keeps it on
screen (flipping/sliding at screen edges), and — with `.grab()` —
dismisses it when the user clicks outside. Real menu semantics, no
fullscreen overlay:

```rust
let popup = spawn_popup(
    bar_id,
    PopupConfig::new(250)                      // width; height sizes to content
        .anchor_rect(button_ref.rect().get())  // parent surface coords
        .anchor(PopupAnchor::Bottom)           // attach point on the rect
        .gravity(PopupGravity::Bottom)         // growth direction
        .grab(),                               // dismiss on outside click
    move || menu_widget(),
);
```

Dismissal is reactive — reset your open/closed state when the
compositor closes the popup:

```rust
create_effect(move || {
    if popup.dismissed() {
        menu_open.set(false);
    }
})
.detach();
```

`popup.close()` closes it programmatically. Popups render their own
widget tree and share the app's reactive state like any surface;
anchoring a popup to another popup creates a nested popup (submenus).

Note: for a bottom bar use `anchor(Top)` + `gravity(Top)` so the menu
opens upward — or just rely on the compositor's flip adjustment.

See `examples/popup_example.rs`.

## Session Lock (Lock Screens)

`lock_session` asks the compositor to lock the session
(`ext-session-lock-v1`). Once granted, guido creates one lock surface
per output using your widget factory — the compositor blanks every
output, shows the lock surfaces, and routes all input to them, so a
`text_input` password field works out of the box:

```rust
lock_session(|output: OutputInfo| {
    let attempt = create_signal(String::new());
    container()
        .width(fill())
        .height(fill())
        .background(Color::rgb(0.07, 0.07, 0.1))
        .layout(Flex::column().main_alignment(MainAlignment::Center))
        .child(text(format!("Locked — {:?}", output.name)))
        .child(text_input(attempt).password(true).on_submit(|s| {
            if verify_password(s) {
                unlock_session();
            }
        }))
});
```

The lifecycle is reactive:

```rust
text(move || format!("{:?}", lock_state().get()))
// Unlocked → Locking → Locked, back to Unlocked on unlock/denial
```

Details worth knowing:

- Outputs plugged in **while locked** get a lock surface automatically
  (the factory is called again); the compositor blanks any output
  without one.
- If the compositor refuses the lock (no protocol support, or another
  lock client is active), `lock_state` returns to `Unlocked`.
- Unlike ordinary surfaces, closing lock surfaces never exits the app:
  a lock daemon whose only surfaces are lock surfaces keeps running
  after unlocking, idle until the next `lock_session` call.
- Layer-shell properties (anchor, margins, exclusive zone…) don't
  apply to lock surfaces; their size always comes from the compositor.

See `examples/simple_lock.rs` — note that running it really locks your
session (it has a 30-second auto-unlock safety net; the password is
`guido`).

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
                    .child(container().text_color(Color::WHITE).child(text("Notification")))
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
