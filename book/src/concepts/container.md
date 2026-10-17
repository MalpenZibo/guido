# Container

The Container is Guido's primary building block. Nearly everything you build uses containers - they handle layout, styling, events, and child management.

## Creating Containers

```rust
# extern crate guido;
# fn main() {
use guido::prelude::*;

let view = container();
# ;
# }
```

## Adding Children

### Single Child

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Hello"))
# ;
# }
```

### Multiple Children

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().children([
    text("First"),
    text("Second"),
    text("Third"),
])
# ;
# }
```

### Conditional Children

An `Option` is a child, so `child` takes one — absent when `None`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let show_extra = create_signal(false);

container()
    .child(text("Always shown"))
    .child(show_extra.get().then(|| text("Sometimes shown")))
# ;
# }
```

That reads the signal once, when the tree is built, so it never changes again.
For a child that comes and goes, pass a closure — it re-runs when a signal it
read changes:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let show_extra = create_signal(false);
container().child(move || show_extra.get().then(|| text("Sometimes shown")))
# ;
# }
```

## Styling

Containers support extensive styling options:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    // Background
    .background(Color::rgb(0.2, 0.2, 0.3))

    // Corners
    .corners(Corners::squircle(8.0))
     // iOS-style smooth corners

    // Border
    .border(2.0, Color::WHITE)

    // Spacing
    .padding(16.0)

    // Size
    .width(200.0)
    .height(100.0)
# ;
# }
```

See [Building UI](../building-ui/README.md) for complete styling reference.

### Backdrop Blur

Blur whatever is behind the container — both what this surface already
drew and, where the surface is translucent, what the compositor has
below it. Pair it with a translucent background so the result shows
through:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgba(0.12, 0.12, 0.18, 0.55)) // translucent
    .corners(16.0)
    .backdrop_blur(32.0)
# ;
# }
```

See [Wayland Layer Shell — Backdrop Blur](../advanced/wayland.md#backdrop-blur).

## Layout

Control how children are arranged:

```rust,ignore
# // not compiled: `[...]` stands in for the children; this block is about
# // the layout above them.
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .layout(Flex::row().spacing(8.0).center())
    .children([...])
# ;
# }
```

See [Layout](layout.md) for details on flex layouts and stacks.

## Event Handling

Respond to user interactions:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_click(|| println!("Clicked!"))
    .on_hover(|hovered| println!("Hover: {}", hovered))
    .on_scroll(|dx, dy, source| println!("Scroll: {}, {}", dx, dy))
# ;
# }
```

## State Layers

Add hover and pressed visual feedback:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
# ;
# }
```

See [Interactivity](../interactivity/README.md) for the full state layer API.

## Transforms

Apply 2D transformations:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    // The three compose, in this order whatever order they are written in
    .translate((10.0, 20.0))
    .rotate(45.0)
    .scale(1.5)
    .pivot(Pivot::TOP_LEFT)
# ;
# }
```

See [Transforms](../transforms/README.md) for details.

## Animations

The motion rides with the value: declare a property with a transition and it
eases to each new value instead of jumping there.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
    .scale(Scale::NONE.transition(Transition::spring(SpringConfig::BOUNCY)))
# ;
# }
```

See [Animations](../animations/README.md) for timing and spring options.

## Visibility

Control whether a container is visible. When hidden, it takes up no space in layout, does not paint, and ignores all events. Nothing inside it advances an animation either, so a spinner in a hidden panel costs no frames, and shown again it spins again. An exit still plays out, so that its widget can leave.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let tab = create_signal(String::from("settings"));
// Static
container().visible(false);

// Reactive signal
let show = create_signal(true);
container().visible(show);

// Reactive closure
container().visible(move || tab.get() == "settings")
# ;
# }
```

Unlike `.child(Option)`, which adds or removes a child from the tree, `.visible()` keeps the widget in the tree but hides it completely. This is useful when you want to toggle visibility without recreating the widget and its state.

## Scrolling

Make containers scrollable when content overflows:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn large_content() -> Container { container() }
# fn main() {
container()
    .width(200.0)
    .height(200.0)
    .scroll(Scroll::vertical())
    .child(large_content())
# ;
# }
```

### Scroll Axes

The axis is chosen by constructor, and it is the one part of a `Scroll` that
takes no signal — it decides what kind of widget this is, as `layout` and
`control` do.

| Constructor | Description |
|------|-------------|
| `Scroll::vertical()` | Vertical scrolling only |
| `Scroll::horizontal()` | Horizontal scrolling only |
| `Scroll::both()` | Both directions |

A container with no `.scroll(..)` does not scroll, which is what the old
`ScrollAxis::None` said.

### What Moves It

Three gestures, and which of them applies depends on what is pointing:

| Gesture | Mouse | Finger |
|------|------|------|
| Wheel or touchpad over the content | yes | — |
| Dragging the scrollbar handle, or clicking its track | yes | yes |
| Dragging the **content** | no | yes |

The last row is the touch gesture, and the pointer kind is why it is one row
and not two. A finger drags the content because grabbing a scrollbar is the one
gesture a touch interface never uses; a mouse does not, because dragging
content with a mouse is how text is selected, and a scrollable that swallowed
the drag would take selection away inside itself.

A finger owns nothing until it has moved 18 logical pixels — the slop Flutter's
`kTouchSlop` settled on. Under that the press belongs to whatever it landed on,
so tapping a button inside a list still presses the button. Over it the press
is taken back: the child is told the press it saw is void, exactly as it is
when the pointer leaves without releasing, so its pressed state clears and its
`on_click` does not fire. The slop is spent once — the move that crosses it
scrolls the part of itself beyond it, so the content neither jumps by the
threshold nor loses the rest of a fast swipe.

Releasing mid-motion hands the content to the same momentum a touchpad flick
gets — it coasts and slows down. A finger landing on content that is still
coasting stops it, and that press is the scroller's: it stops the list rather
than pressing what is under it.

A scroller with nothing to scroll claims none of this, so a list that happens
to fit today still answers a tap the way it always did.

The keyboard moves it too, the nearest scroller around the focus — a scroller
that is `.focusable(true)` and holds the focus itself, or one with the focused
widget somewhere inside it:

| Key | Scrolls |
| --- | --- |
| Up / Down | one line, 40 pixels — what a wheel notch scrolls |
| Left / Right | one line along the horizontal axis |
| Page Up / Page Down | one viewport height |
| Home / End | to the start or the end of the vertical axis, or of the horizontal one on a `Scroll::horizontal()` scroller |

The key goes to the focus first, as every key does: the focused widget, then
the scroller's own `on_key_down`, and only what both let through scrolls. A
focused text input keeps Left, Right, Home and End for its caret and lets Up
and Down through, and a list that moves a selection with the arrows takes
them in its own `on_key_down`. A scroller that cannot move that way — at its
end, or with no axis for the key, as a page on a `Scroll::horizontal()`
scroller — lets the key go on, so the next scroller out moves instead, as a
browser's do. With Ctrl, Alt or Super held, the key is left for whoever binds
it.

With nothing focused, nothing scrolls. Clicking into a list that is not
focusable gives nothing the focus, so a list that should scroll after a click
is made focusable:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn large_content() -> Container { container() }
# fn main() {
container()
    .height(300.0)
    .focusable(true)
    .scroll(Scroll::vertical())
    .child(large_content())
# ;
# }
```

Nested scrollers go to the innermost one that can move: a finger in a list
inside a page drags the list, and the page only where the list is not — or
where it has nothing to scroll on that axis. The wheel has always gone to the
innermost, and this is the same rule for a finger. A list that reaches its end
mid-drag keeps the gesture rather than passing what is left of it outward, as
Flutter's nested scrollables do; CSS calls the other behaviour scroll chaining,
and there is no `overscroll-behavior` here to ask for it.

### Filling the Viewport

A scrolled axis has no end, so a `fill()` child of the content has nothing to
fill there and is as big as its own content. `fill_viewport` gives the content
the visible size as a minimum, and a fill child then takes what the rest of the
content leaves of it — a footer that sits at the bottom of a short list:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .height(300)
    .scroll(Scroll::vertical().fill_viewport(true))
    .child(text("a short list"))
    .child(container().height(fill()))
    .child(text("footer"))
# ;
# }
```

Once the content is taller than the viewport the minimum is met, the fill child
is back to its content size, and the content scrolls. Content nested a level
down has to fill too — `container().height(fill())` around the column — since
a child that does not fill is as big as it is, minimum or not. Android calls
this `fillViewport`.

### Custom Scrollbars

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .scroll(
        Scroll::vertical()
            .width(6.0)
            .handle(|h| h
                .background(Color::rgb(0.4, 0.6, 0.9))
                .corners(3.0)
                .when_hovered(|s| s.background(Color::rgb(0.5, 0.7, 1.0)))),
    )
# ;
# }
```

### Hidden Scrollbars

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .scroll(Scroll::vertical().visibility(ScrollbarVisibility::Hidden))
# ;
# }
```

## Complete Example

Here's a fully-styled interactive button:

```rust
# extern crate guido;
# use guido::prelude::*;
fn create_button(label: &str, on_click: impl Fn() + 'static) -> Container {
    container()
        // Layout
        .padding(16.0)

        // Styling
        .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
        .corners(8.0)
        .border(1.0, Color::rgb(0.4, 0.6, 0.9))
        .outline(2.0, Color::TRANSPARENT.transition(150.0), 2.0)

        // State layers
        .when_hovered(|s| s.lighter(0.1).outline(2.0, Color::rgb(0.5, 0.7, 1.0), 2.0))
        .when_pressed(|s| s.ripple().darker(0.05).scale(0.98))

        // Event
        .on_click(on_click)

        // Content
        .child(container().child(text(label).color(Color::WHITE)))
}
# fn main() {}
```

## Builder Methods Reference

### Children
- `.child(widget)` - Add single child
- `.children([...])` - Add multiple children
- `.child(Option<widget>)` - Conditional child, absent when `None`
- `.child(move || ..)` - Reactive child, rebuilt when a signal it read changes
- `.children(keyed(items, key_fn, view_fn))` - Keyed reactive list

### Styling
- `.background(color)` - Solid background
- `.gradient(LinearGradient::horizontal(start, end))` - Horizontal gradient
- `.gradient(LinearGradient::vertical(start, end))` / `.gradient(LinearGradient::diagonal(start, end))`
- `.corners(8.0)` / `.corners([16.0, 0.0])` - Rounded corners: one, two or four values
- `.corners(Corners::squircle(12.0))` / `Corners::bevel(..)` / `Corners::scoop(..)` -
  the shape of the corner
- `.border(width, color)` - Border, inside the box: it takes room, as CSS's `border-box`
- `.outline(width, color, offset)` - A ring outside the box that takes no room: the focus or hover ring
- `.shadow(shadow)` - Shadows: one `Shadow` or up to four, each an offset, blur, spread and colour, outside the box or `inset`

### Spacing
- `.padding(all)` - Uniform padding
- `.padding([v, h])` - CSS two-value shorthand
- `.padding([t, r, b, l])` - CSS four-value shorthand

### Sizing
- `.width(w)` / `.height(h)` - Fixed size
- `.width(at_least(w))` / `.width(at_most(w))` - Bounded width
- `.height(at_least(h).at_most(h2))` - Bounded height

### Layout
- `.layout(Flex::row())` - Horizontal layout
- `.layout(Flex::column())` - Vertical layout
- `.layout(ZStack::new().align(Alignment::CENTER))` - Children stacked, centred
- `.align_self(Alignment::TOP_END)` - Where this one sits in a `ZStack`

### Events
- `.on_click(handler)` - Click events
- `.on_hover(handler)` - Hover enter/leave
- `.on_scroll(handler)` - Scroll events

### State Layers
- `.when_hovered(|s| s...)` - Hover overrides
- `.when_pressed(|s| s...)` - Pressed overrides

### Transforms
- `.translate((x, y))` - Move
- `.rotate(degrees)` - Rotate
- `.scale(factor)` - Scale
- `.pivot(origin)` - Pivot point

### Animations
Declared on the value, not beside it:
- `value.transition(ms)` - ease to each new value
- `value.timeline(keyframes.played_by(plays))` - play a sequence on a trigger

### Visibility
- `.visible(condition)` - Show or hide the container (accepts static, signal, or closure)

### Scrolling
- `.scroll(Scroll::vertical() | Scroll::horizontal() | Scroll::both())` - Scroll,
  and say what the scrollbar looks like. One value: the parts cannot be declared
  apart from the thing they configure
- `Scroll::width`, `hover_width`, `margin`, `min_handle_size`, `reserve_gutter` /
  `overlay()`, `visibility(..)`, `fill_viewport(..)` - the measurements the
  container's layout needs, each taking a static value, a signal or a closure.
  The bar is 6 wide and grows to 10 under the pointer; an undeclared
  `hover_width` keeps that proportion to `width`, so a widened bar still grows
- `Scroll::track(|t| ..)` and `Scroll::handle(|h| ..)` - the scrollbar is two
  containers, so these take everything a container takes
