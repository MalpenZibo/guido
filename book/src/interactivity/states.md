# Hover & Pressed States

Define visual changes for different interaction states.

## Hover State

Applied when the mouse cursor is over the widget:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .hover_state(|s| s.lighter(0.1))
```

### Common Hover Patterns

**Lighten background:**
```rust
.hover_state(|s| s.lighter(0.1))
```

**Explicit color change:**
```rust
.hover_state(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))
```

**Border highlight:**
```rust
.border(1.0, Color::rgb(0.3, 0.3, 0.4))
.hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
```

**Elevation lift:**
```rust
.elevation(2.0)
.hover_state(|s| s.elevation(4.0))
```

## Pressed State

Applied when the mouse button is held down:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .pressed_state(|s| s.darker(0.1))
```

### Common Pressed Patterns

**Darken background:**
```rust
.pressed_state(|s| s.darker(0.1))
```

**Scale down (tactile feedback):**
```rust
.pressed_state(|s| s.transform(Transform::scale(0.98)))
```

**Reduce elevation (press into surface):**
```rust
.elevation(4.0)
.pressed_state(|s| s.elevation(1.0))
```

**Ripple effect:**
```rust
.pressed_state(|s| s.ripple())
```

## Combining Hover and Pressed

Most interactive elements use both states:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .hover_state(|s| s.lighter(0.1))
    .pressed_state(|s| s.ripple().darker(0.05))
```

## Combining Multiple Overrides

Each state can override multiple properties:

```rust
.hover_state(|s| s
    .lighter(0.1)
    .border(2.0, Color::rgb(0.5, 0.7, 1.0))
    .elevation(6.0)
)

.pressed_state(|s| s
    .ripple()
    .darker(0.05)
    .transform(Transform::scale(0.98))
    .elevation(2.0)
)
```

## With Animations

Add transitions for smooth state changes:

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_transform(Transition::spring(SpringConfig::SMOOTH))
    .hover_state(|s| s.lighter(0.1).border(2.0, Color::WHITE))
    .pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
```

## Button Patterns

### Simple Button

```rust
container()
    .padding(12.0)
    .background(Color::rgb(0.3, 0.5, 0.8))
    .corner_radius(6.0)
    .hover_state(|s| s.lighter(0.1))
    .pressed_state(|s| s.ripple())
    .on_click(|| println!("Clicked!"))
    .child(text("Click me").color(Color::WHITE))
```

### Outlined Button

```rust
container()
    .padding(12.0)
    .background(Color::TRANSPARENT)
    .corner_radius(6.0)
    .border(1.0, Color::rgb(0.5, 0.5, 0.6))
    .hover_state(|s| s.background(Color::rgba(1.0, 1.0, 1.0, 0.1)))
    .pressed_state(|s| s.ripple())
    .child(text("Outlined").color(Color::WHITE))
```

### Card with Lift

```rust
container()
    .padding(16.0)
    .background(Color::rgb(0.15, 0.15, 0.2))
    .corner_radius(8.0)
    .elevation(2.0)
    .animate_elevation(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.elevation(6.0).lighter(0.03))
    .pressed_state(|s| s.elevation(1.0))
    .children([...])
```

## API Reference

### StateStyle Builder

```rust
impl StateStyleBuilder {
    // Background
    pub fn background(self, color: Color) -> Self;
    pub fn lighter(self, amount: f32) -> Self;
    pub fn darker(self, amount: f32) -> Self;

    // Border
    pub fn border(self, width: f32, color: Color) -> Self;
    pub fn border_width(self, width: f32) -> Self;
    pub fn border_color(self, color: Color) -> Self;

    // Other
    pub fn corner_radius(self, radius: f32) -> Self;
    pub fn transform(self, transform: Transform) -> Self;
    pub fn elevation(self, level: f32) -> Self;

    // Ripple
    pub fn ripple(self) -> Self;
    pub fn ripple_with_color(self, color: Color) -> Self;
}
```
