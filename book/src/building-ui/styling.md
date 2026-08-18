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
container().padding(16)                // Integers work too
container().padding([8.0, 16.0])       // [vertical, horizontal]
container().padding([8, 16])           // Integer arrays too
container().padding([1.0, 2.0, 3.0, 4.0])  // [top, right, bottom, left]
container().padding(Padding::all(8.0).with_top(20.0))  // Builder for one edge
```

## Sizing

### Fixed Size

```rust
container()
    .width(100.0)
    .height(50.0)
```

Integers work too:

```rust
container()
    .width(100)
    .height(50)
```

### Constraints

```rust
container()
    .width(at_least(50.0).at_most(200.0))
    .height(at_least(30.0).at_most(100.0))
```

### At Least / At Most

```rust
container().width(at_least(100.0))           // At least 100px
container().width(at_least(100))             // Integers work too
container().width(at_most(400))              // At most 400px
container().width(at_least(100).at_most(400)) // Range
```

### Fraction of Available Space

`fraction(f)` takes a fraction (0.0..=1.0) of the space offered by the
parent, resolved at layout time. It is the natural tool for
value-proportional bars — sliders, gauges, progress — because the width
follows the value on the very first frame, with no measured-rect
round-trip:

```rust
// A slider fill bar at the current volume
let volume = create_signal(40);
container().width(move || fraction(volume.get() as f32 / 100.0))
```

## Static or Reactive

Every property that survives to paint takes a signal, a closure, or a plain
value — the same `IntoSignal` shape everywhere, so which spelling you use is
never the API's decision:

```rust
container()
    .background(theme.surface)                       // a constant
    .background(surface_color)                       // a signal
    .background(move || if hot.get() { HOT } else { COOL })   // a closure
    .gradient(move || palette.get().header())
    .backdrop_blur(move || if frosted.get() { 24.0 } else { 0.0 })
    .overflow(move || if collapsed.get() { Overflow::Hidden } else { Overflow::Visible })
```

A blur radius of `0.0` is "no blur", on a container and on a text alike, which
is what lets one signal switch the effect on and off rather than forcing the
caller to rebuild the widget in a Rust branch.

**What is not reactive** is structural: `.layout(..)`, `.scrollable(..)`,
`.scrollbar(..)`, `.scrollbar_visibility(..)`, `.control()`, and the
`.animate_*()` declarations. These say
what kind of thing the container *is*; change one and you are describing a
different widget, so declare it in the closure that builds the widget instead.

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
        .when_hovered(|s| s.lighter(0.05).elevation(6.0))

        // Children
        .children([
            container().font_size(18.0).bold().text_color(Color::WHITE).child(text(title)),
            container().font_size(14.0).text_color(Color::rgb(0.7, 0.7, 0.75)).child(text(content)),
        ])
}
```
