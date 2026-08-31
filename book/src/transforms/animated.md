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

```rust,ignore
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
                .layout(Flex::column().main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
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
                .layout(Flex::column().main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
                .child(container().child(text("Scale").font_size(12.0).color(Color::WHITE))),
        ])
}
```

## API Reference

```rust,ignore
// The three components each take a value that may carry its own motion.
impl Container {
    pub fn translate<M>(self, t: impl IntoAnimated<Translate, M>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoAnimated<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoAnimated<Scale, M>) -> Self;
}

// Duration-based
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition

// Spring-based
Transition::spring(config: SpringConfig) -> Transition
```
