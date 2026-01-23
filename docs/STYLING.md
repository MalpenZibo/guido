# Styling Guide

This document covers the visual styling options available in Guido.

## Colors

### Creating Colors

```rust
// RGB (values 0.0-1.0)
Color::rgb(0.2, 0.4, 0.8)

// RGBA with alpha
Color::rgba(0.2, 0.4, 0.8, 0.5)

// Predefined colors
Color::WHITE
Color::BLACK
Color::RED
Color::GREEN
Color::BLUE
Color::TRANSPARENT
```

### Color Operations

```rust
// Blend toward white (lighter)
let lighter = color.lighter(0.1);  // 10% lighter

// Blend toward black (darker)
let darker = color.darker(0.1);   // 10% darker
```

## Backgrounds

### Solid Color

```rust
container().background(Color::rgb(0.2, 0.2, 0.3))
```

### Gradients

```rust
// Horizontal gradient (left to right)
container().gradient_horizontal(Color::RED, Color::BLUE)

// Vertical gradient (top to bottom)
container().gradient_vertical(Color::RED, Color::BLUE)

// Diagonal gradient
container().gradient_diagonal(Color::RED, Color::BLUE)
```

## Borders

### Basic Border

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

### Animated Borders

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
```

## Corner Radius

### Uniform Radius

```rust
container().corner_radius(8.0)  // 8px radius on all corners
```

### Corner Curvature (Superellipse)

Control the shape of corners using CSS K-values:

```rust
// Squircle - iOS-style smooth corners (K=2)
container()
    .corner_radius(12.0)
    .squircle()

// Circle - standard circular corners (K=1, default)
container()
    .corner_radius(12.0)  // Default is circular

// Bevel - diagonal cut corners (K=0)
container()
    .corner_radius(12.0)
    .bevel()

// Scoop - concave/inward corners (K=-1)
container()
    .corner_radius(12.0)
    .scoop()

// Custom curvature value
container()
    .corner_radius(12.0)
    .corner_curvature(1.5)  // Between circle and squircle
```

**Curvature reference:**
| Style | K value | Description |
|-------|---------|-------------|
| Squircle | 2.0 | Smooth, iOS-style |
| Circle | 1.0 | Standard rounded |
| Bevel | 0.0 | Diagonal/chamfered |
| Scoop | -1.0 | Concave inward |

## Shadows and Elevation

Material Design-style elevation shadows:

```rust
container().elevation(2.0)   // Subtle shadow
container().elevation(8.0)   // More pronounced shadow
container().elevation(16.0)  // Strong shadow
```

### Elevation in State Layers

```rust
container()
    .elevation(2.0)
    .hover_state(|s| s.elevation(4.0))
    .pressed_state(|s| s.elevation(1.0))
```

## Padding

### Uniform Padding

```rust
container().padding(16.0)  // 16px on all sides
```

### Directional Padding

```rust
container()
    .padding_horizontal(20.0)  // Left and right
    .padding_vertical(10.0)    // Top and bottom
```

## Sizing

### Fixed Size

```rust
container()
    .width(100.0)
    .height(50.0)
```

### Minimum/Maximum Size

```rust
container()
    .min_width(50.0)
    .max_width(200.0)
    .min_height(30.0)
    .max_height(100.0)
```

## Text Styling

```rust
text("Hello")
    .font_size(16.0)
    .color(Color::WHITE)
    .bold()              // Font weight
    .italic()            // Font style
    .nowrap()            // Prevent wrapping
```

## Layout Styling

### Flex Layout

```rust
container()
    .layout(
        Flex::row()
            .spacing(8.0)
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
    )
```

### Alignment Options

**Main Axis (direction of flow):**
- `MainAxisAlignment::Start`
- `MainAxisAlignment::End`
- `MainAxisAlignment::Center`
- `MainAxisAlignment::SpaceBetween`
- `MainAxisAlignment::SpaceAround`
- `MainAxisAlignment::SpaceEvenly`

**Cross Axis (perpendicular to flow):**
- `CrossAxisAlignment::Start`
- `CrossAxisAlignment::End`
- `CrossAxisAlignment::Center`
- `CrossAxisAlignment::Stretch`

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
