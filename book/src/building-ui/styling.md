# Styling Overview

This page provides a complete reference for all styling options available in Guido.

## Backgrounds

### Solid Color

```rust
container().background(Color::rgb(0.2, 0.2, 0.3))
```

### Gradients

```rust
// Horizontal (left to right)
container().gradient_horizontal(Color::RED, Color::BLUE)

// Vertical (top to bottom)
container().gradient_vertical(Color::RED, Color::BLUE)

// Diagonal
container().gradient_diagonal(Color::RED, Color::BLUE)
```

## Corners

### Basic Radius

```rust
container().corner_radius(8.0)  // 8px radius on all corners
```

### Corner Curvature

Control corner shape using CSS K-values:

```rust
container().corner_radius(12.0).squircle()  // iOS-style (K=2)
container().corner_radius(12.0)              // Circular (K=1, default)
container().corner_radius(12.0).bevel()      // Diagonal (K=0)
container().corner_radius(12.0).scoop()      // Concave (K=-1)
container().corner_radius(12.0).corner_curvature(1.5)  // Custom
```

## Borders

```rust
container()
    .border(2.0, Color::WHITE)  // Width and color

// Or separately
container()
    .border_width(2.0)
    .border_color(Color::WHITE)
```

## Shadows (Elevation)

```rust
container().elevation(2.0)   // Subtle
container().elevation(8.0)   // Medium
container().elevation(16.0)  // Strong
```

## Padding

```rust
container().padding(16.0)              // All sides
container().padding_horizontal(20.0)   // Left and right
container().padding_vertical(10.0)     // Top and bottom
```

## Sizing

### Fixed Size

```rust
container()
    .width(100.0)
    .height(50.0)
```

### Constraints

```rust
container()
    .min_width(50.0)
    .max_width(200.0)
    .min_height(30.0)
    .max_height(100.0)
```

### At Least

```rust
container().width(at_least(100.0))  // At least 100px
container().height(at_least(50.0))  // At least 50px
```

## Complete Example

```rust
fn styled_card(title: &str, content: &str) -> Container {
    container()
        // Size and padding
        .width(300.0)
        .padding(20.0)

        // Background and corners
        .background(Color::rgb(0.15, 0.15, 0.2))
        .corner_radius(12.0)
        .squircle()

        // Border
        .border(1.0, Color::rgb(0.25, 0.25, 0.3))

        // Shadow
        .elevation(4.0)

        // Layout
        .layout(Flex::column().spacing(12.0))

        // State layers
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .hover_state(|s| s.lighter(0.05).elevation(6.0))

        // Children
        .children([
            text(title)
                .font_size(18.0)
                .bold()
                .color(Color::WHITE),
            text(content)
                .font_size(14.0)
                .color(Color::rgb(0.7, 0.7, 0.75)),
        ])
}
```
