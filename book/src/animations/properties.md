# Animatable Properties

Every property on this page takes a motion the same way: `.transition(..)` or
`.timeline(..)` on the value it is being set to.

## Background

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3).transition(200.0))
    .when_hovered(|s| s.lighter(0.1))
# ;
# }
```

Works with:
- Solid colors
- State layer overrides (lighter, darker, explicit)

## Border

A border is declared as a pair, and each half carries its own timing. Its width
takes room inside the box, so each frame of a width transition lays the
container out and the content moves with the border:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let thick = create_signal(false);
container()
    .border(
        (move || if thick.get() { 4.0 } else { 1.0 }).transition(150.0),
        Color::rgb(0.3, 0.3, 0.4),
    )
# ;
# }
```

The mirror image, where only the colour moves:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(2.0, Color::rgb(0.3, 0.3, 0.4).transition(150.0))
    .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.7, 1.0)))
# ;
# }
```

Or both at once, on curves of their own — a width that springs while its colour
eases is what one call could never express:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(
        2.0.transition(Transition::spring(SpringConfig::BOUNCY)),
        Color::rgb(0.3, 0.3, 0.4).transition(300.0),
    )
# ;
# }
```

## Transform

Translation, rotation and scale each animate on their own curve. There is no
call that animates the three together, and declaring one says nothing about the
other two:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .scale(Scale::NONE.transition(Transition::new(300.0, TimingFunction::EaseOut)))
    .when_pressed(|s| s.scale(0.98))
# ;
# }
```

Spring animations are especially good for transforms:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.scale(Scale::NONE.transition(Transition::spring(SpringConfig::BOUNCY)))
# ;
# }
```

## Opacity

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let shown = create_signal(true);
container()
    .opacity((move || if shown.get() { 1.0 } else { 0.0 }).transition(200.0))
# ;
# }
```

An opacity fades the container and everything in it, which is what an enter
wants — a panel that fades in the one time it appears:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().opacity(1.0.transition(200.0).entering_from(0.0))
# ;
# }
```

It is paint-only, like a transform: nothing around the container moves, and a
container faded to nothing still takes clicks. See
[Opacity](../building-ui/styling.md#opacity) for what a fade does where children
overlap.

A fade is cheap however much is inside it. A container whose children overlap
is drawn once into a texture, and each frame of the fade lays that texture down
again at the new opacity; nothing inside is painted or drawn again until it
changes. One that moves by whole pixels keeps its texture too; one that moves
to a new fraction of a pixel is drawn again there, so it stays as sharp as it
would be without the fade. One that turns or scales keeps its texture and is
resampled while it does.

## Blur

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().blur(0.0.transition(200.0).entering_from(8.0).exiting_to(8.0))
# ;
# }
```

A view that arrives out of focus and sharpens — and leaves the way it came.
Each frame blurs the texture the container was drawn into once, rather than
drawing the container again. See [Blur](../building-ui/styling.md#blur).

## Width and Height

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let expanded = create_signal(false);

container()
    .width(
        (move || if expanded.get() { 400.0 } else { 200.0 })
            .transition(Transition::spring(SpringConfig::DEFAULT)),
    )
# ;
# }
```

A size follows the content it holds as well as the length declared here, so the
animation is over the resolved extent. These are the one pair that cannot carry
a timeline: they declare a `Length`, which is not an animatable value in itself.

## Position

What layout moves — a row a reorder sends elsewhere, a panel a sibling pushes
down — jumps there unless it asks to slide:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let rows = create_signal(vec![1u32, 2, 3]);

container().layout(Flex::column()).children(keyed(
    move || rows.get(),
    |id| *id,
    |id| {
        container()
            .animate_position(SpringConfig::SNAPPY)
            .child(text(format!("Row {id}")))
    },
))
# ;
# }
```

The row is laid out in its new place at once, and drawn where it was, sliding
over to it; nothing re-lays out while it does. It is measured inside its parent:
scrolling slides nothing, and a parent that moves carries its children without
sliding them. A move during a move starts from where the row is drawn, and a
spring keeps its speed. The first layout places it without a slide — an
entrance is `entering_from`'s.

A size that changes is the width and height's own business, so something that
moves *and* grows takes both: `.animate_position(..)` beside
`.width(w.transition(..))`.

## Corners and Padding

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let open = create_signal(false);
container()
    .corners((move || if open.get() { 30.0 } else { 6.0 }).transition(250.0))
    .padding(8.0.transition(200.0))
# ;
# }
```

A corner transition that crosses zero curvature changes family in one frame:
below zero a corner is concave and a different formula draws it. Within a family
it is continuous.

## Shadow

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
container()
    .shadow(LOW.transition(200.0))
    .when_hovered(|s| s.shadow(RAISED))
# ;
# }
```

All four fields interpolate — offset, blur, spread and colour — so a shadow can
change colour as it grows, or slide from below a card to beside it. A list of
shadows interpolates shadow by shadow, the shorter padded with shadows that draw
nothing, so one arriving fades in. An inset shadow eases into another inset one;
between an inset and an outer one there is nothing to draw halfway, and the
list steps at the midpoint.

A timeline is written over `Shadows`, the type the property holds, so each
stop converts with `.into()`:

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
container().shadow(Shadows::from(LOW).timeline(
    Keyframes::new(600.0)
        .at(0.0, LOW.into())
        .at(0.5, RAISED.into())
        .at(1.0, LOW.into()),
))
# ;
# }
```

A shadow falls outside the box that casts it, so the container has to tell the
layout how far its painting reaches — and the reach has to cover the deepest
shadow it can ever cast, since a hover never re-runs layout.

That makes the two ways of lifting a card cost different amounts:

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
# let lifted = create_signal(false);
# container()
// Constants: the deepest is RAISED whatever happens, so hovering moves only the
// paint and nothing is re-laid-out.
.shadow(LOW).when_hovered(|s| s.shadow(RAISED))

// A signal: the deepest genuinely changes when it is written, so every write
// re-runs the layout of this subtree.
.shadow(move || if lifted.get() { RAISED } else { LOW })
# ;
# }
```

Both are correct, and the second is the one to reach for when the depth is
driven by something other than pointer state. Prefer the state layer when it can
say the same thing.

## Text Colour and Size

Declared on the widget that draws the glyphs — a `text` or a `text_input` —
because that is where text style is declared at all:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let busy = create_signal(false);
text("saving…").color(
    (move || if busy.get() { Color::rgb(0.9, 0.6, 0.2) } else { Color::WHITE })
        .transition(200.0),
)
# ;
# }
```

`color` and `font_size` are the two that move. A family and a weight snap to an
installed face, and a stroke and a shadow are records with nothing to
interpolate, so those four take values only.

Both take a `timeline` as readily as a `transition` — a label that flashes on an
alert, a size that bounces once when a value changes:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let alerts = create_signal(0u32);
text("disk full").color(Color::WHITE.timeline(
    Keyframes::new(600.0)
        .at(0.0, Color::WHITE)
        .at(0.5, Color::rgb(0.9, 0.2, 0.2))
        .at(1.0, Color::WHITE)
        .played_by(alerts),
))
# ;
# }
```

An easing `font_size` re-measures the text on every frame it moves, which
reflows whatever contains it — worth a shorter duration than a colour, and worth
avoiding in a long list.

A state override supplies a value and never a timing, here as everywhere:
`when_hovered(|s| s.color(..))` takes a colour, and a transition on it is a
compile error rather than something quietly ignored.

## Arc Progress and Colour

An [`arc()`](../building-ui/arcs-and-paths.md) moves its `progress` and its
`color` the same way, and nothing else of it moves:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let level = create_signal(40.0_f32);
arc()
    .size(24.0)
    .progress((move || level.get() / 100.0).transition(800.0))
    .color(
        (move || if level.get() < 20.0 { Color::rgb(0.9, 0.3, 0.3) } else { Color::WHITE })
            .transition(200.0),
    )
# ;
# }
```

An easing progress redraws one shape a frame and builds nothing: the arc is
drawn exactly rather than cut into triangles.

## Multiple Animations

Each property carries its own, where it is declared:

```rust
# extern crate guido;
# use guido::prelude::*;
# const FLAT: Shadow = Shadow::none();
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3).transition(200.0))
    .outline(2.0, Color::TRANSPARENT.transition(150.0), 2.0)
    .shadow(LOW.transition(250.0))
    .scale(Scale::NONE.transition(Transition::spring(SpringConfig::GENTLE)))

    .when_hovered(|s| s
        .lighter(0.1)
        .outline(2.0, Color::WHITE, 2.0)
        .shadow(RAISED)
    )
    .when_pressed(|s| s
        .scale(0.98)
        .shadow(FLAT)
    )
# ;
# }
```

## Complete Reference

| Property | Declared on | Recommended Transition |
|----------|-------------|----------------------|
| Background | `background(..)` | Duration, EaseOut |
| Border width | `border(width, _)` | Duration, EaseOut |
| Border colour | `border(_, colour)` | Duration, EaseOut |
| Outline width, colour, offset | `outline(width, colour, offset)` | Duration, EaseOut |
| Corners | `corners(..)` | Duration |
| Padding | `padding(..)` | Duration |
| Translate | `translate(..)` | Spring or Duration |
| Rotate | `rotate(..)` | Spring or Duration |
| Scale | `scale(..)` | Spring or Duration |
| Opacity | `opacity(..)` | Duration, EaseOut |
| Blur | `blur(..)` | Duration, EaseOut |
| Width, height | `width(..)`, `height(..)` | Spring |
| Position given by layout | `animate_position(..)` | Spring |
| Shadow | `shadow(..)` | Duration, EaseOut |
| Text colour | `color(..)` on a `text` or `text_input` | Duration, EaseOut |
| Font size | `font_size(..)` on a `text` or `text_input` | Duration |
| Arc progress, colour | `progress(..)`, `color(..)` on an `arc` | Duration, EaseOut |

## Best Practices

### Match Durations for Related Properties

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
// Same duration for border width and color
.border(1.0.transition(150.0), Color::WHITE.transition(150.0))
# ;
# }
```

### Use Springs for Physical Motion

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
// Spring for size/position changes
.width(200.0.transition(Transition::spring(SpringConfig::DEFAULT)))
.scale(Scale::NONE.transition(Transition::spring(SpringConfig::BOUNCY)))

// Duration for visual changes
.background(Color::WHITE.transition(200.0))
# ;
# }
```

### Keep Animations Subtle

- 150-300ms for most UI animations
- Avoid overly bouncy springs in professional UIs
- Let animations enhance, not distract

## API Reference

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
/// On everything a property setter accepts — a value, a signal, a closure.
pub trait Animate<T, M>: IntoSignal<T, M> + Sized {
    fn transition(self, transition: impl Into<TransitionConfig>) -> Animated<T>;
    fn timeline(self, keyframes: Keyframes<T>) -> Animated<T>
    where
        T: Animatable;
}
```
