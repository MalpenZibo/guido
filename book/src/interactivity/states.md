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

## Text Colour

A state layer can change the colour of the text below the container, not just
the box around it:

```rust
container()
    .text_color(theme.text_weak)
    .hover_state(|s| s.text_color(theme.text))
    .child(text("Label"))
```

This works the same way the ordinary declaration does — the container publishes
its text colour to descendants, and here what it publishes folds in the hover.
A text that declares its own colour nearer down is unaffected, and a container
with no state layer mentioning text creates nothing extra.

The base it returns to when the state ends is the colour that would have been
inherited anyway, so a container can override on hover without restating what
its ancestors already said:

```rust
container()
    .text_color(theme.text_weak)              // set once, further up
    .child(
        container()
            .hover_state(|s| s.text_color(theme.text))
            .child(text("Label")),            // weak, then strong, then weak
    )
```

It can be animated like any other container property:

```rust
container()
    .text_color(theme.text_weak)
    .hover_state(|s| s.text_color(theme.text))
    .animate_text_color(Transition::new(200.0, TimingFunction::EaseOut))
    .child(text("Label"))
```

A transition declared on two levels — an animated colour whose own base comes
from an ancestor that is itself animating — currently retargets every frame,
which gives a damped chase rather than a transition with its own curve. It
converges either way; CSS instead starts the inner one once, towards the
outer's final value.

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
    .child(container().text_color(Color::WHITE).child(text("Click me")))
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
    .child(container().text_color(Color::WHITE).child(text("Outlined")))
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
