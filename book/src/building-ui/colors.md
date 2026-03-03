# Colors

Guido provides a simple color system for styling widgets.

## Creating Colors

### RGB (0.0-1.0 range)

```rust
Color::rgb(0.2, 0.4, 0.8)  // R, G, B values from 0.0 to 1.0
```

### RGBA with Alpha

```rust
Color::rgba(0.2, 0.4, 0.8, 0.5)  // 50% transparent
```

### From 8-bit Values (0-255)

```rust
Color::from_rgb8(51, 102, 204)       // Same as Color::rgb(0.2, 0.4, 0.8)
Color::from_rgba8(51, 102, 204, 128) // With alpha
```

### From Hex

```rust
Color::from_hex(0x3366CC)  // Hex RGB value
```

### Predefined Colors

```rust
Color::WHITE
Color::BLACK
Color::RED
Color::GREEN
Color::BLUE
Color::YELLOW
Color::CYAN
Color::MAGENTA
Color::GRAY
Color::TRANSPARENT
```

## Color Operations

### Lightening and Darkening

```rust
let lighter = color.lighter(0.1);  // 10% toward white
let darker = color.darker(0.2);    // 20% toward black
```

### Mixing

Linear interpolation between two colors:

```rust
let blend = color_a.mix(color_b, 0.5);  // 50/50 blend
let mostly_a = color_a.mix(color_b, 0.2);  // 80% A, 20% B
```

### Invert

```rust
let inverted = color.invert();  // Flip RGB channels
```

### Grayscale

Convert to perceptual grayscale using Rec. 709 luminance weights:

```rust
let gray = color.grayscale();
```

### Luminance

Get the perceived brightness (0.0 = black, 1.0 = white):

```rust
let brightness = color.luminance();
```

### Alpha Manipulation

```rust
let semi = color.with_alpha(0.5);      // Set alpha to 0.5
let faded = color.scale_alpha(0.5);    // Halve the current alpha
```

### Convert to 8-bit

```rust
let (r, g, b, a) = color.to_rgba8();  // Each 0-255
```

## Using Colors with State Layers

Colors integrate with the state layer API for hover effects:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .hover_state(|s| s.lighter(0.1))   // Lighten on hover
    .pressed_state(|s| s.darker(0.1))  // Darken on press
```

Or use explicit colors:

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .hover_state(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))
    .pressed_state(|s| s.background(Color::rgb(0.2, 0.4, 0.7)))
```

## Reactive Colors

Colors can be reactive using signals:

```rust
let bg_color = create_signal(Color::rgb(0.2, 0.2, 0.3));

container().background(bg_color)
```

Or closures:

```rust
let is_active = create_signal(false);

container().background(move || {
    if is_active.get() {
        Color::rgb(0.3, 0.6, 0.4)  // Green when active
    } else {
        Color::rgb(0.2, 0.2, 0.3)  // Gray when inactive
    }
})
```

## Color Tips

### Dark Theme Palette

For dark UIs, use low-saturation colors with subtle variation:

```rust
let background = Color::rgb(0.08, 0.08, 0.12);  // Near black
let surface = Color::rgb(0.12, 0.12, 0.16);     // Slightly lighter
let primary = Color::rgb(0.3, 0.5, 0.8);        // Accent blue
let text = Color::rgb(0.9, 0.9, 0.95);          // Near white
let text_secondary = Color::rgb(0.6, 0.6, 0.7); // Muted text
```

### Hover States

For hover states, lightening by 5-15% works well:

```rust
// Subtle hover
.hover_state(|s| s.lighter(0.05))

// Noticeable hover
.hover_state(|s| s.lighter(0.1))

// Strong hover
.hover_state(|s| s.lighter(0.15))
```

### Transparency

Use alpha for overlays and effects:

```rust
// Semi-transparent overlay
let overlay = Color::rgba(0.0, 0.0, 0.0, 0.5);

// Or use with_alpha on an existing color
let overlay = Color::BLACK.with_alpha(0.5);

// Ripple color (40% opacity white)
let ripple = Color::WHITE.with_alpha(0.4);
```
