# Text

The Text widget renders styled text content with support for reactive updates.

## Basic Text

```rust
text("Hello, World!")
```

## Styling

### Font Size

```rust
text("Large text").font_size(24.0)
text("Small text").font_size(12.0)
```

### Color

```rust
text("Colored text").color(Color::rgb(0.9, 0.3, 0.3))
text("White text").color(Color::WHITE)
```

### Font Family

Set the font family using predefined families or custom font names:

```rust
// Predefined font families
text("Sans-serif text").font_family(FontFamily::SansSerif)
text("Serif text").font_family(FontFamily::Serif)
text("Monospace text").font_family(FontFamily::Monospace)

// Shorthand for monospace
text("Code example").mono()

// Custom font by name (if available on system)
text("Custom font").font_family(FontFamily::Name("Inter".into()))
```

Available font families:
- `FontFamily::SansSerif` - Default sans-serif font
- `FontFamily::Serif` - Serif font
- `FontFamily::Monospace` - Monospace/fixed-width font
- `FontFamily::Cursive` - Cursive font
- `FontFamily::Fantasy` - Fantasy/decorative font
- `FontFamily::Name(String)` - Custom font by name

### Font Weight

Set the font weight using predefined constants or numeric values (100-900):

```rust
// Using constants
text("Thin text").font_weight(FontWeight::THIN)
text("Light text").font_weight(FontWeight::LIGHT)
text("Normal text").font_weight(FontWeight::NORMAL)
text("Medium text").font_weight(FontWeight::MEDIUM)
text("Semi-bold text").font_weight(FontWeight::SEMI_BOLD)
text("Bold text").font_weight(FontWeight::BOLD)
text("Black text").font_weight(FontWeight::BLACK)

// Shorthand for bold
text("Bold text").bold()

// Custom numeric weight
text("Custom weight").font_weight(FontWeight(550))
```

Available weight constants:
- `FontWeight::THIN` (100)
- `FontWeight::EXTRA_LIGHT` (200)
- `FontWeight::LIGHT` (300)
- `FontWeight::NORMAL` (400)
- `FontWeight::MEDIUM` (500)
- `FontWeight::SEMI_BOLD` (600)
- `FontWeight::BOLD` (700)
- `FontWeight::EXTRA_BOLD` (800)
- `FontWeight::BLACK` (900)

### Text Wrapping

By default, text wraps to fit the available width. Disable wrapping for single-line text:

```rust
text("This text will not wrap").nowrap()
```

## Reactive Text

Text content can update based on signals:

```rust
let message = create_signal("Hello".to_string());

text(move || message.get())
```

### Formatted Reactive Text

```rust
let count = create_signal(0);

text(move || format!("Count: {}", count.get()))
```

## Combining Styles

Chain style methods:

```rust
text("Styled Text")
    .font_size(18.0)
    .color(Color::WHITE)
    .font_family(FontFamily::Serif)
    .bold()
    .nowrap()
```

## Text in Containers

Text is typically placed inside containers for padding and backgrounds:

```rust
container()
    .padding(12.0)
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(4.0)
    .child(
        text("Button Label")
            .color(Color::WHITE)
            .font_size(14.0)
    )
```

## Typography Patterns

### Headings

```rust
text("Page Title")
    .font_size(24.0)
    .bold()
    .color(Color::WHITE)
```

### Body Text

```rust
text("Regular content text")
    .font_size(14.0)
    .color(Color::rgb(0.8, 0.8, 0.85))
```

### Secondary Text

```rust
text("Subtitle or caption")
    .font_size(12.0)
    .color(Color::rgb(0.6, 0.6, 0.65))
```

### Code/Monospace Text

```rust
text("let x = 42;")
    .mono()
    .font_size(13.0)
    .color(Color::rgb(0.6, 0.9, 0.6))
```

### Labels

```rust
text("LABEL")
    .font_size(11.0)
    .bold()
    .color(Color::rgb(0.5, 0.5, 0.55))
```

## App-Level Default Font

Set a default font family for the entire application:

```rust
App::new()
    .default_font_family(FontFamily::Name("Inter".into()))
    .run(|app| {
        app.add_surface(config, || view);
    });
```

All text widgets will use this font family unless they explicitly override it.

## Complete Example

```rust
fn article_card(title: &str, author: &str, preview: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.12, 0.12, 0.16))
        .corner_radius(8.0)
        .layout(Flex::column().spacing(8.0))
        .child(
            // Title - bold serif
            text(title)
                .font_size(18.0)
                .font_family(FontFamily::Serif)
                .bold()
                .color(Color::WHITE)
        )
        .child(
            // Author - light weight
            text(format!("By {}", author))
                .font_size(12.0)
                .font_weight(FontWeight::LIGHT)
                .color(Color::rgb(0.5, 0.5, 0.6))
        )
        .child(
            // Preview text
            text(preview)
                .font_size(14.0)
                .color(Color::rgb(0.7, 0.7, 0.75))
        )
}
```

## API Reference

```rust
text(content: impl IntoMaybeDyn<String>) -> Text

impl Text {
    pub fn font_size(self, size: impl IntoMaybeDyn<f32>) -> Self;
    pub fn color(self, color: impl IntoMaybeDyn<Color>) -> Self;
    pub fn font_family(self, family: impl IntoMaybeDyn<FontFamily>) -> Self;
    pub fn font_weight(self, weight: impl IntoMaybeDyn<FontWeight>) -> Self;
    pub fn bold(self) -> Self;      // Shorthand for FontWeight::BOLD
    pub fn mono(self) -> Self;      // Shorthand for FontFamily::Monospace
    pub fn nowrap(self) -> Self;
}
```
