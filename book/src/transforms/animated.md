# Animated Transforms

Animate transform changes with smooth transitions.

## Enabling Animation

```rust
container()
    .rotate(rotation_signal)
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
```

When `rotation_signal` changes, the transform animates smoothly.

## Duration-Based Animation

Standard easing curve transitions:

```rust
// Smooth ease-out rotation
container()
    .rotate(rotation)
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .on_click(move || rotation.update(|r| *r += 45.0))
```

## Spring-Based Animation

Physics simulation for bouncy, natural motion:

```rust
container()
    .scale(scale_signal)
    .animate_transform(Transition::spring(SpringConfig::BOUNCY))
```

Spring presets:
- `SpringConfig::DEFAULT` - Balanced
- `SpringConfig::SMOOTH` - Gentle, minimal overshoot
- `SpringConfig::BOUNCY` - Energetic with visible bounce

## Examples

### Click to Rotate

```rust
let rotation = create_signal(0.0f32);

container()
    .width(80.0)
    .height(80.0)
    .background(Color::rgb(0.3, 0.6, 0.8))
    .corner_radius(8.0)
    .rotate(rotation)
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.1))
    .pressed_state(|s| s.ripple())
    .on_click(move || rotation.update(|r| *r += 45.0))
```

### Bouncy Scale Toggle

```rust
let scale_factor = create_signal(1.0f32);
let is_scaled = create_signal(false);

container()
    .scale(scale_factor)
    .animate_transform(Transition::spring(SpringConfig::BOUNCY))
    .on_click(move || {
        is_scaled.update(|s| *s = !*s);
        let target = if is_scaled.get() { 1.3 } else { 1.0 };
        scale_factor.set(target);
    })
```

### Scale on Press (State Layer)

```rust
container()
    .animate_transform(Transition::spring(SpringConfig::SMOOTH))
    .pressed_state(|s| s.transform(Transform::scale(0.98)))
```

### Smooth Translation

```rust
let offset_x = create_signal(0.0f32);

container()
    .translate(offset_x, 0.0)
    .animate_transform(Transition::new(400.0, TimingFunction::EaseInOut))
    .on_scroll(move |_, dy, _| {
        offset_x.update(|x| *x += dy * 10.0);
    })
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

```rust
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
                .corner_radius(8.0)
                .rotate(rotation)
                .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
                .hover_state(|s| s.lighter(0.1))
                .pressed_state(|s| s.ripple())
                .on_click(move || rotation.update(|r| *r += 45.0))
                .layout(Flex::column().main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
                .child(text("Rotate").color(Color::WHITE).font_size(12.0)),

            // Spring-based scale
            container()
                .width(80.0)
                .height(80.0)
                .background(Color::rgb(0.3, 0.8, 0.4))
                .corner_radius(8.0)
                .scale(scale)
                .animate_transform(Transition::spring(SpringConfig::BOUNCY))
                .hover_state(|s| s.lighter(0.1))
                .pressed_state(|s| s.ripple())
                .on_click(move || {
                    is_scaled.update(|s| *s = !*s);
                    scale.set(if is_scaled.get() { 1.3 } else { 1.0 });
                })
                .layout(Flex::column().main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
                .child(text("Scale").color(Color::WHITE).font_size(12.0)),
        ])
}
```

## API Reference

```rust
impl Container {
    pub fn animate_transform(self, transition: Transition) -> Self;
}

// Duration-based
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition

// Spring-based
Transition::spring(config: SpringConfig) -> Transition
```
