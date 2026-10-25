# Transitions

A transition eases a property to each new value over a fixed time, with an
easing curve. It is declared on the value itself, with `.transition(..)`.

## Declaring one

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = Color::WHITE;
# let timing = TimingFunction::EaseOut;
# let duration_ms = 200.0;
# let _: Animated<Color> =
value.transition(Transition::new(duration_ms, timing))
# ;
# }
```

- `duration_ms` - Animation duration in milliseconds
- `timing` - Easing curve for the animation

A bare number is the short form: `.transition(200.0)` is 200ms eased out.

## Choosing the transition for each change

A panel that springs open and eases shut needs two transitions, and which one
plays is something only the application knows: it is opening or closing because
of its own state. So the transition can be read from that state, with a closure:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let open = create_signal(false);
container().height(
    (move || if open.get() { 300.0 } else { 0.0 })
        .transition(move || {
            if open.get() {
                Transition::spring(SpringConfig::BOUNCY)
            } else {
                Transition::new(200.0, TimingFunction::EaseOut).delay(100.0)
            }
        }),
)
# ;
# }
```

The closure is read **when a change starts**: when the value moves to a new
target, when an [`entering_from`](../transforms/animated.md) plays, and when an
`exiting_to` begins. By the time a widget is removed, the state that removed it
already says it is closing, so its exit takes the close. A transition that
changes while a change is running leaves it alone and applies from the next
one, which is what CSS does.

Sometimes no state knows the way a value is going. A box sized by its content
grows when the next content is wider, and only the layout finds that out. For
that case the closure can take the change itself — where the value is when it
starts, in flight or not, and where it is going:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().width(at_least(0.0).transition(|from: f32, to: f32| {
    let ease = Transition::new(200.0, TimingFunction::EaseOut);
    // grow after the new content has faded in; shrink at once
    if to > from { ease.delay(120.0) } else { ease }
}))
# ;
# }
```

A width and a height hand the closure the sizes the layout resolved, as `f32`,
whatever `Length` they were declared with.

## Examples

### Background Animation

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
    .when_hovered(|s| s.lighter(0.15))
# ;
# }
```

### Border and Outline Animation

A border is declared as a pair and each half carries its own timing. Its width
takes room inside the box, so a width that animates moves the content with it;
a hover ring is an outline, which takes none, and whose three halves animate
the same way:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4).transition(150.0))
    .outline(2.0, Color::TRANSPARENT.transition(150.0), 2.0)
    .when_hovered(|s| s
        .border(1.0, Color::rgb(0.5, 0.5, 0.6))
        .outline(2.0, Color::rgb(0.5, 0.5, 0.6), 2.0))
# ;
# }
```

### Transform Animation

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

The resting value is what the property is when no state layer overrides it —
here `Scale::NONE`, the size the press shrinks away from.

## Duration Guidelines

| Duration | Use Case |
|----------|----------|
| 100-150ms | Quick feedback (button press) |
| 150-200ms | State changes (hover) |
| 200-300ms | Content changes (expand/collapse) |
| 300-500ms | Major transitions (page changes) |

## Combining with State Layers

A state layer supplies a value for a property somebody else declared, and
carries no timing of its own. The property eases whatever supplies its value:

```rust
# extern crate guido;
# use guido::prelude::*;
# const FLAT: Shadow = Shadow::none();
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3).transition(200.0))
    .shadow(LOW.transition(200.0))

    // State changes trigger the animations declared above
    .when_hovered(|s| s.lighter(0.1).shadow(RAISED))
    .when_pressed(|s| s.darker(0.05).shadow(FLAT))
# ;
# }
```

Writing a timing on the override does not compile — it would be a declaration
that is legal and does nothing:

```rust,ignore
# // not compiled: the line is the mistake the paragraph above names.
.when_hovered(|s| s.background(HOT.transition(900.0)))   // error
```

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# const HIGH: Shadow = Shadow::simple((0.0, 6.0), 10.0, Color::rgba(0.0, 0.0, 0.0, 0.22));
fn animated_card() -> Container {
    container()
        .padding(20.0)
        .background(Color::rgb(0.15, 0.15, 0.2).transition(200.0))
        .corners(12.0)
        .border(1.0, Color::rgb(0.25, 0.25, 0.3))
        .outline(2.0, Color::TRANSPARENT.transition(150.0), 2.0)
        .shadow(RAISED.transition(250.0))

        // State layers
        .when_hovered(|s| s
            .lighter(0.05)
            .outline(2.0, Color::rgb(0.4, 0.6, 0.9), 2.0)
            .shadow(HIGH)
        )
        .when_pressed(|s| s
            .darker(0.02)
            .shadow(LOW)
        )

        .child(container().child(text("Hover me!").color(Color::WHITE)))
}
# fn main() {}
```

## API Reference

```text
/// Ease to each new value this holds: a Transition, a SpringConfig, a number
/// of milliseconds, a closure returning a Transition, or a closure of the
/// change, |from, to| -> Transition
value.transition(transition: impl IntoTransition<T, M>) -> Animated<T>;

/// A bare number is milliseconds, eased out
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition;

/// Create a spring-based transition
Transition::spring(config: SpringConfig) -> Transition
```
