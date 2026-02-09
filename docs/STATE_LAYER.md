# State Layer API

The state layer system provides declarative style overrides based on widget interaction state (hover, pressed, focused). This enables rich visual feedback without manual signal management.

## Overview

State layers allow containers to define how they should look when:
- **Hovered**: Mouse cursor is over the widget
- **Pressed**: Mouse button is held down on the widget
- **Focused**: Any child widget has keyboard focus (e.g., text input)

Style changes are defined declaratively using builder methods, and the framework handles all state transitions, animations, and rendering automatically.

## Basic Usage

```rust
use guido::prelude::*;

container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(8.0)
    .hover_state(|s| s.lighter(0.1))      // Lighten on hover
    .pressed_state(|s| s.ripple())         // Ripple on press
    .child(text("Click me"))
```

### Focused State for Input Containers

The `focused_state` is applied when any child widget has keyboard focus. This is particularly useful for styling input containers:

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corner_radius(6.0)
    .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))  // Highlight when focused
    .child(text_input(value))
```

## State Style Methods

### Background Color

```rust
// Explicit color
.hover_state(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))

// Relative to base color
.hover_state(|s| s.lighter(0.1))   // 10% lighter
.pressed_state(|s| s.darker(0.1))  // 10% darker
```

### Border

```rust
// Change border on hover
.border(1.0, Color::rgb(0.3, 0.3, 0.4))
.hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
.pressed_state(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))

// Or just width or color
.hover_state(|s| s.border_width(2.0))
.hover_state(|s| s.border_color(Color::WHITE))
```

### Transform

```rust
// Scale down on press for tactile feedback
.pressed_state(|s| s.transform(Transform::scale(0.98)))

// Combine with other effects
.pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
```

### Corner Radius

```rust
.corner_radius(8.0)
.hover_state(|s| s.corner_radius(12.0))
```

### Elevation (Shadow)

```rust
.elevation(2.0)
.hover_state(|s| s.elevation(4.0))
.pressed_state(|s| s.elevation(1.0))
```

## Ripple Effects

Ripple effects provide Material Design-style touch feedback. The ripple expands from the click point and contracts toward the release point.

### Default Ripple

```rust
.pressed_state(|s| s.ripple())
```

### Colored Ripple

```rust
.pressed_state(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
```

### Ripple with Other Effects

```rust
.pressed_state(|s| s.ripple().transform(Transform::scale(0.98)))
```

## Animations

State transitions can be animated using the `animate_*` methods:

```rust
container()
    .background(Color::rgb(0.3, 0.6, 0.4))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.15))
    .pressed_state(|s| s.darker(0.1))
```

Available animation methods:
- `animate_background(Transition)` - Animate background color changes
- `animate_border_width(Transition)` - Animate border width changes
- `animate_border_color(Transition)` - Animate border color changes
- `animate_transform(Transition)` - Animate transform changes

### Transition Types

```rust
// Duration-based with timing function
Transition::new(200.0, TimingFunction::EaseOut)
Transition::new(150.0, TimingFunction::EaseInOut)

// Spring-based for physics-driven animation
Transition::spring(SpringConfig::BOUNCY)
Transition::spring(SpringConfig::SMOOTH)
```

## Complete Example

```rust
fn create_button(label: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .border(1.0, Color::rgb(0.4, 0.6, 0.9))
        // Animations
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        // State overrides
        .hover_state(|s| s.lighter(0.1).border(2.0, Color::rgb(0.5, 0.7, 1.0)))
        .pressed_state(|s| s.ripple().darker(0.05).transform(Transform::scale(0.98)))
        .child(text(label).color(Color::WHITE))
}
```

## Implementation Notes

### StateStyle Struct

The `StateStyle` struct holds all possible overrides:

```rust
pub struct StateStyle {
    pub background: Option<BackgroundOverride>,
    pub border_width: Option<f32>,
    pub border_color: Option<Color>,
    pub corner_radius: Option<f32>,
    pub transform: Option<Transform>,
    pub elevation: Option<f32>,
    pub ripple: Option<RippleConfig>,
}
```

### BackgroundOverride Enum

Background can be set absolutely or relatively:

```rust
pub enum BackgroundOverride {
    Exact(Color),      // Use this exact color
    Lighter(f32),      // Blend toward white by amount (0.0-1.0)
    Darker(f32),       // Blend toward black by amount (0.0-1.0)
}
```

### Ripple Rendering

Ripples are rendered in the overlay layer (on top of text and other content). They:
- Expand from the click point to fill the container bounds
- Respect corner radius and container clipping
- Contract toward the release point when the mouse is released
- Work correctly with transformed containers (rotated, scaled, translated)


Benefits of the new API:
- Less boilerplate code
- No manual signal management
- Built-in animation support
- Ripple effects included
- Better separation of concerns
