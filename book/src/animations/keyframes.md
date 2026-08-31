# Keyframes

Every other animation in guido moves *towards* something: a property gets a new
value and a transition carries it there. That is the whole of what a state
change looks like, and none of what a sequence looks like — a shake, a flash, a
bounce, anything that has to pass through somewhere on its way and end where it
started.

A timeline is the other shape. It has no target: it plays. Declare it with
`.timeline(..)`, on the value the property rests at between plays:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rejections = create_signal(0);
container()
    .rotate(0.0.timeline(
        Keyframes::new(320.0)
            .at(0.0, 0.0)
            .at(0.15, 2.0)
            .at(0.40, -1.6)
            .at(0.65, 0.9)
            .at(1.0, 0.0),
        rejections,
    ))
# ;
# }
```

It reaches every animatable property, not only the transform components — a
`Keyframes<Color>` on a background is a flash:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let errors = create_signal(0);
# let surface = Color::rgb(0.15, 0.15, 0.2);
container()
    .background(surface.timeline(
        Keyframes::new(240.0)
            .at(0.0, surface)
            .at(0.3, Color::rgb(0.8, 0.2, 0.2))
            .at(1.0, surface),
        errors,
    ))
# ;
# }
```

## The resting value

The value `.timeline(..)` is called on is what the property *is* whenever
nothing is playing. A shake returns to where it began, which makes it look
redundant — but a sequence that ends somewhere else snaps back to it, and a
sequence resting on a live signal has nowhere else to put its expression:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rejections = create_signal(0);
# let spin = create_signal(0u32);
# let shake = || Keyframes::new(320.0).at(0.0, 0.0).at(0.5, 2.0).at(1.0, 0.0);
// A glyph that spins on one signal, and shakes on another.
container()
    .rotate((move || spin.get() as f32 * 90.0).timeline(shake(), rejections))
# ;
# }
```

A timeline is for something that happens and is over. A change that should
persist is a transition.

## What plays it

The second argument is a signal, and **every change to it plays the sequence
once**. A count rather than a flag, because two refusals in a row are two
events and a signal that stays equal notifies nobody — the second wrong
password has to shake as loudly as the first. SwiftUI's keyframe animator takes
its trigger the same way.

Nothing plays on the first frame: the container remembers what the signal held
when it was built. Played again while it is still running, a sequence starts
over from the top rather than continuing — half a shake is not what a second
refusal means.

## Offsets and easing

An offset is a fraction of one run, `0.0` to `1.0`, and `duration` is how long
one run takes. The easing declared at a stop governs the segment that *starts*
there, which is CSS's rule for a timing function written inside a keyframe:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
Keyframes::new(360.0)
    .at_with(0.0, Translate::NONE, TimingFunction::EaseIn)
    .at_with(0.35, Translate::new(0.0, 14.0), TimingFunction::EaseOut)
    .at(1.0, Translate::NONE)
# ;
# }
```

Before the first stop and after the last one the nearest stop holds — a
timeline whose first stop is at `0.25` is not a timeline that starts undefined.
`repeat(n)` plays the whole run `n` times.

## What it does to the declared value

**A running timeline replaces it.** For as long as the sequence plays, it is
what the property is; when it ends, the property goes back to whatever is
declared *now* — including a value that changed while it was playing. This is
the rule CSS settled on, where an animation outranks a normal declaration for
as long as it runs and hands the property back afterwards.

A timeline belongs to the one component it moves, so a card can hover and shake
at the same time without the two meeting at all:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rejections = create_signal(0);
# let shake = move || {};
container()
    .when_hovered(|s| s.scale(1.03))
    .scale(Scale::NONE.transition(Transition::spring(SpringConfig::SNAPPY)))
    .rotate(0.0.timeline(shake(), rejections))
# ;
# }
```

The hover is on `scale` and the shake on `rotate`, so both are drawn throughout:
the card grows under the pointer while it is still shaking its head.

The replacing rule applies when a timeline and a declaration are on **the same**
component:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rejections = create_signal(0);
# let shake = move || {};
container()
    .when_hovered(|s| s.rotate(3.0))
    .rotate(0.0.timeline(shake(), rejections))
# ;
# }
```

Here the hover tilt is still declared while the shake runs; it simply is not
being drawn. When the sequence ends the property is handed back from wherever
the sequence left it, rather than the card jumping to the tilted angle on the
sequence's last frame.

A property carries one motion: a value is declared either with a transition or
with a timeline, and a second declaration of the same property replaces the
whole thing — the value included. Where a card wants both a spring and a
sequence, they go on two components, as in the example above.

## What a segment cannot be eased with

A spring. A keyframe segment is a fixed slice of a fixed duration, which is
exactly what a spring has not got — it settles when it settles. Passing one to
`at_with` substitutes `EaseInOut` and says so in debug builds, rather than
quietly playing the segment as a straight line.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
Keyframes::new(320.0)
    .at_with(0.0, 0.0, TimingFunction::EaseOut)
    .at(1.0, 2.0)
# ;
# }
```

## When not to use them

A timeline is a choreography, and most motion is not. If what you want is
"go there, springily", a spring already does it — and since a spring keeps its
momentum when it is retargeted, out-and-back is a wobble without a timeline at
all. Reach for keyframes when the shape of the motion is the point, not the
destination.

`cargo run --example keyframes_example` plays both a shake and a nod from the
same trigger, over a hover that stays declared throughout.
