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

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let show_extra = create_signal(false);

container().children([
    text("Always shown"),
    container().maybe_child(show_extra, || text("Sometimes shown")),
])
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
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .layout(
        Flex::row()
            .spacing(8.0)
            .main_alignment(MainAlignment::Center)
            .cross_alignment(CrossAlignment::Center)
    )
    .children([...])
# ;
# }
```

See [Layout](layout.md) for details on flex layouts.

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

Control whether a container is visible. When hidden, it takes up no space in layout, does not paint, and ignores all events.

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

Unlike `.maybe_child()` which adds or removes a child from the tree, `.visible()` keeps the widget in the tree but hides it completely. This is useful when you want to toggle visibility without recreating the widget and its state.

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

```rust,ignore
fn create_button(label: &str, on_click: impl Fn() + 'static) -> Container {
    container()
        // Layout
        .padding(16.0)

        // Styling
        .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
        .corners(8.0)
        .border(1.0.transition(150.0), Color::rgb(0.4, 0.6, 0.9))

        // State layers
        .when_hovered(|s| s.lighter(0.1).border(2.0, Color::rgb(0.5, 0.7, 1.0)))
        .when_pressed(|s| s.ripple().darker(0.05).scale(0.98))

        // Event
        .on_click(on_click)

        // Content
        .child(container().child(text(label).color(Color::WHITE)))
}
```

## Builder Methods Reference

### Children
- `.child(widget)` - Add single child
- `.children([...])` - Add multiple children
- `.maybe_child(Option<widget>)` - Conditional child
- `.child(move || ..)` - Reactive child, rebuilt when a signal it read changes
- `.children(keyed(items, key_fn, view_fn))` - Keyed reactive list

### Styling
- `.background(color)` - Solid background
- `.gradient(LinearGradient::horizontal(start, end))` - Horizontal gradient
- `.gradient(LinearGradient::vertical(start, end))` / `.gradient(LinearGradient::diagonal(start, end))`
- `.corners(8.0)` / `.corners([16.0, 0.0])` - Rounded corners: one, two or four values
- `.corners(Corners::squircle(12.0))` / `Corners::bevel(..)` / `Corners::scoop(..)` -
  the shape of the corner
- `.border(width, color)` - Border
- `.elevation(level)` - Shadow

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
- `value.timeline(keyframes, plays)` - play a sequence on a trigger

### Visibility
- `.visible(condition)` - Show or hide the container (accepts static, signal, or closure)

### Scrolling
- `.scroll(Scroll::vertical() | Scroll::horizontal() | Scroll::both())` - Scroll,
  and say what the scrollbar looks like. One value: the parts cannot be declared
  apart from the thing they configure
- `Scroll::width`, `hover_width`, `margin`, `min_handle_size`, `reserve_gutter` /
  `overlay()`, `visibility(..)` - the measurements the container's layout needs,
  each taking a static value, a signal or a closure
- `Scroll::track(|t| ..)` and `Scroll::handle(|h| ..)` - the scrollbar is two
  containers, so these take everything a container takes
