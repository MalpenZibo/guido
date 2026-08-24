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

A border is always both halves at once — on the container and in a state layer
alike. A width with no colour and a colour with no width are the same thing, no
border, so there is nothing for a half-declaration to mean and no way to write
one. Each half takes a signal of its own, which covers anything that has to
change:

```rust
container().border(1.5, move || if failed.get() { theme.danger } else { theme.line })
```

### In a State Layer

Same call, and it replaces the whole border. Layers resolve last-declared-wins,
so the border a caller wants to outrank the others goes last:

```rust
container()
    .border(1.5, theme.line)
    .when_focused(|s| s.border(1.5, theme.accent))
    .state(failed, |s| s.border(1.5, theme.danger))
```

A width repeated across layers is a constant in your own code, not something the
API should let you leave half-said:

```rust
const FIELD_BORDER: f32 = 1.5;
```

### Animated Borders

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
```

## Corner Radius

### Uniform Radius

```rust
container().corner_radius(8.0)  // 8px radius on all corners
```

### Per-Corner Radii

Round each corner independently with `corner_radii` — accordion-style
lists round only the top of the first row and only the bottom of the last:

```rust
use guido::prelude::CornerRadii;

container().corner_radii(CornerRadii::top(16.0))     // first row
container().corner_radii(CornerRadii::bottom(16.0))  // last row
container().corner_radii(CornerRadii {
    top_left: 16.0,
    top_right: 4.0,
    bottom_right: 16.0,
    bottom_left: 4.0,
})
```

`corner_radii` overrides `corner_radius` for drawing (background, border,
shadow, gradient). Child clipping, blur regions and rounded hit testing
keep using a uniform radius — the largest of the four.

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
    .when_hovered(|s| s.elevation(4.0))
    .when_pressed(|s| s.elevation(1.0))
```

## Padding

```rust
container().padding(16.0)                          // 16px on all sides
container().padding(16)                            // integers work too
container().padding([8.0, 16.0])                   // [vertical, horizontal]
container().padding([8, 16])                       // integer arrays work too
container().padding([1.0, 2.0, 3.0, 4.0])         // [top, right, bottom, left]
container().padding([1, 2, 3, 4])                  // integer 4-value shorthand
container().padding(Padding::all(8.0).with_top(20.0))   // builder pattern
```

## Sizing

### Fixed Size

```rust
container()
    .width(100.0)
    .height(50.0)

// Integers work too
container()
    .width(100)
    .height(50)
```

### Minimum/Maximum Size

```rust
container()
    .width(at_least(50.0).at_most(200.0))
    .height(at_least(30.0).at_most(100.0))
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

## Text Styling

How a text looks can be declared on the text itself:

```rust
text("Hello").font_size(16.0).color(Color::WHITE).bold()
```

`color`, `font_size`, `font_family`, `font_weight`, `bold`, `mono`,
`text_stroke` and `text_shadow` come from the `TextStyled` trait, implemented
by `Text` and `TextInput` — the two widgets that draw glyphs. A `TextInput`
also implements `InputStyled` for `cursor_color`, `selection_color` and
`placeholder_color`, which only it can draw.

### The same style, many times: write a function

There is no inheritance: a container declares nothing about the text inside
it, because a container draws a box. For "the same kind of label, many times",
the tool is the one the language already gives you:

```rust
let label = |s: &str| text(s).color(theme.weak).font_size(12.0);

container()
    .layout(Flex::row().spacing(8.0))
    .children([label("one"), label("two"), label("three")])
```

That keeps the declaration next to the widget that draws it, costs no wrapper
node, and gives the style a name.

### A state that reaches the glyphs

Hover belongs to the box and colour to the glyphs, so each is declared where it
happens and [`control()`](../docs/STATE_LAYER.md) joins them: the leaf resolves
its own states from the nearest control above it.

```rust
container()
    .padding(8.0)
    .control()
    .when_hovered(|s| s.lighter(0.1))
    .child(
        text("Label")
            .color(theme.weak)
            .when_hovered(|s| s.color(theme.strong)),
    )
```

### Properties with no declaration

Fall back to white, 14 logical pixels, the registered default family and normal
weight.

### Stroke and shadow

For text over an image, where no single colour works against the whole
picture:

```rust
container()
    
    
    
    .child(text("09:41").text_shadow(TextShadow::new(0.0, 2.0, 10.0, Color::rgba(0.0, 0.0, 0.0, 0.75))).text_stroke(TextStroke::new(1.5, Color::BLACK)).color(Color::WHITE))
```

Both are drawn under the fill — a stroke painted over it would eat half the
weight of every stem. Neither affects layout.

## Layout Styling

### Flex Layout

```rust
container()
    .layout(
        Flex::row()
            .spacing(8.0)
            .main_alignment(MainAlignment::Center)
            .cross_alignment(CrossAlignment::Center)
    )
```

### Alignment Options

**Main Axis (direction of flow):**
- `MainAlignment::Start`
- `MainAlignment::End`
- `MainAlignment::Center`
- `MainAlignment::SpaceBetween`
- `MainAlignment::SpaceAround`
- `MainAlignment::SpaceEvenly`

**Cross Axis (perpendicular to flow):**
- `CrossAlignment::Start`
- `CrossAlignment::End`
- `CrossAlignment::Center`
- `CrossAlignment::Stretch`

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
            container().child(text(title).color(Color::WHITE)),
            container().child(text(content).bold().font_size(14.0).font_size(18.0).color(Color::rgb(0.7, 0.7, 0.75))),
        ])
}
```
