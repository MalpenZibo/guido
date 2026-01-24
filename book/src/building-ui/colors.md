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

### Predefined Colors

```rust
Color::WHITE
Color::BLACK
Color::RED
Color::GREEN
Color::BLUE
Color::TRANSPARENT
```

## Color Operations

### Lightening

Blend toward white:

```rust
let lighter = color.lighter(0.1);  // 10% toward white
let lighter = color.lighter(0.3);  // 30% toward white
```

### Darkening

Blend toward black:

```rust
let darker = color.darker(0.1);  // 10% toward black
let darker = color.darker(0.3);  // 30% toward black
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

// Ripple color (40% opacity white)
let ripple = Color::rgba(1.0, 1.0, 1.0, 0.4);
```
