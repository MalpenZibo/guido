# Event System

This page explains how input events flow through Guido.

## Event Flow

```text
Wayland → Platform → App → Widget Tree
                              │
                              ├─ MouseMove
                              ├─ MouseEnter/MouseLeave
                              ├─ MouseDown/MouseUp
                              └─ Scroll/ScrollEnd
```

## Event Types

### Mouse Movement

```text
Event::MouseMove { at, pointer };
Event::MouseEnter { at };
Event::MouseLeave
```

Tracked for hover states. The platform layer determines which widget the cursor is over.

### Mouse Buttons

```text
Event::MouseDown { at, button, pointer };
Event::MouseUp { at, button }
```

Used for click detection and pressed states.

### Which Pointer

A move and a press carry a `PointerKind` — `Mouse` or `Finger`. Touch is folded
into the pointer pipeline, so a finger arrives as an ordinary `MouseDown` and
every widget that wants to treat the two alike gets that for free.

One field, not a parallel touch event family: `Event::Scroll` carries a
`ScrollSource` on the same principle. Almost nothing reads it. What does is a
scroller, because a finger dragging its content scrolls it and a mouse doing
the same must go on selecting text — see
[Scrolling](../concepts/container.md#scrolling).

### Scrolling

```text
Event::Scroll { at, delta_x, delta_y, source };
Event::ScrollEnd { at }
```

- `at` - Pointer position, or `None`: see [Where an event is](#where-an-event-is)
- `delta_x` - Horizontal scroll amount, in pixels
- `delta_y` - Vertical scroll amount, in pixels
- `source` - `Wheel`, `Finger` or `Continuous`

`ScrollEnd` is the end of a scroll gesture — the finger lifting off a touchpad,
which the compositor reports as `wl_pointer.axis_stop`. It carries no delta
because nothing moved.

Only `Finger` is guaranteed to produce one. The protocol says a `wl_pointer`
sequence from a `Wheel` or `Continuous` source *may or may not* be terminated,
and that clients "should treat scroll sequences from these scroll sources as
unterminated by default" — so a `Continuous` gesture may simply never end, and a
wheel may occasionally send a stop even though nothing was held down.

It is what decides when momentum scrolling may begin. Without it the end of a
gesture has to be guessed from a gap between samples, and a slow scroll is made
of gaps. A source that never terminates therefore never gets momentum, which is
the honest answer rather than a guess.

## Event Routing

No event is offered to every widget. Where each one goes is worked out
centrally, the way Flutter routes its events, and a widget's `event` is only
called for what was routed to it:

| Event | Reaches |
| --- | --- |
| A pointer event with a position — move, press, release, wheel, a scroll's end, enter | The widgets that can be under the point, and those the pointer record still owes one |
| A pointer event without one — the surface leave, or what a collapsed transform passes down | The widgets the pointer record owes one, and nothing else |
| A key, and the surface gaining or losing the keyboard | The focused widget, then each of its ancestors; a key press nobody there took then goes to the containers that declared `on_key_down`, the innermost first |
| A paste | The widget that asked for it |

*The widgets that can be under the point* are found the way paint finds what
it can see: a container whose children are ordered along an axis searches them
by their laid-out bounds, grown by how far any of them draws outside its box,
so a transformed child is found where it is drawn. Children in no order are
tested one by one against the same reach.

*The pointer record* keeps two sets of offers, as Flutter's `MouseTracker` and
`GestureBinding` do. The last event's offers make a widget the pointer just
left see the move that takes its hover away, however far the pointer went. The
press's offers, kept until the release, make a drag reach the widget it started
on wherever it goes. A mouse has one press route however many of its buttons
are down: the first button pressed decides it, it is kept until the last one is
released, and a press or release of another button in between goes along that
route rather than to whatever is under the pointer. A point a clipping
container clips away reaches only the record, and without its position — except
for the widget a press holds, which keeps it for its drag and its release.

Within a container the children are offered an event in the order they stand,
and the first to answer `Handled` stops it; if none did, the container handles
it itself. So the innermost widget hears an event first, and its ancestors
after it — with one exception: a scroller answers its own scrollbar, and a
content drag it has already won, before its children are asked.

## Hit Testing

### Basic Hit Test

```rust,ignore
fn contains(&self, x: f32, y: f32) -> bool {
    x >= self.x && x <= self.x + self.width &&
    y >= self.y && y <= self.y + self.height
}
```

### With Corner Radius

Clicks outside rounded corners don't register:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let radius = 8.0f32;
// SDF-based hit test
let dist = sdf_rounded_rect(point, bounds, radius, k);
dist <= 0.0  // Inside if distance is negative
# ;
# }
```

### With Transforms

The transform is undone before the point is compared against the laid-out
bounds — and a transform that has collapsed the widget onto a line has no
inverse to undo it with:

```rust,ignore
fn contains(&self, at: Option<Point>) -> bool {
    // `untransform_point` answers None where the transform has no inverse,
    // and `contains_at` answers false for a point that is not there.
    self.bounds.contains_at(untransform_point(&self.transform, at))
}
```

## Where an event is

A pointer event carries `at: Option<Point>`, and the `None` is the interesting
half. It means one of two things, and a consumer wants the same answer from
both: a keyboard or focus event never had a position, and a pointer event that
descended into a subtree scaled to nothing has *lost* the one it had.

`scale(0.0)` squashes the plane onto a line. The map stops being one-to-one —
every point in the box lands on top of every other — so there is no inverse,
and no coordinate that could stand for "nowhere": every number is somewhere,
and a descendant that rotates or mirrors would carry a far-away sentinel back
into the visible half-plane.

So the position is simply absent, and every bounds test below answers no:

```rust,ignore
container()
    .width(80.0)
    .height(40.0)
    .scale(Scale::new(1.0, 0.0))   // collapsed: takes no click, hover or scroll
    .on_click(|| unreachable!())
```

The event still *travels*. A press given up and a hover cleared are things only
a delivered event can do, so a container inside a menu that collapses mid-press
hears the release and drops its own pressed state — the state layer stops
painting, the ripple is cancelled. What it does *not* do is call `on_click` or
`on_mouse_up`: those say where the release landed, and it landed nowhere. An
application pairing `on_mouse_down` with `on_mouse_up` to track a press of its
own should expect the down without the up, and clear on the same signal that
collapsed the box.

A key or a focus change is untouched, having never had a position to lose.

## Event Handlers

Containers register callbacks:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_click(|| println!("Clicked!"))
    .on_hover(|hovered| println!("Hover: {}", hovered))
    .on_scroll(|dx, dy, source| println!("Scroll"))
# ;
# }
```

Internally stored as optional closures:

```rust,ignore
pub struct Container {
    on_click: Option<Box<dyn Fn()>>,
    on_hover: Option<Box<dyn Fn(bool)>>,
    on_scroll: Option<Box<dyn Fn(f32, f32, ScrollSource)>>,
}
```

## State Layer Integration

The state layer system uses events internally:

1. **MouseEnter** → Set hover state true
2. **MouseLeave** → Set hover state false, and pressed state false with the
   ripple *cancelled* rather than completed — a press that ends this way
   activated nothing, which is also how a cancelled touch gesture ends
3. **MouseDown** → Set pressed state true, record click point
4. **MouseUp** → Set pressed state false, trigger ripple contraction

```rust,ignore
fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
    match event {
        Event::MouseEnter { .. } => {
            self.flags.update(|f| f.insert(InteractionFlags::HOVERED));
        }
        Event::MouseDown { at: Some(at), .. } => {
            self.flags.update(|f| f.insert(InteractionFlags::PRESSED));
            // The event's own moment, not the wall clock: a ripple begins
            // when the press arrived, and `event_instant` is the only thing
            // that knows when that was. `as_frame_start` is the crossing —
            // the ripple then grows on frames.
            self.ripple.start(at.x, at.y, tree.event_instant().as_frame_start());
        }
        // ...
    }
    EventResponse::Ignored
}
```

The flags live in a signal rather than a plain field, so that a *descendant*
resolving a state layer subscribes to them — see
[Interactivity](../interactivity/README.md).

## EventResponse

Widgets return whether they handled the event:

```rust,ignore
pub enum EventResponse {
    Handled,   // Stop propagation
    Ignored,   // Continue to parent
}
```

## Platform Integration

### Wayland Events

The platform layer receives Wayland protocol events:

```rust,ignore
// From wl_pointer
fn pointer_motion(x: f32, y: f32) {
    self.cursor_x = x;
    self.cursor_y = y;
    self.dispatch(Event::MouseMove {
        at: Some(Point::new(x, y)),
        pointer: PointerKind::Mouse,
    });
}

fn pointer_button(button: u32, state: ButtonState) {
    match state {
        ButtonState::Pressed => self.dispatch(Event::MouseDown { ... }),
        ButtonState::Released => self.dispatch(Event::MouseUp { ... }),
    }
}
```

### Event Loop

Uses calloop for event loop integration:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Main loop
loop {
    // 1. Process Wayland events
    event_queue.dispatch_pending()?;

    // 2. Layout and paint
    widget.layout(constraints);
    widget.paint(&mut ctx);

    // 3. Render to screen
    renderer.render(&ctx);
}
# ;
# }
```

