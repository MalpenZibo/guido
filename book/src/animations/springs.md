# Spring Physics

Spring animations use physics simulation for natural, dynamic motion. Unlike duration-based transitions, springs can overshoot and bounce.

## Creating Spring Transitions

```rust
Transition::spring(SpringConfig::BOUNCY)
```

## Built-in Configurations

### DEFAULT

Balanced spring - responsive without excessive bounce:

```rust
SpringConfig::DEFAULT
```

### SMOOTH

Gentle spring - minimal overshoot:

```rust
SpringConfig::SMOOTH
```

### BOUNCY

Energetic spring - visible bounce:

```rust
SpringConfig::BOUNCY
```

## When to Use Springs

Springs excel at:

- **Width/height animations** - Expanding cards, accordions
- **Transform animations** - Scale, rotation, translation
- **Physical feedback** - Elements that should feel tangible

```rust
// Width expansion with spring
let expanded = create_signal(false);

container()
    .width(move || if expanded.get() { 600.0 } else { 50.0 })
    .animate_width(Transition::spring(SpringConfig::DEFAULT))
    .on_click(move || expanded.update(|e| *e = !*e))
```

## Spring vs Duration

### Use Duration For:
- Color changes
- Opacity changes
- Border changes
- Subtle state transitions

### Use Springs For:
- Size changes
- Position changes
- Scale transforms
- Anything that should feel physical

## Examples

### Bouncy Scale

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

### Smooth Expansion

```rust
let expanded = create_signal(false);

container()
    .width(move || at_least(if expanded.get() { 400.0 } else { 200.0 }))
    .animate_width(Transition::spring(SpringConfig::SMOOTH))
    .on_click(move || expanded.update(|e| *e = !*e))
```

### Animated Rotation

```rust
let rotation = create_signal(0.0f32);

container()
    .rotate(rotation)
    .animate_transform(Transition::spring(SpringConfig::DEFAULT))
    .on_click(move || rotation.update(|r| *r += 90.0))
```

## Combining Spring and Duration

Use different transition types for different properties:

```rust
container()
    // Spring for physical properties
    .animate_transform(Transition::spring(SpringConfig::BOUNCY))
    .animate_width(Transition::spring(SpringConfig::SMOOTH))

    // Duration for color properties
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
```

## Complete Example

```rust
fn spring_button() -> Container {
    let pressed = create_signal(false);

    container()
        .padding(20.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(12.0)
        .scale(move || if pressed.get() { 1.1 } else { 1.0 })

        // Spring for scale - bouncy feedback
        .animate_transform(Transition::spring(SpringConfig::BOUNCY))

        // Duration for color - smooth transition
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))

        .hover_state(|s| s.lighter(0.1))
        .on_click(move || {
            pressed.set(true);
            // In real app: trigger action and reset
        })

        .child(text("Spring!").color(Color::WHITE).font_size(18.0))
}
```

## API Reference

```rust
/// Spring configuration presets
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl SpringConfig {
    pub const DEFAULT: SpringConfig;
    pub const SMOOTH: SpringConfig;
    pub const BOUNCY: SpringConfig;
}

/// Create spring transition
Transition::spring(config: SpringConfig) -> Transition
```
