# Keyframes

Every other animation in guido moves *towards* something: a property gets a new
value and a transition carries it there. That is the whole of what a state
change looks like, and none of what a sequence looks like — a shake, a flash, a
bounce, anything that has to pass through somewhere on its way and end where it
started.

A timeline is the other shape. It has no target: it plays.

```rust
container()
    .keyframes_transform(
        Keyframes::new(320.0)
            .at(0.0, Transform::IDENTITY)
            .at(0.15, Transform::rotate_degrees(2.0))
            .at(0.40, Transform::rotate_degrees(-1.6))
            .at(0.65, Transform::rotate_degrees(0.9))
            .at(1.0, Transform::IDENTITY),
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
    .at_with(0.0, Transform::IDENTITY, TimingFunction::EaseIn)
    .at_with(0.35, Transform::translate(0.0, 14.0), TimingFunction::EaseOut)
    .at(1.0, Transform::IDENTITY)
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

So a card can hover and shake without the two arguing:

```rust
container()
    .when_hovered(|s| s.transform(Transform::scale(1.03)))
    .animate_transform(Transition::spring(SpringConfig::SNAPPY))
    .keyframes_transform(shake(), rejections)
```

The hover is still declared while the shake runs; it simply is not being drawn,
and it has the property back the moment the sequence ends.

## When not to use them

A timeline is a choreography, and most motion is not. If what you want is
"go there, springily", a spring already does it — and since a spring keeps its
momentum when it is retargeted, out-and-back is a wobble without a timeline at
all. Reach for keyframes when the shape of the motion is the point, not the
destination.

`cargo run --example keyframes_example` plays both a shake and a nod from the
same trigger, over a hover that stays declared throughout.
