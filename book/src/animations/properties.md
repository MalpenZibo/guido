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

A border is declared as a pair, and each half carries its own timing:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(1.0.transition(150.0), Color::rgb(0.3, 0.3, 0.4))
    // A border is declared as a pair, so the layer restates the colour it is
    // not changing — otherwise the width eases while the colour jumps.
    .when_hovered(|s| s.border(2.0, Color::rgb(0.3, 0.3, 0.4)))
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
change colour as it grows, or slide from below a card to beside it.

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

An easing `font_size` re-measures the text on every frame it moves, which
reflows whatever contains it — worth a shorter duration than a colour, and worth
avoiding in a long list.

A state override supplies a value and never a timing, here as everywhere:
`when_hovered(|s| s.color(..))` takes a colour, and a transition on it is a
compile error rather than something quietly ignored.

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
    .border(1.0.transition(150.0), Color::rgb(0.3, 0.3, 0.4).transition(150.0))
    .shadow(LOW.transition(250.0))
    .scale(Scale::NONE.transition(Transition::spring(SpringConfig::GENTLE)))

    .when_hovered(|s| s
        .lighter(0.1)
        .border(2.0, Color::WHITE)
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
| Corners | `corners(..)` | Duration |
| Padding | `padding(..)` | Duration |
| Translate | `translate(..)` | Spring or Duration |
| Rotate | `rotate(..)` | Spring or Duration |
| Scale | `scale(..)` | Spring or Duration |
| Width, height | `width(..)`, `height(..)` | Spring |
| Shadow | `shadow(..)` | Duration, EaseOut |
| Text colour | `color(..)` on a `text` or `text_input` | Duration, EaseOut |
| Font size | `font_size(..)` on a `text` or `text_input` | Duration |

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
/// On everything a property setter accepts — a value, a signal, a closure.
pub trait Animate<T, M>: IntoSignal<T, M> + Sized {
    fn transition(self, transition: impl Into<TransitionConfig>) -> Animated<T>;
    fn timeline<M2>(self, keyframes: Keyframes<T>, plays: impl IntoSignal<u32, M2>) -> Animated<T>
    where
        T: Animatable;
}
```
