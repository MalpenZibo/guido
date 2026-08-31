# Transitions

A transition eases a property to each new value over a fixed time, with an
easing curve. It is declared on the value itself, with `.transition(..)`.

## Declaring one

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = Color::WHITE;
# let timing = TimingFunction::EaseOut;
# let duration_ms = 200.0;
value.transition(Transition::new(duration_ms, timing))
# ;
# }
```

- `duration_ms` - Animation duration in milliseconds
- `timing` - Easing curve for the animation

A bare number is the short form: `.transition(200.0)` is 200ms eased out.

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

### Border Animation

A border is declared as a pair and each half carries its own timing, so the
width can spring while the colour eases:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .border(
        1.0.transition(150.0),
        Color::rgb(0.3, 0.3, 0.4).transition(150.0),
    )
    .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
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
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3).transition(200.0))
    .elevation(2.0.transition(200.0))

    // State changes trigger the animations declared above
    .when_hovered(|s| s.lighter(0.1).elevation(4.0))
    .when_pressed(|s| s.darker(0.05).elevation(1.0))
# ;
# }
```

Writing a timing on the override does not compile — it would be a declaration
that is legal and does nothing:

```rust,ignore
.when_hovered(|s| s.background(HOT.transition(900.0)))   // error
```

## Complete Example

```rust,ignore
fn animated_card() -> Container {
    container()
        .padding(20.0)
        .background(Color::rgb(0.15, 0.15, 0.2).transition(200.0))
        .corners(12.0)
        .border(
            1.0.transition(150.0),
            Color::rgb(0.25, 0.25, 0.3).transition(150.0),
        )
        .elevation(4.0.transition(250.0))

        // State layers
        .when_hovered(|s| s
            .lighter(0.05)
            .border(2.0, Color::rgb(0.4, 0.6, 0.9))
            .elevation(8.0)
        )
        .when_pressed(|s| s
            .darker(0.02)
            .elevation(2.0)
        )

        .child(container().child(text("Hover me!").color(Color::WHITE)))
}
```

## API Reference

```text
/// Ease to each new value this holds
value.transition(transition: impl Into<TransitionConfig>) -> Animated<T>;

/// A bare number is milliseconds, eased out
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition;

/// Create a spring-based transition
Transition::spring(config: SpringConfig) -> Transition
```
