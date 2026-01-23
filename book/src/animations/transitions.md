# Transitions

Duration-based transitions animate properties over a fixed time with an easing curve.

## Creating Transitions

```rust
Transition::new(duration_ms, timing_function)
```

- `duration_ms` - Animation duration in milliseconds
- `timing_function` - Easing curve for the animation

## Examples

### Background Animation

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.15))
```

### Border Animation

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
```

### Transform Animation

```rust
container()
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .pressed_state(|s| s.transform(Transform::scale(0.98)))
```

## Duration Guidelines

| Duration | Use Case |
|----------|----------|
| 100-150ms | Quick feedback (button press) |
| 150-200ms | State changes (hover) |
| 200-300ms | Content changes (expand/collapse) |
| 300-500ms | Major transitions (page changes) |

## Combining with State Layers

Transitions work seamlessly with state layers:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .elevation(2.0)

    // Animate multiple properties
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_elevation(Transition::new(200.0, TimingFunction::EaseOut))

    // State changes trigger animations
    .hover_state(|s| s.lighter(0.1).elevation(4.0))
    .pressed_state(|s| s.darker(0.05).elevation(1.0))
```

## Complete Example

```rust
fn animated_card() -> Container {
    container()
        .padding(20.0)
        .background(Color::rgb(0.15, 0.15, 0.2))
        .corner_radius(12.0)
        .border(1.0, Color::rgb(0.25, 0.25, 0.3))
        .elevation(4.0)

        // Animations
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_elevation(Transition::new(250.0, TimingFunction::EaseOut))

        // State layers
        .hover_state(|s| s
            .lighter(0.05)
            .border(2.0, Color::rgb(0.4, 0.6, 0.9))
            .elevation(8.0)
        )
        .pressed_state(|s| s
            .darker(0.02)
            .elevation(2.0)
        )

        .child(text("Hover me!").color(Color::WHITE))
}
```

## API Reference

```rust
/// Create a duration-based transition
Transition::new(duration_ms: f32, timing: TimingFunction) -> Transition

/// Create a spring-based transition
Transition::spring(config: SpringConfig) -> Transition
```
