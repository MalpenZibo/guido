# State Layer API

The state layer system provides declarative style overrides based on widget interaction state.

## Overview

State layers let containers define how they should look when:
- **Hovered** - Mouse cursor is over the widget
- **Pressed** - Mouse button is held down on the widget

Changes are defined declaratively, and the framework handles state transitions, animations, and rendering.

## Basic Usage

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corners(8.0)
    .when_hovered(|s| s.lighter(0.1))      // Style when hovered
    .when_pressed(|s| s.ripple())         // Style when pressed
    .child(text("Click me"))
```

## Available Overrides

State layers can override these properties:

### Background

```rust
// Explicit color
.when_hovered(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))

// Relative to base
.when_hovered(|s| s.lighter(0.1))   // 10% lighter
.when_pressed(|s| s.darker(0.1))  // 10% darker
```

### Border

```rust
.when_hovered(|s| s.border(2.0, Color::WHITE))   // both halves, always
```

### Transform

```rust
.when_pressed(|s| s.scale(0.98))
```

### Corner Radius

```rust
.when_hovered(|s| s.corners(12.0))
```

### Elevation

```rust
.when_hovered(|s| s.elevation(8.0))
.when_pressed(|s| s.elevation(2.0))
```

### Ripple

```rust
.when_pressed(|s| s.ripple())
.when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
```

## Combining Overrides

Chain multiple overrides in a single state:

```rust
.when_hovered(|s| s
    .lighter(0.1)
    .border(2.0, Color::WHITE)
    .elevation(6.0)
)

.when_pressed(|s| s
    .ripple()
    .darker(0.05)
    .scale(0.98)
)
```

## With Animations

Add transitions for smooth state changes:

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.lighter(0.15))
    .when_pressed(|s| s.darker(0.1))
```

## Complete Example

```rust
fn interactive_button(label: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corners(8.0)
        .border(1.0, Color::rgb(0.4, 0.6, 0.9))

        // Animations
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_transform(Transition::spring(SpringConfig::SMOOTH))

        // State layers
        .when_hovered(|s| s
            .lighter(0.1)
            .border(2.0, Color::rgb(0.5, 0.7, 1.0))
        )
        .when_pressed(|s| s
            .ripple()
            .darker(0.05)
            .scale(0.98)
        )

        .child(container().child(text(label).color(Color::WHITE)))
}
```

## How It Works

Internally, `StateStyle` holds all possible overrides:

```rust
pub struct StateStyle {
    pub background: Option<BackgroundOverride>,
    // Both halves or neither: half a border is no border.
    pub border: Option<BorderOverride>,
    pub corner_radius: Option<Signal<f32>>,
    pub transform: Option<Signal<Transform>>,
    pub elevation: Option<Signal<f32>>,
    pub text_color: Option<Signal<Color>>,
    pub alpha: Option<Signal<f32>>,
    pub ripple: Option<RippleConfig>,
}
```

When the container paints, it checks the current state and applies overrides accordingly, blending with animations when configured.
