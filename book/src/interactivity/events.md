# Event Handling

Containers can respond to mouse events for user interaction.

## Click Events

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_click(|| {
        println!("Clicked!");
    })
# ;
# }
```

Click events fire when the mouse button is pressed and released within the container bounds.

### With Signal Updates

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);

container()
    .on_click(move || {
        count.update(|c| *c += 1);
    })
    .child(text(move || format!("Clicks: {}", count.get())))
# ;
# }
```

## Hover Events

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_hover(|hovered| {
        if hovered {
            println!("Mouse entered");
        } else {
            println!("Mouse left");
        }
    })
# ;
# }
```

The callback receives a boolean indicating hover state.

### Hover with State Layer

For visual hover effects, use `when_hovered` instead:

```rust,ignore
# // not compiled: `log` is the caller's crate, not one the book is
# // compiled against.
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Preferred for visual effects
container().when_hovered(|s| s.lighter(0.1));

// Use on_hover for side effects only
container().on_hover(|hovered| {
    log::info!("Hover changed: {}", hovered);
})
# ;
# }
```

## Other Buttons and Keys

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let close = move || {};
container()
    .on_click(|| println!("left"))
    .on_right_click(|| println!("right"))
    .on_middle_click(|| println!("middle"))
    .on_key_down(move |key, _mods| {
        if key == Key::Escape {
            close();
        }
    })
# ;
# }
```

`on_key_down` fires while the surface has keyboard focus — a layer surface
with `KeyboardInteractivity` set, or a popup holding a grab.

## Latched Modifiers

The `Modifiers` a key event carries describe *that keystroke*, which is what a
shortcut needs. Caps lock is a different question: it is latched, so it stays
on with nothing pressed, and something on screen may need to say so before
anything has been typed — a password field warning that what goes in will be
upper case.

That cannot be answered from a key event. The compositor sends the modifier
update *after* the key that toggled it, so the press of caps lock reports the
state it had before. Read the signal instead:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(move || {
    keyboard_modifiers()
        .get()
        .caps_lock
        .then(|| text("Caps lock is on"))
})
# ;
# }
```

Everything is false until the compositor sends the first update, which it does
when a surface takes keyboard focus.

## Scroll Events

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_scroll(|dx, dy, source| {
        println!("Scroll: dx={}, dy={}", dx, dy);
    })
# ;
# }
```

Parameters:
- `dx` - Horizontal scroll amount
- `dy` - Vertical scroll amount
- `source` - Scroll source (wheel, touchpad)

### Scroll with Signal

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let offset = create_signal(0.0f32);

container()
    .on_scroll(move |_dx, dy, _source| {
        offset.update(|o| *o += dy);
    })
    .child(text(move || format!("Offset: {:.0}", offset.get())))
# ;
# }
```

## Touch Input

Touchscreens work out of the box: the first finger down drives the same
pointer pipeline as the mouse, so `on_click`, `when_hovered`,
`when_pressed` (including ripples), and scroll all respond to touch. A
tap is a click; lifting the finger clears hover state. No code changes
are needed.

## Cursor Shape

A container says which shape the pointer takes over it:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let open = || {};
container()
    .cursor(CursorIcon::Pointer)
    .on_click(open)
# ;
# }
```

After every pointer event the shape is resolved from where the pointer is: the
innermost widget under it that declares one wins, and with none the pointer is
the arrow. A text input declares the I-beam the same way, so a field inside a
clickable row shows `Text` over itself and `Pointer` over the rest of the row,
with nothing to reset when the pointer moves between them:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let open = || {};
let query = create_signal(String::new());

container()
    .cursor(CursorIcon::Pointer)
    .on_click(open)
    .child(text_input(query))
# ;
# }
```

Like any other property it takes a signal, and a change reaches the screen
without the pointer having to move. A shape that is about a whole surface —
busy while something loads — is a `cursor` on its root:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let loading = create_signal(false);

container().cursor(move || {
    if loading.get() { CursorIcon::Wait } else { CursorIcon::Default }
})
# ;
# }
```

A container declared with `takes_input(false)` claims no cursor: the
compositor is not giving it the pointer, so whatever is beneath it answers.

## Combining Events

A container can have multiple event handlers:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);
let hovered = create_signal(false);

container()
    .on_click(move || count.update(|c| *c += 1))
    .on_hover(move |h| hovered.set(h))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
# ;
# }
```

## Event Propagation

Events flow through the widget tree from children to parents. A child receives events first; if it handles the event, the parent won't receive it.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Inner container handles clicks, outer doesn't receive them
container()
    .on_click(|| println!("Outer - won't fire for inner clicks"))
    .child(
        container()
            .on_click(|| println!("Inner - handles click"))
            .child(text("Click me"))
    )
# ;
# }
```

## Hit Testing

Events only fire when the click is within the container's bounds. Guido properly handles:

- **Corner radius** - Clicks outside rounded corners don't register
- **Transforms** - Rotated/scaled containers have correct hit areas
- **Nested transforms** - Parent transforms are accounted for

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
fn interactive_counter() -> impl Widget {
    let count = create_signal(0);
    let scroll_offset = create_signal(0.0f32);

    container()
        .layout(Flex::column().spacing(12.0))
        .padding(16.0)
        .children([
            // Click counter
            container()
                .padding(12.0)
                .background(Color::rgb(0.3, 0.5, 0.8))
                .corners(8.0)
                .when_hovered(|s| s.lighter(0.1))
                .when_pressed(|s| s.ripple())
                .on_click(move || count.update(|c| *c += 1))
                .child(
                    container().child(text(move || format!("Clicked {} times", count.get())).color(Color::WHITE))
                ),

            // Scroll display
            container()
                .padding(12.0)
                .background(Color::rgb(0.2, 0.3, 0.2))
                .corners(8.0)
                .when_hovered(|s| s.lighter(0.05))
                .on_scroll(move |_dx, dy, _source| {
                    scroll_offset.update(|o| *o += dy);
                })
                .child(
                    container().child(text(move || format!("Scroll offset: {:.0}", scroll_offset.get())).color(Color::WHITE))
                ),
        ])
}
# fn main() {}
```

## API Reference

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
impl Container {
    /// Handle click events
    pub fn on_click(self, handler: impl Fn() + 'static) -> Self;

    /// Handle hover state changes
    pub fn on_hover(self, handler: impl Fn(bool) + 'static) -> Self;

    /// Handle scroll events
    pub fn on_scroll(
        self,
        handler: impl Fn(f32, f32, ScrollSource) + 'static
    ) -> Self;

    /// The pointer's shape over this container
    pub fn cursor<M>(self, cursor: impl IntoSignal<CursorIcon, M>) -> Self;
}
```
