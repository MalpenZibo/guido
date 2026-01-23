# Ripple Effects

Ripples provide Material Design-style touch feedback. They expand from the click point and create a visual acknowledgment of user interaction.

## Basic Ripple

Add a default ripple to the pressed state:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(8.0)
    .pressed_state(|s| s.ripple())
```

The default ripple uses a semi-transparent white overlay.

## Colored Ripple

Customize the ripple color:

```rust
.pressed_state(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
```

Good ripple colors have transparency (alpha 0.2-0.5):

```rust
// Subtle white
Color::rgba(1.0, 1.0, 1.0, 0.2)

// Yellow accent
Color::rgba(1.0, 0.8, 0.0, 0.4)

// Blue accent
Color::rgba(0.3, 0.5, 1.0, 0.3)
```

## Ripple with Other Effects

Combine ripples with other pressed state changes:

```rust
.pressed_state(|s| s
    .ripple()
    .darker(0.05)
    .transform(Transform::scale(0.98))
)
```

## How Ripples Work

1. **Click** - Ripple starts at the click point
2. **Expand** - Ripple grows to fill the container bounds
3. **Release** - Ripple contracts toward the release point
4. **Fade** - Ripple fades out

The ripple:
- Respects corner radius and container shape
- Works correctly with transformed containers (rotated, scaled)
- Renders in the overlay layer (on top of content)

## Ripples on Transformed Containers

Ripples work correctly even with transforms:

```rust
container()
    .padding(16.0)
    .background(Color::rgb(0.4, 0.6, 0.4))
    .corner_radius(8.0)
    .transform(Transform::rotate_degrees(5.0).then(&Transform::translate(10.0, 15.0)))
    .hover_state(|s| s.lighter(0.1))
    .pressed_state(|s| s.ripple())
```

Click coordinates are properly transformed to local container space.

## Ripples with Corner Curvature

Ripples respect different corner styles:

```rust
// Squircle ripple
container()
    .corner_radius(12.0)
    .squircle()
    .pressed_state(|s| s.ripple())

// Beveled ripple
container()
    .corner_radius(12.0)
    .bevel()
    .pressed_state(|s| s.ripple())
```

## Complete Example

```rust
fn ripple_button(label: &str, color: Color) -> Container {
    container()
        .padding(16.0)
        .background(color)
        .corner_radius(8.0)

        // Subtle hover, ripple on press
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple().transform(Transform::scale(0.98)))

        .on_click(|| println!("Clicked!"))
        .child(text(label).color(Color::WHITE))
}

// Usage
ripple_button("Default Ripple", Color::rgb(0.3, 0.5, 0.8))
ripple_button("Action Button", Color::rgb(0.8, 0.3, 0.3))
```

## Ripple Color Guidelines

| Background | Ripple Color |
|------------|--------------|
| Dark | `Color::rgba(1.0, 1.0, 1.0, 0.2-0.3)` |
| Light | `Color::rgba(0.0, 0.0, 0.0, 0.1-0.2)` |
| Colored | Lighter tint with 0.3-0.4 alpha |

## API Reference

```rust
// Default semi-transparent white ripple
.pressed_state(|s| s.ripple())

// Custom colored ripple
.pressed_state(|s| s.ripple_with_color(Color::rgba(r, g, b, a)))
```
