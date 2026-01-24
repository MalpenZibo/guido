# Borders & Corners

Guido renders crisp, anti-aliased borders using SDF (Signed Distance Field) techniques.

![Showcase](../images/showcase.png)

## Basic Border

```rust
container()
    .border(2.0, Color::WHITE)  // 2px white border
```

### Separate Width and Color

```rust
container()
    .border_width(2.0)
    .border_color(Color::rgb(0.5, 0.5, 0.6))
```

## Corner Radius

### Uniform Radius

```rust
container().corner_radius(8.0)  // 8px radius on all corners
```

## Corner Curvature (Superellipse)

Control the shape of corners using CSS K-values. This determines how the corner curves from the edge to the arc.

### Squircle (K=2)

iOS-style smooth corners. The curve starts further from the corner for a smoother transition.

```rust
container()
    .corner_radius(12.0)
    .squircle()
```

### Circle (K=1)

Standard circular corners. This is the default.

```rust
container()
    .corner_radius(12.0)  // Default is circular
```

### Bevel (K=0)

Diagonal cut corners. Creates a chamfered look.

```rust
container()
    .corner_radius(12.0)
    .bevel()
```

### Scoop (K=-1)

Concave/inward corners. Creates a scooped appearance.

```rust
container()
    .corner_radius(12.0)
    .scoop()
```

### Custom Curvature

For values between the presets:

```rust
container()
    .corner_radius(12.0)
    .corner_curvature(1.5)  // Between circle and squircle
```

## Curvature Reference

| Style | K Value | Description |
|-------|---------|-------------|
| Squircle | 2.0 | Smooth, iOS-style |
| Circle | 1.0 | Standard rounded (default) |
| Bevel | 0.0 | Diagonal/chamfered |
| Scoop | -1.0 | Concave inward |

## Animated Borders

Borders can animate on state changes:

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
    .pressed_state(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))
```

## Borders with Different Curvatures

Borders respect corner curvature:

```rust
container()
    .border(2.0, Color::rgb(0.5, 0.3, 0.7))
    .corner_radius(12.0)
    .squircle()  // Border follows squircle shape
```

## Borders with Gradients

Borders work with gradient backgrounds:

```rust
container()
    .gradient_horizontal(Color::rgb(0.3, 0.1, 0.4), Color::rgb(0.1, 0.3, 0.5))
    .corner_radius(8.0)
    .border(2.0, Color::rgba(1.0, 1.0, 1.0, 0.3))  // Semi-transparent white
```

## Complete Example

```rust
fn card_with_border() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.12, 0.12, 0.16))
        .corner_radius(12.0)
        .squircle()
        .border(1.0, Color::rgb(0.2, 0.2, 0.25))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
        .hover_state(|s| s
            .border(2.0, Color::rgb(0.4, 0.6, 0.9))
            .lighter(0.03)
        )
        .child(text("Hover to see border change").color(Color::WHITE))
}
```
