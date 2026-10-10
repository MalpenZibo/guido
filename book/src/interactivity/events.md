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

When one pointer move leaves some containers and enters others, every
`on_hover(false)` it causes is called before any `on_hover(true)`. Leaves go
from the innermost container outward, enters from the outermost inward, so the
container the pointer left hears first and the one it reached hears last. A
container holding both, which the pointer never left, hears nothing. The order
is the same whichever way the pointer moved and whichever sibling is drawn on
top, and it is the one browsers, Flutter, GTK and Qt give.

The hover *state* that `when_hovered` reads has already changed by the time
any of these callbacks runs.

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
    .on_key_down(move |key, _mods, _repeat| {
        if key == Key::Escape {
            close();
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    })
# ;
# }
```

`on_key_down` fires while the surface has keyboard focus — a layer surface
with `KeyboardInteractivity` set, or a popup holding a grab.

It answers whether it took the key, as a widget's `event` does. `Handled` stops
the key there; `Ignored` lets it go on to the next container listening, and
then to what the key does by default — Tab moving the focus, under
[Moving the Focus with Tab](#moving-the-focus-with-tab). A listener that answers
`Handled` for every key keeps Tab from ever moving the focus out of it.

A widget with the focus — a text input you clicked into — hears a key first,
and the containers around it after. `on_key_down` hears what nothing focused
took, so Escape still closes a menu while its search field has the focus, and
it hears it with nothing focused at all. Where two containers both declare it,
the inner one hears the key first; where they stand side by side, the earlier
one. The other hears it only when the first lets it through.

A held key arrives again at the compositor's repeat rate, and each repeat is
an ordinary key-down whose third argument is `true`. Text editing wants every
one; something that should happen once per press returns early on a repeat:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let activate = move || {};
container().on_key_down(move |key, _mods, repeat| {
    if key != Key::Enter {
        return EventResponse::Ignored;
    }
    if !repeat {
        activate();
    }
    EventResponse::Handled
})
# ;
# }
```

A widget of your own reads the same flag as the `repeat` field of
`Event::KeyDown`.

`on_key_up` hears a key being released, with the modifiers held as it went up,
along the same route as `on_key_down`. That is where a button answers Space:
the press shows it pressed, and the release activates it, so moving away before
letting go abandons it.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let activate = move || {};
let held = create_signal(false);
container()
    .on_key_down(move |key, _mods, _repeat| {
        if key != Key::Char(' ') {
            return EventResponse::Ignored;
        }
        held.set(true);
        EventResponse::Handled
    })
    .on_key_up(move |key, _mods| {
        if key == Key::Char(' ') && held.get_untracked() {
            held.set(false);
            activate();
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    })
# ;
# }
```

A release is not paired with its press. A text input that took a key's press
lets its release through, so a container around it or listening beside it can
hear a key go up that it never heard go down — which is why the sample above
remembers the press itself.

## Moving the Focus with Tab

Tab moves the keyboard focus to the next text input on the surface, and
Shift+Tab to the one before. It is what a Tab does when nothing took it: the
focused widget hears it first, then the containers around it, then the
listeners, and only a Tab all of them let through moves the focus. A widget
that wants Tab for itself — an editor inserting a tab character — answers
`Handled`, and the focus stays where it is.

The order is reading order: top to bottom by where each input was laid out,
then left to right, as GTK orders it. It follows what is on screen, so a list
reordered by key is walked in the order it is shown. At the last input Tab goes
round to the first, and Shift+Tab at the first to the last. With nothing
focused, Tab takes the first and Shift+Tab the last.

An input inside a hidden or disabled container is passed over, and so is one
playing its exit. Tab stays on the surface holding the keyboard: in a popup, it
goes round the popup's own inputs.

Tab with Ctrl, Alt or the logo key held does not move the focus; that is left
to whatever shortcut it is.

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

`CursorIcon::Hidden` is a shape like the others: the pointer is still there and
still reaches widgets, but nothing is drawn for it. A surface with no pointer —
a lock screen, a kiosk — is `Hidden` on its root, and a widget inside can still
claim a visible shape, because the innermost claim wins:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let unlock = || {};
container()
    .cursor(CursorIcon::Hidden)
    .child(container().cursor(CursorIcon::Pointer).on_click(unlock))
# ;
# }
```

A text input's I-beam is the innermost claim over it, so it shows even there; a
field that should not show it says so with its own `cursor` — see
[Pointer Shape](../building-ui/text-input.md#pointer-shape).

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
