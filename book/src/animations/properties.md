# Animatable Properties

This page lists all container properties that can be animated.

## Background

Animate background color changes:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.lighter(0.1))
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
    // A border is declared as a pair, so the layer restates the colour it is
    // not changing — otherwise the width eases while the colour jumps.
    .when_hovered(|s| s.border(2.0, Color::rgb(0.3, 0.3, 0.4)))
```

## Border Color

Animate border color:

```rust
container()
    .border(2.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    // The mirror image: the width is restated so only the colour moves.
    .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.7, 1.0)))
```

## Transform

Animate translation, rotation, and scale:

```rust
container()
    .animate_scale(Transition::new(300.0, TimingFunction::EaseOut))
    .when_pressed(|s| s.scale(0.98))
```

Works with:
- Rotate
- Scale
- Translate

Each on its own curve — there is no call that animates the three together, and
declaring one says nothing about the other two.

Spring animations are especially good for transforms:

```rust
.animate_scale(Transition::spring(SpringConfig::BOUNCY))
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
    .when_hovered(|s| s.elevation(6.0))
```

A shadow falls outside the box that casts it, so the container has to tell the
layout how far its painting reaches — and the reach has to cover the deepest
shadow it can ever cast, since a hover never re-runs layout.

That makes the two ways of lifting a card cost different amounts:

```rust
// Constants: the deepest is 6 whatever happens, so hovering moves only the
// paint and nothing is re-laid-out.
.elevation(2.0).when_hovered(|s| s.elevation(6.0))

// A signal: the deepest genuinely changes when it is written, so every write
// re-runs the layout of this subtree.
.elevation(move || if lifted.get() { 6.0 } else { 2.0 })
```

Both are correct, and the second is the one to reach for when the depth is
driven by something other than pointer state. Prefer the state layer when it can
say the same thing.

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
    .animate_scale(Transition::spring(SpringConfig::GENTLE))

    .when_hovered(|s| s
        .lighter(0.1)
        .border(2.0, Color::WHITE)
        .elevation(6.0)
    )
    .when_pressed(|s| s
        .scale(0.98)
        .elevation(1.0)
    )
```

## Complete Reference

| Property | Method | Recommended Transition |
|----------|--------|----------------------|
| Background | `animate_background()` | Duration, EaseOut |
| Border Width | `animate_border_width()` | Duration, EaseOut |
| Border Color | `animate_border_color()` | Duration, EaseOut |
| Translate | `animate_translate()` | Spring or Duration |
| Rotate | `animate_rotate()` | Spring or Duration |
| Scale | `animate_scale()` | Spring or Duration |
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
.animate_scale(Transition::spring(SpringConfig::BOUNCY))

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
    pub fn animate_translate(self, transition: Transition) -> Self;
    pub fn animate_rotate(self, transition: Transition) -> Self;
    pub fn animate_scale(self, transition: Transition) -> Self;
    pub fn animate_width(self, transition: Transition) -> Self;
    pub fn animate_elevation(self, transition: Transition) -> Self;
}
```
