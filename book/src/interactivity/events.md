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

### Double Clicks

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let select = move || {};
# let open = move || {};
container()
    .on_click(select)
    .on_double_click(open)
# ;
# }
```

`on_double_click` fires on the release of the second click, as the web's
`dblclick` does. `on_click` still fires for both clicks, so the first click of
a double click selects and the second opens. A click is the second of a pair
when it is the same button, within 400 ms and 5 px of the first, with no drag
between.

Every press carries its place in the run: `Event::MouseDown`'s `count` is 1
for a single click, 2 for the second of a double click, 3 for a triple, so a
widget that wants a triple click reads it there. A finger always presses 1.

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

A container hidden or disabled while the pointer is on it ends the hover and
the press of everything in it, itself included: `on_hover(false)` is called,
and a press that was being held is dropped without a click, so nothing comes
back hovered or pressed when it is shown or enabled again. That happens at the
next pointer event to reach it, or once a frame has been drawn after hiding it
moved something on the surface, as the next paragraph says — not at the instant
it is hidden.

What is under a pointer that stays still can change: a wheel step, a touchpad's
glide or the keyboard scrolls a list, a row above grows, a child is built in
place of the one that was there. Once a frame has been drawn after something
like that, the hover and the cursor are worked out again where the pointer is,
so the row that went hears `on_hover(false)` and the one now under the pointer
`on_hover(true)`, without waiting for the pointer to move. It is worked out once
a frame, where the pointer is then: a row that a single wheel step carries
right past the pointer is never under it when the hover is worked out, and
hears nothing. A child removed with no exit hears nothing as it goes; what it uncovers does.
`on_pointer_move` hears none of this: the pointer did not move. Nor does a
hover style that draws the box elsewhere count — `when_hovered` drawing it
lifted or scaled — or a box near the pointer's edge would take its hover back
on one frame and give it back on the next. It is still worked out where the
box is drawn, so a layout elsewhere on the surface can take the hover from a
lifted box whose pointer rests in the strip the lift uncovered. One that
changes the size does count, as any layout does, and under a pointer resting
near the edge it flickers that way: a label whose `font_size` shrinks on hover,
or an `on_hover` that makes its container smaller or shows something on top of
it. A CSS `:hover` that resizes its element does the same. An animation that
only draws a box elsewhere is not followed either: a box that slides, or that
moves or enters by a transform, takes the hover when the pointer next moves,
and so does a sibling that only comes to be drawn on top. One that changes a
size is a layout, and is followed as one. With a button held nothing is
worked out again, since a move then would carry on as a drag; the release of
the mouse button does it, unless its click closes the surface. A finger lifted
is over nothing. A surface that changed size works nothing out again until the
next pointer event says where the pointer is.

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

A key is a `Key`: a character, `Key::Char`, or one of the named keys — Enter,
Tab, Escape, Backspace, Delete, the arrows, Home and End, `Key::PageUp`,
`Key::PageDown`, and the function keys `Key::F(1)` to `Key::F(12)`. The
keypad's Enter, and with NumLock off its arrows, Home, End, Page Up, Page Down
and Delete, are the same keys as the main block's. A text input takes neither Up, Down, Page Up,
Page Down nor a function key, so they reach the listener around it — unless a
scroller stands between them, which scrolls with the arrows, Page Up, Page
Down, Home and End while it can move: see
[What Moves It](../concepts/container.md#what-moves-it).

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

## Hearing a Key Before the Focused Widget

The field has the key first, so a key it uses never reaches the containers
around it: a text input takes Left and Right to move its caret. A launcher
whose search field keeps the focus while the arrows move a selection needs
those keys before the field sees them. `on_key_down_capture` hears a key on its
way down to the focused widget instead of on its way back up:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let query = create_signal(String::new());
let selected = create_signal(0usize);
container()
    .on_key_down_capture(move |key, _mods, _repeat| match key {
        Key::Left => {
            selected.update(|s| *s = s.saturating_sub(1));
            EventResponse::Handled
        }
        Key::Right => {
            selected.update(|s| *s += 1);
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    })
    .child(text_input(query).autofocus())
# ;
# }
```

Capture listeners hear the key from the outermost container in. `Handled`
stops it there: the field never sees it, no `on_key_down` hears it, and a Tab
taken this way does not move the focus. `Ignored` lets it go on to the next
capture listener inside, then the focused widget, then the `on_key_down`
listeners on the way back up — so the characters above still type, and Up and
Down, which the field lets through, still reach an `on_key_down` on the same
container. A focused container hears its own capture before its own
`on_key_down`.

With nothing focused, a key goes to the containers listening for it as
before, and a capture listener hears it on the way to them, before the
listeners inside it. `on_key_up_capture` does the same for releases.

## Moving the Focus with Tab

Tab moves the keyboard focus to the next text input on the surface, and
Shift+Tab to the one before. A container joins them by declaring
`focusable(true)`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let open = move || {};
container()
    .focusable(true)
    .when_focused(|s| s.outline(2.0, Color::WHITE, 2.0))
    .on_key_down(move |key, _mods, _repeat| {
        if key == Key::Enter {
            open();
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    })
    .child(text("Settings"))
# ;
# }
```

Tab stops at it, a press inside it takes the focus — as `tabindex="0"` does on
the web — and with the focus there its `when_focused` lights up and its
`on_key_down` hears the keys first. The press is not consumed: a clickable row
around it still gets its click. A press that something inside it took — a
button in the card — stays that button's, and the focus does not move. A container that is not focusable can still be
given the focus by a [widget ref](../advanced/widget-ref.md#moving-the-keyboard);
it is only not a place Tab stops.

Moving the focus is what a Tab does when nothing took it: the
focused widget hears it first, then the containers around it, then the
listeners, and only a Tab all of them let through moves the focus. A widget
that wants Tab for itself — an editor inserting a tab character — answers
`Handled`, and the focus stays where it is.

The order is reading order: top to bottom by where each stop was laid out,
then left to right, as GTK orders it. It follows what is on screen, so a list
reordered by key is walked in the order it is shown. At the last stop Tab goes
round to the first, and Shift+Tab at the first to the last. With nothing
focused, Tab takes the first and Shift+Tab the last.

A stop inside a hidden or disabled container is passed over, and so is one
playing its exit. Tab stays on the surface holding the keyboard: in a popup, it
goes round the popup's own stops.

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

    /// Handle the second click of a double click, on its release
    pub fn on_double_click(self, handler: impl Fn() + 'static) -> Self;

    /// Handle hover state changes
    pub fn on_hover(self, handler: impl Fn(bool) + 'static) -> Self;

    /// Handle a key press, or its release; each answers whether it took it
    pub fn on_key_down(
        self,
        handler: impl Fn(Key, Modifiers, bool) -> EventResponse + 'static
    ) -> Self;
    pub fn on_key_up(
        self,
        handler: impl Fn(Key, Modifiers) -> EventResponse + 'static
    ) -> Self;

    /// The same, heard before the focused widget inside
    pub fn on_key_down_capture(
        self,
        handler: impl Fn(Key, Modifiers, bool) -> EventResponse + 'static
    ) -> Self;
    pub fn on_key_up_capture(
        self,
        handler: impl Fn(Key, Modifiers) -> EventResponse + 'static
    ) -> Self;

    /// Make Tab stop here, and a press inside take the focus
    pub fn focusable(self, focusable: bool) -> Self;

    /// Handle scroll events
    pub fn on_scroll(
        self,
        handler: impl Fn(f32, f32, ScrollSource) + 'static
    ) -> Self;

    /// The pointer's shape over this container
    pub fn cursor<M>(self, cursor: impl IntoSignal<CursorIcon, M>) -> Self;
}
```
