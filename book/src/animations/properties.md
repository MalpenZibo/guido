# Animatable Properties

This page lists all container properties that can be animated.

## Background

Animate background color changes:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.1))
```

Works with:
- Solid colors
- State layer overrides (lighter, darker, explicit)

## Border Width

Animate border thickness:

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .hover_state(|s| s.border_width(2.0))
```

## Border Color

Animate border color:

```rust
container()
    .border(2.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .hover_state(|s| s.border_color(Color::rgb(0.5, 0.7, 1.0)))
```

## Transform

Animate translation, rotation, and scale:

```rust
container()
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .pressed_state(|s| s.transform(Transform::scale(0.98)))
```

Works with:
- Rotate
- Scale
- Translate
- Combined transforms

Spring animations are especially good for transforms:

```rust
.animate_transform(Transition::spring(SpringConfig::BOUNCY))
```

## Width

Animate width changes:

```rust
let expanded = create_signal(false);

container()
    .width(move || if expanded.get() { 400.0 } else { 200.0 })
    .animate_width(Transition::spring(SpringConfig::DEFAULT))
```

## Elevation

Animate shadow depth:

```rust
container()
    .elevation(2.0)
    .animate_elevation(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.elevation(6.0))
```

## Multiple Animations

Combine animations on a single container:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .elevation(2.0)

    // Animate all
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_elevation(Transition::new(250.0, TimingFunction::EaseOut))
    .animate_transform(Transition::spring(SpringConfig::SMOOTH))

    .hover_state(|s| s
        .lighter(0.1)
        .border(2.0, Color::WHITE)
        .elevation(6.0)
    )
    .pressed_state(|s| s
        .transform(Transform::scale(0.98))
        .elevation(1.0)
    )
```

## Complete Reference

| Property | Method | Recommended Transition |
|----------|--------|----------------------|
| Background | `animate_background()` | Duration, EaseOut |
| Border Width | `animate_border_width()` | Duration, EaseOut |
| Border Color | `animate_border_color()` | Duration, EaseOut |
| Transform | `animate_transform()` | Spring or Duration |
| Width | `animate_width()` | Spring |
| Elevation | `animate_elevation()` | Duration, EaseOut |

## Best Practices

### Match Durations for Related Properties

```rust
// Same duration for border width and color
.animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
.animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
```

### Use Springs for Physical Motion

```rust
// Spring for size/position changes
.animate_width(Transition::spring(SpringConfig::DEFAULT))
.animate_transform(Transition::spring(SpringConfig::BOUNCY))

// Duration for visual changes
.animate_background(Transition::new(200.0, TimingFunction::EaseOut))
```

### Keep Animations Subtle

- 150-300ms for most UI animations
- Avoid overly bouncy springs in professional UIs
- Let animations enhance, not distract

## API Reference

```rust
impl Container {
    pub fn animate_background(self, transition: Transition) -> Self;
    pub fn animate_border_width(self, transition: Transition) -> Self;
    pub fn animate_border_color(self, transition: Transition) -> Self;
    pub fn animate_transform(self, transition: Transition) -> Self;
    pub fn animate_width(self, transition: Transition) -> Self;
    pub fn animate_elevation(self, transition: Transition) -> Self;
}
```
