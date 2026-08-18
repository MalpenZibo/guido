# Text

The Text widget renders text content with support for reactive updates.

## Basic Text

```rust
text("Hello, World!")
```

## Styling

A text declares how it looks:

```rust
text("Hello").font_size(24.0).color(theme.text).bold()
```

The methods come from the `TextStyled` trait — `color`, `font_size`,
`font_family`, `font_weight`, `bold`, `mono`, `text_stroke`, `text_shadow` —
implemented by the two widgets that draw glyphs, `Text` and `TextInput`.

### Dressing a whole subtree

The same properties can be declared on a container instead, where everything
below inherits them. Resolution is per property, so overriding the size leaves
a colour set further up alone:

```rust
container()
    .text_color(theme.text)
    .font_size(14.0)
    .layout(Flex::column().spacing(8.0))
    .child(text("inherits both"))
    .child(container().font_size(21.0).child(text("bigger, same colour")))
```

Containers that say nothing about text are transparent to this, so layout
wrappers do not interrupt it. A declaration on the text itself is the nearest
one there is, so it wins over every container above it:

```rust
container().text_color(theme.weak)
    .child(text("quiet"))
    .child(text("loud").color(theme.strong))
```

Properties nothing declares fall back to white, 14 logical pixels, the
registered default family and normal weight.

### Repeating a style

For "the same kind of label, many times", write a function rather than reaching
for a wrapper:

```rust
let label = |s: &str| text(s).color(theme.weak).font_size(12.0);

container()
    .layout(Flex::row().spacing(8.0))
    .children([label("one"), label("two"), label("three")])
```

The style keeps a name, stays next to the widget that draws it, and costs no
extra node. A container's inherited declaration is the right tool when the
texts are not yours to write — inside a component you are calling — or when
there are too many to name.

## Legibility over an image

Over a photograph no single text colour works, because the picture is light in
some places and dark in others. Two declarations separate the glyphs from
whatever is behind them:

```rust
container()
    .text_color(Color::WHITE)
    // A contour around the glyphs. CSS spells this `-webkit-text-stroke`;
    // CSS `outline` is the contour of a box, which is why the name differs.
    .text_stroke(TextStroke::new(1.5, Color::BLACK))
    // As CSS `text-shadow`: offset x, offset y, blur, colour.
    .text_shadow(TextShadow::new(0.0, 2.0, 10.0, Color::rgba(0.0, 0.0, 0.0, 0.75)))
    .child(text("09:41"))
```

The shadow is usually the more effective of the two: it darkens the whole
neighbourhood the glyph sits in rather than only its edge. Both are drawn
*under* the fill, so a stroke outlines the text instead of eating into it.

Both are approximated by re-drawing the glyphs at offsets, which costs fill rate
but no extra rasterization — the glyph atlas is keyed on glyph, size and weight,
and takes colour per draw. What decides whether the approximation shows is the
*spacing* between copies: more than a couple of pixels apart and they stop
blending, so the square features of a glyph — a colon's dots, the stem of a 4 —
read as separate copies rather than one halo. The shadow therefore fills a disc
whose sample count grows with the radius (about a hundred copies at blur 10), and
a very wide blur spreads a fixed budget thinner instead of costing more. A thick
stroke is the case with a visible ceiling: its corners begin to scallop past a
few pixels, where the honest fix is a dilate on an offscreen mask, not more taps.

Neither changes how much room the text takes, so adding a shadow never moves
its neighbours.

`cargo run --example text_decoration_example` shows both against a control.

## Frosted glass

The third way to sit a text on a picture is to make the letters a window onto
it, blurred:

```rust
text("09:41")
    .font_size(76.0)
    // The tint over the glass; without one the glyphs are the blur alone.
    .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
    .backdrop_blur(16.0)
```

This is the same effect a container gets from
[`backdrop_blur`](../concepts/container.md), with the shape of the letters in
place of the shape of the box — CSS's `backdrop-filter` together with
`background-clip: text`. The renderer rasterizes the glyphs into a coverage
mask, blurs the region they cover, and composites the result back through the
mask; the text is then drawn over its own frost, which is why the colour reads
as a tint.

It filters what *this surface* has already drawn — a wallpaper, a photo, the
panel underneath. A container's blur can also reach what the compositor puts
behind the surface, through `ext-background-effect-v1`; that protocol takes a
region, and regions are rectangles, so glyphs cannot be expressed in it.

Three things to know before reaching for it:

- **It is not inherited.** Every other text property can be declared on a
  container for the subtree below it; this one cannot, because each frosted
  text ends the render pass to filter the target. It is asked for one text at
  a time on purpose.
- **It is not legibility.** Frost softens the background where a shadow darkens
  it, so over a busy photograph the shadow still does more. They compose.
- **Rotation and scale skip it**, since the mask is rasterized square. A frost
  sitting beside its own letters would be worse than none.

`cargo run --example frosted_text` puts it beside a shadow and a control.

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
