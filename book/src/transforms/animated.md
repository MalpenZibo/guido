# Animated Transforms

Animate transform changes with smooth transitions.

## Declaring one

The timing rides with the value: `.transition(..)` on whatever the component is
being set to, so a component cannot be animated without being set.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rotation_signal = create_signal(0.0f32);
container()
    .rotate(rotation_signal.transition(Transition::new(300.0, TimingFunction::EaseOut)))
# ;
# }
```

When `rotation_signal` changes, the transform animates smoothly.

## Appearing mid-animation

`transition` says how a value moves. `entering_from` says where it starts the
one time the widget appears — the CSS pair, where `transition` supplies the
motion and `@starting-style` supplies the before-value.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let open = create_signal(true);
# let collapsed = Scale::new(1.0, 0.0);
container()
    .scale(
        (move || if open.get() { Scale::NONE } else { collapsed })
            .transition(Transition::spring(SpringConfig::SNAPPY))
            .entering_from(collapsed),
    )
    .pivot(Pivot::TOP)
# ;
# }
```

A menu that scales open on its first layout, and closes by the same transition
running the other way.

It is not transform-shaped: the verb hangs off the value, so it reaches every
animatable property. A card can fade in.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.1, 0.1, 0.15).transition(200.0).entering_from(Color::TRANSPARENT))
# ;
# }
```

The enter plays once, at the first layout, and is consumed there: a relayout, a
resize or a state change is not an appearance and does not replay it. It needs a
transition to travel with, so declaring one on a timeline is a panic rather than
a value quietly ignored — a timeline already says where it starts.

## Leaving

`exiting_to` is the other half: where a property goes when its widget is
removed. A child of `.child(move || ..)`, `.children(move || ..)` or `keyed(..)`
that declares one is not torn down when it is removed. It stays where it stood,
travels to its exit with the transition already named, and is disposed when it
arrives.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let step = create_signal(1.0f32);
# let name = create_signal(String::from("default"));
# const WIDTH: f32 = 120.0;
# let slide = Transition::new(200.0, TimingFunction::EaseOut);
container().child(move || {
    container().child(text(name.get())).translate(
        Translate::NONE
            .transition(slide.clone())
            .entering_from(Translate::new(step.get_untracked() * WIDTH, 0.0))
            .exiting_to(move || Translate::new(-step.get() * WIDTH, 0.0)),
    )
})
# ;
# }
```

A picker that slides the old name out one way as the new one slides in from the
other. The exit is asked for when the child is removed, not when it was built,
which is why it takes a closure: the direction is only known when the change
that removes the child happens.

While it leaves, the child is inert. A click over it reaches whatever is under
it, the focus it held is released, and a signal it read no longer relays out or
repaints it — its item may already be gone. An effect created while building it
is not a widget, though: it lives until the child is disposed, and runs if what
it reads changes. In a `keyed(..)` list, a key that comes back while its row is
leaving gets that row back: the exit is cancelled and it travels home from
wherever it had got to. The row itself is not rebuilt; a `.child(move || ..)`
inside it is re-run, as it would be for any change it read.

Only the removed widget's own properties are waited on. An exit declared on
something inside it plays when *that* is the widget removed, and a container
removed with no exit of its own takes a leaving child with it at once.

## Duration-Based Animation

Standard easing curve transitions:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let rotation = create_signal(0.0f32);
// Smooth ease-out rotation
container()
    .rotate(rotation.transition(Transition::new(300.0, TimingFunction::EaseOut)))
    .on_click(move || rotation.update(|r| *r += 45.0))
# ;
# }
```

## Spring-Based Animation

Physics simulation for bouncy, natural motion:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let scale_signal = create_signal(1.0f32);
container()
    .scale(scale_signal.transition(Transition::spring(SpringConfig::BOUNCY)))
# ;
# }
```

Spring presets:
- `SpringConfig::DEFAULT` - Balanced
- `SpringConfig::GENTLE` - Slower and smooth, minimal overshoot
- `SpringConfig::SNAPPY` - Quickest response, subtle overshoot
- `SpringConfig::BOUNCY` - Energetic with visible bounce

## Examples

### Click to Rotate

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let rotation = create_signal(0.0f32);

container()
    .width(80.0)
    .height(80.0)
    .background(Color::rgb(0.3, 0.6, 0.8))
    .corners(8.0)
    .rotate(rotation.transition(Transition::new(300.0, TimingFunction::EaseOut)))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
    .on_click(move || rotation.update(|r| *r += 45.0))
# ;
# }
```

### Bouncy Scale Toggle

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let scale_factor = create_signal(1.0f32);
let is_scaled = create_signal(false);

container()
    .scale(scale_factor.transition(Transition::spring(SpringConfig::BOUNCY)))
    .on_click(move || {
        is_scaled.update(|s| *s = !*s);
        let target = if is_scaled.get() { 1.3 } else { 1.0 };
        scale_factor.set(target);
    })
# ;
# }
```

### Scale on Press (State Layer)

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .scale(Scale::NONE.transition(Transition::spring(SpringConfig::GENTLE)))
    .when_pressed(|s| s.scale(0.98))
# ;
# }
```

### Smooth Translation

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let offset_x = create_signal(0.0f32);

container()
    .translate(
        (move || (offset_x.get(), 0.0))
            .transition(Transition::new(400.0, TimingFunction::EaseInOut)),
    )
    .on_scroll(move |_, dy, _| {
        offset_x.update(|x| *x += dy * 10.0);
    })
# ;
# }
```

### Sliding by Its Own Width

A relative translate animates like a pixel one, and what animates is the
fraction. So a panel can slide in from exactly its own width without anybody
knowing the width — `entering_from` plays it on the first frame, before a pixel
of it has been measured:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .translate(
        Translate::NONE
            .transition(Transition::new(250.0, TimingFunction::EaseOut))
            .entering_from(Translate::relative(-1.0, 0.0)),
    )
# ;
# }
```

And a slide out is a signal holding where it should be:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let open = create_signal(true);

container()
    .translate(
        (move || if open.get() { Translate::NONE } else { Translate::relative(-1.0, 0.0) })
            .transition(Transition::new(250.0, TimingFunction::EaseInOut)),
    )
# ;
# }
```

If the width changes while it is out, it stays exactly one width out, at once:
the declared offset did not change, so there is nothing to animate.

## When to Use Each Type

### Duration-Based
- Rotation on click
- State layer transforms
- Predictable, controlled motion

### Spring-Based
- Scale effects that should feel physical
- Bounce-back effects
- Natural, dynamic interactions

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
fn animated_transforms_demo() -> impl Widget {
    let rotation = create_signal(0.0f32);
    let scale = create_signal(1.0f32);
    let is_scaled = create_signal(false);

    container()
        .layout(Flex::row().spacing(20.0))
        .padding(20.0)
        .children([
            // Duration-based rotation
            container()
                .width(80.0)
                .height(80.0)
                .background(Color::rgb(0.3, 0.5, 0.8))
                .corners(8.0)
                .rotate(rotation.transition(Transition::new(300.0, TimingFunction::EaseOut)))
                .when_hovered(|s| s.lighter(0.1))
                .when_pressed(|s| s.ripple())
                .on_click(move || rotation.update(|r| *r += 45.0))
                .layout(Flex::column().center())
                .child(container().child(text("Rotate").font_size(12.0).color(Color::WHITE))),

            // Spring-based scale
            container()
                .width(80.0)
                .height(80.0)
                .background(Color::rgb(0.3, 0.8, 0.4))
                .corners(8.0)
                .scale(scale.transition(Transition::spring(SpringConfig::BOUNCY)))
                .when_hovered(|s| s.lighter(0.1))
                .when_pressed(|s| s.ripple())
                .on_click(move || {
                    is_scaled.update(|s| *s = !*s);
                    scale.set(if is_scaled.get() { 1.3 } else { 1.0 });
                })
                .layout(Flex::column().center())
                .child(container().child(text("Scale").font_size(12.0).color(Color::WHITE))),
        ])
}
# fn main() {}
```

## API Reference

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
// The three components each take a value that may carry its own motion.
impl Container {
    pub fn translate<M>(self, t: impl IntoAnimated<Translate, M>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoAnimated<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoAnimated<Scale, M>) -> Self;
}

// Where a property starts the one time its widget appears.
Animated::entering_from(self, from: T) -> Animated<T>

// Where it goes when its widget is removed, asked at removal.
Animated::exiting_to(self, to: impl IntoSignal<T, M>) -> Animated<T>

// Duration-based
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition

// Spring-based
Transition::spring(config: SpringConfig) -> Transition
```
