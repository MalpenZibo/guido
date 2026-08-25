# Keyframes

Every other animation in guido moves *towards* something: a property gets a new
value and a transition carries it there. That is the whole of what a state
change looks like, and none of what a sequence looks like — a shake, a flash, a
bounce, anything that has to pass through somewhere on its way and end where it
started.

A timeline is the other shape. It has no target: it plays.

```rust
container()
    .keyframes_rotate(
        Keyframes::new(320.0)
            .at(0.0, 0.0)
            .at(0.15, 2.0)
            .at(0.40, -1.6)
            .at(0.65, 0.9)
            .at(1.0, 0.0),
        rejections,
    )
```

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
Keyframes::new(360.0)
    .at_with(0.0, Translate::NONE, TimingFunction::EaseIn)
    .at_with(0.35, Translate::new(0.0, 14.0), TimingFunction::EaseOut)
    .at(1.0, Translate::NONE)
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

```rust
container()
    .when_hovered(|s| s.scale(1.03))
    .animate_scale(Transition::spring(SpringConfig::SNAPPY))
    .keyframes_rotate(shake(), rejections)
```

The hover is on `scale` and the shake on `rotate`, so both are drawn throughout:
the card grows under the pointer while it is still shaking its head.

The replacing rule applies when a timeline and a declaration are on **the same**
component:

```rust
container()
    .when_hovered(|s| s.rotate(3.0))
    .animate_rotate(Transition::spring(SpringConfig::SNAPPY))
    .keyframes_rotate(shake(), rejections)
```

Here the hover tilt is still declared while the shake runs; it simply is not
being drawn. When the sequence ends the property is handed back **through its
declared transition**, from wherever the sequence left it — so a pointer that
arrived mid-shake gets its spring, rather than the card jumping to the tilted
angle on the sequence's last frame. A callback on that transition fires as it
normally would, for the same reason.

The builders do not care which order you write them in: declaring a transition
after a sequence keeps the sequence, and the other way round too.

## What a segment cannot be eased with

A spring. A keyframe segment is a fixed slice of a fixed duration, which is
exactly what a spring has not got — it settles when it settles. Passing one to
`at_with` substitutes `EaseInOut` and says so in debug builds, rather than
quietly playing the segment as a straight line.

```rust
Keyframes::new(320.0)
    .at_with(0.0, 0.0, TimingFunction::EaseOut)
    .at(1.0, 2.0)
```

## When not to use them

A timeline is a choreography, and most motion is not. If what you want is
"go there, springily", a spring already does it — and since a spring keeps its
momentum when it is retargeted, out-and-back is a wobble without a timeline at
all. Reach for keyframes when the shape of the motion is the point, not the
destination.

`cargo run --example keyframes_example` plays both a shake and a nod from the
same trigger, over a hover that stays declared throughout.
