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

### Font Weight

```rust
text("Bold text").bold()
```

### Font Style

```rust
text("Italic text").italic()
```

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

### Labels

```rust
text("LABEL")
    .font_size(11.0)
    .bold()
    .color(Color::rgb(0.5, 0.5, 0.55))
```

## Complete Example

```rust
fn article_card(title: &str, author: &str, preview: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.12, 0.12, 0.16))
        .corner_radius(8.0)
        .layout(Flex::column().spacing(8.0))
        .children([
            // Title
            text(title)
                .font_size(18.0)
                .bold()
                .color(Color::WHITE),
            // Author
            text(format!("By {}", author))
                .font_size(12.0)
                .color(Color::rgb(0.5, 0.5, 0.6)),
            // Preview text
            text(preview)
                .font_size(14.0)
                .color(Color::rgb(0.7, 0.7, 0.75)),
        ])
}
```

## API Reference

```rust
text(content: impl Into<String>) -> Text

impl Text {
    pub fn font_size(self, size: f32) -> Self;
    pub fn color(self, color: impl IntoMaybeDyn<Color>) -> Self;
    pub fn bold(self) -> Self;
    pub fn italic(self) -> Self;
    pub fn nowrap(self) -> Self;
}
```
