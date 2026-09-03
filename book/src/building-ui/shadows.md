# Shadows

A shadow is four numbers: an offset, a blur, a spread and a colour. `Shadow`
carries all four, and `.shadow(..)` is the only way to ask for one.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().shadow(Shadow::new((0.0, 4.0), 8.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.2)));

// `simple` leaves the spread at zero, which is what most shadows want.
container().shadow(Shadow::simple((0.0, 2.0), 4.0, Color::rgba(0.0, 0.0, 0.0, 0.16)));

// And `none` is a shadow that draws nothing — the resting value to animate out of.
container().shadow(Shadow::none())
# ;
# }
```

The offset moves the shadow, and it moves in both axes: `(12.0, 0.0)` throws it
to the right, `(0.0, -6.0)` throws it upward. Blur is the softness of the edge.
Spread grows the shadow in every direction before it is blurred. The colour is a
colour, alpha included — a shadow does not have to be black.

## There is no elevation level

guido ships no `elevation` and no table behind one. That is deliberate. A
single number can only reach one of the four degrees of freedom, and the table
that expands it has to choose the other three on your behalf: always straight
down, always black, never any spread.

What a design system wants from elevation is *coordination* — that every card in
an application agrees about how high it sits. That is a set of constants, and it
belongs to the application. `Shadow::new`, `Shadow::simple` and `Shadow::none`
are all `const`, so a ladder costs nothing:

```rust
# extern crate guido;
mod elevation {
    use guido::prelude::{Color, Shadow};

    const fn step(offset_y: f32, blur: f32, alpha: f32) -> Shadow {
        Shadow::new((0.0, offset_y), blur, 0.0, Color::rgba(0.0, 0.0, 0.0, alpha))
    }

    pub const FLAT: Shadow = Shadow::none();
    pub const LOW: Shadow = step(1.0, 3.0, 0.12);
    pub const RAISED: Shadow = step(3.0, 6.0, 0.19);
    pub const HIGH: Shadow = step(6.0, 10.0, 0.22);
}
# fn main() {}
```

Then `container().shadow(elevation::RAISED)` reads the way `.elevation(3.0)`
used to, and the three shadows below it are still reachable.

## Shadows in state layers

A shadow is a paint property like any other, so a state layer overrides it:

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# const FLAT: Shadow = Shadow::none();
# fn main() {
container()
    .shadow(LOW)
    .when_hovered(|s| s.shadow(RAISED))   // lift on hover
    .when_pressed(|s| s.shadow(FLAT))     // press it back down
# ;
# }
```

## Motion

The timing rides with the declaration, never with the override — the same rule
every animatable property follows:

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
container()
    .shadow(LOW.transition(Transition::new(200.0, TimingFunction::EaseOut)))
    .when_hovered(|s| s.shadow(RAISED))
# ;
# }
```

All four fields interpolate, so a shadow can change colour as it grows, or slide
from below a card to beside it. `when_hovered(|s| s.shadow(RAISED.transition(..)))`
does not compile: a state layer supplies a value, and the declaration says how
it moves.

## Shadows and corners

The shadow follows the shape the container is drawn with, curvature included:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .corners(Corners::squircle(12.0))
    .shadow(Shadow::simple((0.0, 8.0), 16.0, Color::rgba(0.0, 0.0, 0.0, 0.22)))
# ;
# }
```

## A card, end to end

```rust,ignore
fn card(title: &str, body: &str) -> Container {
    container()
        .width(200.0)
        .padding(20.0)
        .background(Color::rgb(0.15, 0.15, 0.2).transition(150.0))
        .corners(12.0)
        .shadow(elevation::RAISED.transition(200.0))
        .when_hovered(|s| s.shadow(elevation::HIGH).lighter(0.05))
        .when_pressed(|s| s.shadow(elevation::LOW).darker(0.05))
        .layout(Flex::column().spacing(8.0))
        .children([
            container().child(text(title).font_size(18.0).color(Color::WHITE)),
            container().child(text(body).color(Color::rgb(0.7, 0.7, 0.75))),
        ])
}
```

`cargo run --example shadow_example` shows a ladder beside the three things one
number could never say: a shadow thrown sideways, a coloured one, and one with
spread.

## On dark backgrounds

A black shadow on a dark surface is nearly invisible. Two ways out, and they
compose: lighten the surface as it rises, or give the shadow a colour.

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# fn main() {
container()
    .background(Color::rgb(0.12, 0.12, 0.16))
    .shadow(LOW)
    .when_hovered(|s| {
        s.lighter(0.03)
            .shadow(Shadow::simple((0.0, 8.0), 20.0, Color::rgba(0.35, 0.4, 0.9, 0.45)))
    })
# ;
# }
```
