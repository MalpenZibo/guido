# Animations

Guido supports smooth animations for property changes using both duration-based transitions and spring physics.

![Animation Example](../images/animation_example.png)

## Overview

Animations in Guido work by:
1. Declaring which properties can animate
2. Specifying a transition type (duration or spring)
3. Letting the framework interpolate between values

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.15))
```

When the hover state changes, the background animates smoothly over 200ms.

## In This Section

- [Transitions](transitions.md) - Duration-based animations
- [Timing Functions](timing.md) - Easing curves for natural motion
- [Spring Physics](springs.md) - Physics-based animations
- [Animatable Properties](properties.md) - What can be animated

## Two Types of Animation

### Duration-Based

Fixed duration with easing curve:

```rust
Transition::new(200.0, TimingFunction::EaseOut)
```

Good for:
- UI state changes (hover, pressed)
- Color transitions
- Border changes

### Spring-Based

Physics simulation for natural motion:

```rust
Transition::spring(SpringConfig::BOUNCY)
```

Good for:
- Size changes
- Position changes
- Transform animations
- Any motion that should feel physical

## Quick Reference

```rust
// Duration-based
.animate_background(Transition::new(200.0, TimingFunction::EaseOut))
.animate_border_width(Transition::new(150.0, TimingFunction::EaseInOut))

// Spring-based
.animate_transform(Transition::spring(SpringConfig::BOUNCY))
.animate_width(Transition::spring(SpringConfig::SMOOTH))
```
