# Styling Overview

This page provides a complete reference for all styling options available in Guido.

## Backgrounds

### Solid Color

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().background(Color::rgb(0.2, 0.2, 0.3))
# ;
# }
```

### Gradients

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Horizontal (left to right)
container().gradient(LinearGradient::horizontal(Color::RED, Color::BLUE));

// Vertical (top to bottom)
container().gradient(LinearGradient::vertical(Color::RED, Color::BLUE));

// Diagonal
container().gradient(LinearGradient::diagonal(Color::RED, Color::BLUE))
# ;
# }
```

## Corners

### Basic Radius

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().corners(8.0)  // 8px radius on all corners
# ;
# }
```

### Corner Curvature

Control corner shape using CSS K-values:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().corners(Corners::squircle(12.0));  // iOS-style (K=2)
container().corners(Corners::scoop(12.0));              // Circular (K=1, default)
container().corners(Corners::bevel(12.0));      // Diagonal (K=0)
container().corners(Corners::superellipse(12.0, 1.5));      // Concave (K=-1)
container().corners(12.0)  // Custom
# ;
# }
```

## Borders

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(2.0, Color::WHITE)  // Width and color
# ;
# }
```

A border takes room inside the box, as CSS's `box-sizing: border-box` does: the
children are laid out inside the border and the padding, and a box sized to its
content grows by both. A scroller's viewport and extents are measured inside its
border, so its content scrolls from the border's inner edge to the same edge at
the far end. The width is a layout property for that reason: changing it, from a
signal, a transition or a state layer, relays the container out and moves what
it holds. A ring that should not move anything is an
[outline](borders.md#outlines).

A container that scrolls or hides its overflow cuts its children *inside* its
border, as CSS cuts at the padding box: the border is never covered by what the
container holds, its scrollbar sits inside the border too, and a press on the
border does not reach a child under it. The container's own background, border
and shadow are not cut by that clip.

## Outlines

A ring outside the box, drawn above everything it holds and taking no room — the
focus or hover ring. See [Outlines](borders.md#outlines).

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .corners(8.0)
    .outline(2.0, Color::WHITE, 2.0)  // Width, colour, offset
# ;
# }
```

## Shadows

Four degrees of freedom: offset, blur, spread and colour. There is no elevation
level — a design system's ladder is a set of `Shadow` constants the application
writes down. See [Shadows](shadows.md).

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().shadow(Shadow::simple((0.0, 2.0), 4.0, Color::rgba(0.0, 0.0, 0.0, 0.16)));
container().shadow(Shadow::simple((0.0, 6.0), 10.0, Color::rgba(0.0, 0.0, 0.0, 0.22)));
container().shadow(Shadow::new((14.0, 0.0), 8.0, 4.0, Color::rgba(0.9, 0.2, 0.3, 0.5)))
# ;
# }
```

## Opacity

`opacity` fades a container and everything inside it — its background,
border and shadow, and its children's text, images and frost — from `1.0`, as
declared, down to `0.0`, invisible. Nested opacities multiply: a half inside a
half is a quarter.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .opacity(0.5)
    .child(text("half as strong"))
# ;
# }
```

The container fades as one picture, the way CSS's `opacity` does: where a
label sits on its own background, or a cover under a title, the overlap fades
with the rest instead of showing through. To get that, the container is drawn
into a texture of its own and laid down once — but only when two of the
things it draws actually overlap. Where nothing overlaps, fading each thing on
its own is the same picture, and that is what is drawn, for nothing.

`opacity_mode` chooses the other behaviour: every draw fades on its own, an
overlap shows through darker, and nothing is drawn offscreen.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .opacity(0.5)
    .opacity_mode(OpacityMode::Modulate)
    .child(text("the background shows through the letters"))
# ;
# }
```

A container with a backdrop blur in it fades draw by draw, whatever overlaps:
its frost keeps frosting what guido drew beneath it in the surface for the
whole of the fade. Faded as one picture, the frost would see only what the
container itself drew — CSS calls that a backdrop root — and snap back to the
surface when the fade ended. A blur the compositor runs behind the surface is
not affected either way.

A faded container still takes up its space and still takes input.

## Blur

`blur` softens everything a container draws, its children included, by a
radius in logical pixels — the radius a backdrop blur takes:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .blur(4.0)
    .child(text("out of focus"))
# ;
# }
```

A blur, like a CSS `filter`, always draws the container into a texture of its
own and blurs that. The texture covers what the container draws, its children
included, plus the reach of the blur, cut to the part that can be on the
surface. A radius of zero is drawn the same way and looks exactly like no
blur. A backdrop blur
inside a blurred container frosts only what the container drew beneath it. The blur spreads
past the container's edges by about one and a quarter of its radius. It
animates — see [Animated Properties](../animations/properties.md#blur).

## Padding

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().padding(16.0);              // All sides
container().padding(16);                // Integers work too
container().padding([8.0, 16.0]);       // [vertical, horizontal]
container().padding([8, 16]);           // Integer arrays too
container().padding([1.0, 2.0, 3.0, 4.0]);  // [top, right, bottom, left]
container().padding(Padding::all(8.0).with_top(20.0))  // Builder for one edge
# ;
# }
```

## Sizing

### Fixed Size

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .width(100.0)
    .height(50.0)
# ;
# }
```

Integers work too:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .width(100)
    .height(50)
# ;
# }
```

### Constraints

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .width(at_least(50.0).at_most(200.0))
    .height(at_least(30.0).at_most(100.0))
# ;
# }
```

### At Least / At Most

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().width(at_least(100.0));           // At least 100px
container().width(at_least(100));             // Integers work too
container().width(at_most(400));              // At most 400px
container().width(at_least(100).at_most(400)) // Range
# ;
# }
```

### Fraction of Available Space

`fraction(f)` takes a fraction (0.0..=1.0) of the space offered by the
parent, resolved at layout time. It is the natural tool for
value-proportional bars — sliders, gauges, progress — because the width
follows the value on the very first frame, with no measured-rect
round-trip:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// A slider fill bar at the current volume
let volume = create_signal(40);
container().width(move || fraction(volume.get() as f32 / 100.0))
# ;
# }
```

## Static or Reactive

Every property that survives to paint takes a signal, a closure, or a plain
value — the same `IntoSignal` shape everywhere, so which spelling you use is
never the API's decision:

```rust
# extern crate guido;
# use guido::prelude::*;
# const COOL: Color = Color::rgb(0.2, 0.4, 0.9);
# const HOT: Color = Color::rgb(0.9, 0.3, 0.2);
# #[derive(Clone, Copy)]
# struct Theme { text: Color, text_weak: Color, weak: Color, strong: Color, line: Color, accent: Color, error: Color, danger: Color, surface: Color }
# impl Default for Theme { fn default() -> Self { let c = Color::WHITE; Self { text: c, text_weak: c, weak: c, strong: c, line: c, accent: c, error: c, danger: c, surface: c } } }
# fn main() {
# let collapsed = create_signal(false);
# let frosted = create_signal(false);
# let hot = create_signal(false);
# #[derive(Clone, PartialEq)]
# struct Palette;
# impl Palette {
#     fn header(&self) -> LinearGradient {
#         LinearGradient::horizontal(Color::WHITE, Color::BLACK)
#     }
# }
# let palette = create_signal(Palette);
# let surface_color = create_signal(Color::rgb(0.1, 0.1, 0.15));
# let theme = Theme::default();
container()
    .background(theme.surface)                       // a constant
    .background(surface_color)                       // a signal
    .background(move || if hot.get() { HOT } else { COOL })   // a closure
    .gradient(move || palette.get().header())
    .backdrop_blur(move || if frosted.get() { 24.0 } else { 0.0 })
    .overflow(move || if collapsed.get() { Overflow::Hidden } else { Overflow::Visible })
# ;
# }
```

A blur radius of `0.0` is "no blur", on a container and on a text alike, which
is what lets one signal switch the effect on and off rather than forcing the
caller to rebuild the widget in a Rust branch. That holds wherever the surface
is one of the sources, which is everything by default. A container that
restricts itself to the compositor has no radius of its own — the protocol
carries none — so it asks on its sources alone, and empties them to switch off.

**What is not reactive** is structural: `.layout(..)`, the axis a `.scroll(..)`
is built with, `.control()`, and the motion a
value is declared with — `.transition(..)` and `.timeline(..)`. These say what
kind of thing the container *is*; change one and you are describing a different
widget, so declare it in the closure that builds the widget instead. The
*value* a motion decorates is as reactive as any other.

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
fn styled_card(title: &str, content: &str) -> Container {
    container()
        // Size and padding
        .width(300.0)
        .padding(20.0)

        // Background and corners — the background eases, so the hover below
        // arrives over 200ms rather than in one frame
        .background(Color::rgb(0.15, 0.15, 0.2).transition(200.0))
        .corners(Corners::squircle(12.0))
        

        // Border
        .border(1.0, Color::rgb(0.25, 0.25, 0.3))

        // Shadow
        .shadow(Shadow::simple((0.0, 4.0), 8.0, Color::rgba(0.0, 0.0, 0.0, 0.2)))

        // Layout
        .layout(Flex::column().spacing(12.0))

        // State layers — they supply values; the motion was declared above
        .when_hovered(|s| {
            s.lighter(0.05)
                .shadow(Shadow::simple((0.0, 6.0), 12.0, Color::rgba(0.0, 0.0, 0.0, 0.24)))
        })

        // Children
        .children([
            container().child(text(title).font_size(18.0).color(Color::WHITE)),
            container().child(text(content).font_size(14.0).color(Color::rgb(0.7, 0.7, 0.75))),
        ])
}
# fn main() {}
```
