# Text

The Text widget renders text content with support for reactive updates.

## Basic Text

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
text("Hello, World!")
# ;
# }
```

## Styling

A text declares how it looks:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Copy)]
# struct Theme { text: Color, text_weak: Color, weak: Color, strong: Color, line: Color, accent: Color, error: Color, danger: Color, surface: Color }
# impl Default for Theme { fn default() -> Self { let c = Color::WHITE; Self { text: c, text_weak: c, weak: c, strong: c, line: c, accent: c, error: c, danger: c, surface: c } } }
# fn main() {
# let theme = Theme::default();
text("Hello").font_size(24.0).color(theme.text)
# ;
# }
```

The methods — `color`, `font_size`, `font_family`, `font_weight`, `bold`,
`mono`, `text_stroke`, `text_shadow` — belong to the two widgets that draw
glyphs, `Text` and `TextInput`, and are written from one list so the two always
offer the same thing.

`color` and `font_size` are *declarations*, so they say how they move as well as
what they are:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let busy = create_signal(false);
text("saving…").color((move || if busy.get() { Color::RED } else { Color::WHITE })
    .transition(200.0))
# ;
# }
```

A state override supplies a value and never a timing — `when_hovered(|s|
s.color(..))` takes a colour alone, and a transition there is a compile error
rather than something quietly ignored.

### Repeating a style

A container declares nothing about the text inside it: it draws a box, and the
widget that draws the glyphs is the one that says how they look. For "the same
kind of label, many times", write a function:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Copy)]
# struct Theme { text: Color, text_weak: Color, weak: Color, strong: Color, line: Color, accent: Color, error: Color, danger: Color, surface: Color }
# impl Default for Theme { fn default() -> Self { let c = Color::WHITE; Self { text: c, text_weak: c, weak: c, strong: c, line: c, accent: c, error: c, danger: c, surface: c } } }
# fn main() {
# let theme = Theme::default();
let label = |s: &str| text(s).color(theme.weak).font_size(12.0);

container()
    .layout(Flex::row().spacing(8.0))
    .children([label("one"), label("two"), label("three")])
# ;
# }
```

The style keeps a name, stays next to the widget that draws it, and costs no
extra node.

Properties nothing declares fall back to white, 14 logical pixels, the
registered default family and normal weight.

### A hover that reaches the glyphs

The pointer is over the *box*, and the colour belongs to the *glyphs*. Declare
each where it happens, and let `control()` join them — the text resolves its
own states from the nearest control above it:

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Copy)]
# struct Theme { text: Color, text_weak: Color, weak: Color, strong: Color, line: Color, accent: Color, error: Color, danger: Color, surface: Color }
# impl Default for Theme { fn default() -> Self { let c = Color::WHITE; Self { text: c, text_weak: c, weak: c, strong: c, line: c, accent: c, error: c, danger: c, surface: c } } }
# fn main() {
# let theme = Theme::default();
container()
    .padding(8.0)
    .control()
    .when_hovered(|s| s.lighter(0.1))
    .child(
        text("Label")
            .color(theme.weak)
            .when_hovered(|s| s.color(theme.strong)),
    )
# ;
# }
```

## Legibility over an image

Over a photograph no single text colour works, because the picture is light in
some places and dark in others. Two declarations separate the glyphs from
whatever is behind them:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    
    // A contour around the glyphs. CSS spells this `-webkit-text-stroke`;
    // CSS `outline` is the contour of a box, which is why the name differs.
    
    // As CSS `text-shadow`: offset x, offset y, blur, colour.
    
    .child(text("09:41").text_shadow(TextShadow::new(0.0, 2.0, 10.0, Color::rgba(0.0, 0.0, 0.0, 0.75))).text_stroke(TextStroke::new(1.5, Color::BLACK)).color(Color::WHITE))
# ;
# }
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
# extern crate guido;
# use guido::prelude::*;
# fn main() {
text("09:41")
    .font_size(76.0)
    // The tint over the glass; without one the glyphs are the blur alone.
    .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
    .backdrop_blur(16.0)
# ;
# }
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

- **It is declared on the text.** Every text property is declared on a
  container for the subtree below it; this one cannot, because each frosted
  text ends the render pass to filter the target. It is asked for one text at
  a time on purpose.
- **It is not legibility on its own.** Frost softens the background where a
  stroke and a shadow darken it. A stroke composes with it — over frost it is
  drawn as a true contour, dilated from the same coverage mask and laid outside
  the letter, which is what the glass needs and what a thick stroke wanted
  anyway. A shadow does not: it is still copies of the glyphs *under* the fill,
  so it covers the letter's own area as well as its edge, which is invisible
  under an opaque fill and an opaque letter over frost.
- **Rotation and scale skip it**, since the mask is rasterized square. A frost
  sitting beside its own letters would be worse than none.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
text("09:41")
    .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
    .backdrop_blur(16.0)
    // A contour, because this text is glass. Elsewhere the same declaration is
    // the cheap approximation, and under an opaque fill the two look alike.
    .text_stroke(TextStroke::new(2.0, Color::BLACK))
# ;
# }
```

`cargo run --example frosted_text` puts all of it beside a control.

## Styling

### Font Size

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Large text").font_size(24.0));
container().child(text("Small text").font_size(12.0))
# ;
# }
```

### Color

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Colored text").color(Color::rgb(0.9, 0.3, 0.3)));
container().child(text("White text").color(Color::WHITE))
# ;
# }
```

### Font Family

Set the font family using predefined families or custom font names:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Predefined font families
container().child(text("Sans-serif text").font_family(FontFamily::SansSerif));
container().child(text("Serif text").font_family(FontFamily::Serif));
container().child(text("Monospace text").font_family(FontFamily::Monospace));

// Shorthand for monospace
container().child(text("Code example"));

// Custom font by name (if available on system)
container().child(text("Custom font").font_family(FontFamily::Name("Inter".into())))
# ;
# }
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
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// Using constants
container().child(text("Thin text").font_weight(FontWeight::THIN));
container().child(text("Light text").font_weight(FontWeight::LIGHT));
container().child(text("Normal text").font_weight(FontWeight::NORMAL));
container().child(text("Medium text").font_weight(FontWeight::MEDIUM));
container().child(text("Semi-bold text").font_weight(FontWeight::SEMI_BOLD));
container().child(text("Bold text").font_weight(FontWeight::BOLD));
container().child(text("Black text").font_weight(FontWeight::BLACK));

// Shorthand for bold
container().child(text("Bold text"));

// Custom numeric weight
container().child(text("Custom weight").font_weight(FontWeight(550)))
# ;
# }
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
# extern crate guido;
# use guido::prelude::*;
# fn main() {
text("This text will not wrap").nowrap()
# ;
# }
```

## Reactive Text

Text content can update based on signals:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let message = create_signal("Hello".to_string());

text(move || message.get())
# ;
# }
```

### Formatted Reactive Text

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let count = create_signal(0);

text(move || format!("Count: {}", count.get()))
# ;
# }
```

## Combining Styles

Chain style methods:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Styled Text").font_family(FontFamily::Serif).font_size(18.0).color(Color::WHITE).nowrap())
# ;
# }
```

## Text in Containers

Text is typically placed inside containers for padding and backgrounds:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .padding(12.0)
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corners(4.0)
    .child(
        container().child(text("Button Label").font_size(14.0).color(Color::WHITE))
    )
# ;
# }
```

## Typography Patterns

### Headings

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Page Title").font_size(24.0).color(Color::WHITE))
# ;
# }
```

### Body Text

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Regular content text").font_size(14.0).color(Color::rgb(0.8, 0.8, 0.85)))
# ;
# }
```

### Secondary Text

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Subtitle or caption").font_size(12.0).color(Color::rgb(0.6, 0.6, 0.65)))
# ;
# }
```

### Code/Monospace Text

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("let x = 42;").font_size(13.0).color(Color::rgb(0.6, 0.9, 0.6)))
# ;
# }
```

### Labels

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("LABEL").font_size(11.0).color(Color::rgb(0.5, 0.5, 0.55)))
# ;
# }
```

## App-Level Default Font

Set a default font family for the entire application:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn view() -> Container { container() }
# fn main() {
# let config = SurfaceConfig::new();
App::new()
    .default_font_family(FontFamily::Name("Inter".into()))
    .run(|app| {
        app.add_surface(config, || view);
    });
# ;
# }
```

All text widgets will use this font family unless they explicitly override it.

## Complete Example

```rust,ignore
fn article_card(title: &str, author: &str, preview: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.12, 0.12, 0.16))
        .corners(8.0)
        .layout(Flex::column().spacing(8.0))
        .child(
            // Title - bold serif
            container().child(text(title).font_family(FontFamily::Serif).font_size(18.0).color(Color::WHITE))
        )
        .child(
            // Author - light weight
            container().child(text(format!("By {}", author)).font_weight(FontWeight::LIGHT).font_size(12.0).color(Color::rgb(0.5, 0.5, 0.6)))
        )
        .child(
            // Preview text
            container().child(text(preview).font_size(14.0).color(Color::rgb(0.7, 0.7, 0.75)))
        )
}
```

## API Reference

All properties accept static values, signals, or closures.

```rust,ignore
text(content: impl IntoSignal<String, M>) -> Text

impl Text {
    pub fn font_size<M>(self, size: impl IntoSignal<f32, M>) -> Self;  // numbers work: .font_size(16) or .font_size(16.5)
    pub fn color<M>(self, color: impl IntoSignal<Color, M>) -> Self;
    pub fn font_family<M>(self, family: impl IntoSignal<FontFamily, M>) -> Self;
    pub fn font_weight<M>(self, weight: impl IntoSignal<FontWeight, M>) -> Self;
    pub fn bold(self) -> Self;      // Shorthand for FontWeight::BOLD
    pub fn mono(self) -> Self;      // Shorthand for FontFamily::Monospace
    pub fn wrap<M>(self, wrap: impl IntoSignal<bool, M>) -> Self;
    pub fn nowrap(self) -> Self;    // Shorthand for wrap(false)
}
```
