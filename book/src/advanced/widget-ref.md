# Widget Ref

The **WidgetRef** API provides reactive access to a widget's layout bounds. This is useful when you need to position one widget relative to another — for example, centering a popup menu under a status bar module.

## Creating a WidgetRef

```rust
use guido::prelude::*;

let module_ref = create_widget_ref();
```

This creates a `WidgetRef` with an internal `Signal<Rect>` initialized to `Rect::default()` (all zeros). The signal is updated automatically after each layout pass.

## Attaching to a Container

Attach the ref to a container using the `.widget_ref()` builder method:

```rust
let module = container()
    .widget_ref(module_ref)
    .padding(8.0)
    .background(Color::rgb(0.2, 0.2, 0.3))
    .child(text("System Info"));
```

## Reading Bounds Reactively

Read the bounds via `.rect()`, which returns a `Signal<Rect>`:

```rust
let bounds_text = text(move || {
    let r = module_ref.rect().get();
    format!("x={:.0} y={:.0} w={:.0} h={:.0}", r.x, r.y, r.width, r.height)
});
```

The `Rect` contains surface-relative coordinates:
- `x`, `y` — top-left corner position relative to the surface origin
- `width`, `height` — the widget's layout size

## Positioning a Popup

A common use case is positioning a popup centered under a clickable module:

```rust
let module_ref = create_widget_ref();

// The module in the status bar
let module = container()
    .widget_ref(module_ref)
    .on_click(move || show_popup.set(true))
    .child(text("Menu"));

// The popup, centered under the module
let popup = container()
    .translate(
        move || {
            let r = module_ref.rect().get();
            let midpoint = r.x + r.width / 2.0;
            (midpoint - POPUP_WIDTH / 2.0).clamp(8.0, SCREEN_WIDTH - POPUP_WIDTH - 8.0)
        },
        BAR_HEIGHT,
    )
    .child(popup_content());
```

## Edge Cases

- **Before first layout**: The signal returns `Rect::default()` (all zeros)
- **Widget removal**: The registry entry is automatically cleaned up
- **Cross-surface reads**: Works naturally since all surfaces share the main thread. Surface B may read a one-frame-old value if it renders before surface A
