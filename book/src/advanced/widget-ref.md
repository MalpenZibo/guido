# Widget Ref

A **WidgetRef** is a handle from application code to one widget in the tree. It
answers two questions the composed view cannot answer on its own: where that
widget ended up, and where the keyboard should go.

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

## Moving the Keyboard

Attach the ref to the widget that takes focus — a `text_input`, since a container
cannot hold focus — and ask:

```rust
let field = create_widget_ref();

container()
    .layout(Flex::column().spacing(8.0))
    .child(text_input(value).widget_ref(field))
    .child(
        container()
            .on_click(move || field.focus())
            .child(text("Edit")),
    )
```

`focus()` lands on the next frame, not inside the call. Focus is resolved against
the tree — that is where a widget's ancestors are, and `when_focused` needs them
— and the composing code has no tree. Every toolkit arrives at the same shape:
Compose's `FocusRequester` must be called from an effect, iced's
`text_input::focus(id)` returns a `Task` for the runtime to run, and Flutter tells
you to request from a post-frame callback because during `initState` the widget is
not mounted.

Because of that, **asking early is allowed**: a request that names a widget which
has not been laid out *yet* waits for it rather than being dropped, so `focus()` in
a startup effect does what it looks like. Two requests in one frame are two answers
to the same question, and the later one wins.

A request does not wait forever, though, and the difference matters: one whose
widget has since **left** the tree is dropped, and so is one whose ref belonged to
a scope that is gone — `.focus()` from inside a popup that closed before the frame
came round. Both are waiting for something that already happened, and a request
left parked would fire at whatever took that ref next.

The rest of the verb:

```rust
field.blur();          // give the keyboard back, if this widget has it
field.is_focused();    // reactive when read in a tracked scope
field.widget();        // Some(WidgetId) once laid out; None before, and None
                       // again once the widget leaves or the ref's scope dies
```

For a field that should simply be ready when the screen appears, there is no need
for a handle at all — use
[`autofocus()`](../building-ui/text-input.md#initial-focus).

## When Not to Use It

A `WidgetRef` rect is written *after* layout, so a property derived from one
lags a frame — and reads zero on the first frame, before any layout has run.
That is fine for positioning a popup, and wrong for sizing. Two common cases
have direct layout support instead:

- **A bar proportional to a value** (slider fill, gauge, progress): use
  [`fraction()`](../concepts/layout.md#size-constraints), resolved against the
  real constraints during layout.
- **A widget sized to a sibling** (background, badge, decoration): stack them
  with [`ZStack`](../concepts/layout.md#stacking-children-zstack) and give the
  follower `fill()`.

## Edge Cases

- **Before first layout**: The signal returns `Rect::default()` (all zeros)
- **Widget removal**: The registry entry is automatically cleaned up
- **Cross-surface reads**: Works naturally since all surfaces share the main thread. Surface B may read a one-frame-old value if it renders before surface A
