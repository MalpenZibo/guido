# Wayland Layer Shell

Guido uses the Wayland layer shell protocol for positioning widgets on the desktop. This enables status bars, panels, and overlays.

## App Configuration

```rust
App::new()
    .width(1920)
    .height(32)
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .layer(Layer::Top)
    .namespace("my-status-bar")
    .background_color(Color::rgb(0.1, 0.1, 0.15))
    .run(view);
```

## Layers

Control where your widget appears in the stacking order:

```rust
App::new().layer(Layer::Top)
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

## Anchoring

Control which screen edges the widget attaches to:

```rust
App::new().anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
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
.anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
.height(32)
```

**Bottom dock (full width):**
```rust
.anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
.height(48)
```

**Left sidebar (full height):**
```rust
.anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT)
.width(64)
```

**Corner widget (top-right):**
```rust
.anchor(Anchor::TOP | Anchor::RIGHT)
.width(200)
.height(100)
```

**Centered floating (no anchors):**
```rust
// No anchor = centered on screen
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
App::new()
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .height(32)

// Both dimensions specified, widget is 200x100
App::new()
    .anchor(Anchor::TOP | Anchor::RIGHT)
    .width(200)
    .height(100)
```

## Namespace

Identify your widget to the compositor:

```rust
App::new().namespace("my-app-name")
```

Some compositors use this for:
- Workspace rules
- Blur effects
- Per-app settings

## Exclusive Zones

Reserve screen space (windows won't overlap):

```rust
App::new()
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    .height(32)
    .exclusive_zone(32)  // Reserve 32px at top
```

Without exclusive zone, windows can cover the widget.

## Complete Examples

### Status Bar

```rust
fn main() {
    let view = container()
        .layout(Flex::row().main_axis_alignment(MainAxisAlignment::SpaceBetween))
        .children([
            left_section(),
            center_section(),
            right_section(),
        ]);

    App::new()
        .height(32)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .layer(Layer::Top)
        .exclusive_zone(32)
        .namespace("status-bar")
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
```

### Dock

```rust
fn main() {
    let view = container()
        .layout(Flex::row().spacing(8.0).main_axis_alignment(MainAxisAlignment::Center))
        .children([
            dock_icon("terminal"),
            dock_icon("browser"),
            dock_icon("files"),
        ]);

    App::new()
        .height(64)
        .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
        .layer(Layer::Top)
        .exclusive_zone(64)
        .namespace("dock")
        .background_color(Color::rgba(0.1, 0.1, 0.15, 0.9))
        .run(view);
}
```

### Floating Overlay

```rust
fn main() {
    let view = container()
        .padding(20.0)
        .background(Color::rgb(0.15, 0.15, 0.2))
        .corner_radius(12.0)
        .child(text("Notification").color(Color::WHITE));

    App::new()
        .width(300)
        .height(100)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .layer(Layer::Overlay)
        .namespace("notification")
        .background_color(Color::TRANSPARENT)
        .run(view);
}
```

## API Reference

```rust
impl App {
    pub fn width(self, width: u32) -> Self;
    pub fn height(self, height: u32) -> Self;
    pub fn anchor(self, anchor: Anchor) -> Self;
    pub fn layer(self, layer: Layer) -> Self;
    pub fn exclusive_zone(self, zone: i32) -> Self;
    pub fn namespace(self, namespace: &str) -> Self;
    pub fn background_color(self, color: Color) -> Self;
    pub fn on_update(self, callback: impl Fn() + 'static) -> Self;
    pub fn run(self, view: impl Widget + 'static);
}
```
