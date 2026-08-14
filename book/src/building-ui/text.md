# Text

The Text widget renders text content with support for reactive updates.

## Basic Text

```rust
text("Hello, World!")
```

## Where styling lives

Guido has one styling widget. A `text` carries the string and nothing else —
how it looks is declared on an enclosing container, next to that container's
background, border and animations:

```rust
container().font_size(24.0).text_color(theme.text).child(text("Hello"))
```

Each property is inherited by everything below, until a nearer container
overrides it. Resolution is per property, so overriding the size leaves a
colour set further up alone:

```rust
container()
    .text_color(theme.text)
    .font_size(14.0)
    .layout(Flex::column().spacing(8.0))
    .child(text("inherits both"))
    .child(container().font_size(21.0).child(text("bigger, same colour")))
```

Containers that say nothing about text are transparent to this, so layout
wrappers do not interrupt it. Properties nothing declares fall back to white,
14 logical pixels, the registered default family and normal weight.

## Styling

### Font Size

```rust
container().font_size(24.0).child(text("Large text"))
container().font_size(12.0).child(text("Small text"))
```

### Color

```rust
container().text_color(Color::rgb(0.9, 0.3, 0.3)).child(text("Colored text"))
container().text_color(Color::WHITE).child(text("White text"))
```

### Font Family

Set the font family using predefined families or custom font names:

```rust
// Predefined font families
container().font_family(FontFamily::SansSerif).child(text("Sans-serif text"))
container().font_family(FontFamily::Serif).child(text("Serif text"))
container().font_family(FontFamily::Monospace).child(text("Monospace text"))

// Shorthand for monospace
container().mono().child(text("Code example"))

// Custom font by name (if available on system)
container().font_family(FontFamily::Name("Inter".into())).child(text("Custom font"))
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
container().font_weight(FontWeight::THIN).child(text("Thin text"))
container().font_weight(FontWeight::LIGHT).child(text("Light text"))
container().font_weight(FontWeight::NORMAL).child(text("Normal text"))
container().font_weight(FontWeight::MEDIUM).child(text("Medium text"))
container().font_weight(FontWeight::SEMI_BOLD).child(text("Semi-bold text"))
container().font_weight(FontWeight::BOLD).child(text("Bold text"))
container().font_weight(FontWeight::BLACK).child(text("Black text"))

// Shorthand for bold
container().bold().child(text("Bold text"))

// Custom numeric weight
container().font_weight(FontWeight(550)).child(text("Custom weight"))
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
container().font_size(18.0).text_color(Color::WHITE).font_family(FontFamily::Serif).bold().child(text("Styled Text").nowrap())
```

## Text in Containers

Text is typically placed inside containers for padding and backgrounds:

```rust
container()
    .padding(12.0)
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corner_radius(4.0)
    .child(
        container().text_color(Color::WHITE).font_size(14.0).child(text("Button Label"))
    )
```

## Typography Patterns

### Headings

```rust
container().font_size(24.0).bold().text_color(Color::WHITE).child(text("Page Title"))
```

### Body Text

```rust
container().font_size(14.0).text_color(Color::rgb(0.8, 0.8, 0.85)).child(text("Regular content text"))
```

### Secondary Text

```rust
container().font_size(12.0).text_color(Color::rgb(0.6, 0.6, 0.65)).child(text("Subtitle or caption"))
```

### Code/Monospace Text

```rust
container().mono().font_size(13.0).text_color(Color::rgb(0.6, 0.9, 0.6)).child(text("let x = 42;"))
```

### Labels

```rust
container().font_size(11.0).bold().text_color(Color::rgb(0.5, 0.5, 0.55)).child(text("LABEL"))
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
            container().font_size(18.0).font_family(FontFamily::Serif).bold().text_color(Color::WHITE).child(text(title))
        )
        .child(
            // Author - light weight
            container().font_size(12.0).font_weight(FontWeight::LIGHT).text_color(Color::rgb(0.5, 0.5, 0.6)).child(text(format!("By {}", author)))
        )
        .child(
            // Preview text
            container().font_size(14.0).text_color(Color::rgb(0.7, 0.7, 0.75)).child(text(preview))
        )
}
```

## API Reference

All properties accept static values, signals, or closures.

```rust
text(content: impl IntoSignal<String, M>) -> Text

impl Text {
    pub fn font_size<M>(self, size: impl IntoSignal<f32, M>) -> Self;  // numbers work: .font_size(16) or .font_size(16.5)
    pub fn color<M>(self, color: impl IntoSignal<Color, M>) -> Self;
    pub fn font_family<M>(self, family: impl IntoSignal<FontFamily, M>) -> Self;
    pub fn font_weight<M>(self, weight: impl IntoSignal<FontWeight, M>) -> Self;
    pub fn bold(self) -> Self;      // Shorthand for FontWeight::BOLD
    pub fn mono(self) -> Self;      // Shorthand for FontFamily::Monospace
    pub fn nowrap(self) -> Self;
}
```
