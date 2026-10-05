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
`mono`, `line_height`, `letter_spacing`, `text_stroke`, `text_shadow` — belong to the two widgets that draw
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
- **A transform carries it along.** The coverage is cut in the text's own
  space and read back through the transform, so a rotated or scaled text keeps
  a frost registered with its letters — which is what an `animate_transform` on
  a frosted label needs, and what it used to lose for the length of the
  animation. A transformed text is rasterized finer for it, so a very large one
  can reach the coverage size ceiling and lose the frost with a warning in the
  log; it is an effect for a label.

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
container().child(text("Custom font").font_family(FontFamily::name("Inter")))
# ;
# }
```

Available font families:
- `FontFamily::SansSerif` - Default sans-serif font
- `FontFamily::Serif` - Serif font
- `FontFamily::Monospace` - Monospace/fixed-width font
- `FontFamily::Cursive` - Cursive font
- `FontFamily::Fantasy` - Fantasy/decorative font
- `FontFamily::name(&str)` - Custom font by name, interned

### Font Weight

Set the font weight using predefined constants or numeric values (100-900).
A weight the font has no face for is drawn in the nearest one it has — a
semibold in a family with only regular and bold comes out bold — never in a
different font:

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

### Line Height

Each line is as tall as the font says by default: its ascent, descent and line
gap, scaled to the size. A bare number is a multiple of the font size, as in
CSS; `LineHeight::Absolute` is logical pixels, which is how a design system
writes a type scale such as 14/20:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container().child(text("Loose notes").line_height(1.5));
container().child(text("Body").font_size(14.0).line_height(LineHeight::Absolute(20.0)))
# ;
# }
```

A `TextInput` is one line tall, so a field and a label with the same style
have the same height.

### Letter Spacing

Extra room after every character, in logical pixels — the unit a design token
such as a tracking of `1.5` is written in, so it is the same distance at any
font size. `0.0` is the default and the font's own spacing; a negative value
draws the letters tighter. Like every other style, it takes a signal as readily
as a value, and a state can supply it:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let wide = create_signal(false);
container().child(text("SECTION").font_size(11.0).letter_spacing(1.5));
container().child(text("Display").font_size(40.0).letter_spacing(-1.0));
container().child(
    text("Hover me")
        .letter_spacing(move || if wide.get() { 4.0 } else { 0.0 })
        .when_hovered(|s| s.letter_spacing(2.0)),
)
# ;
# }
```

The text is measured with the spacing, so the box it takes grows and shrinks
with it, and a text that wraps wraps where the spaced line no longer fits. A
`TextInput` puts its caret, its selection and its clicks where the spaced
letters are, and a `PasswordInput` spaces the dots it draws.

What the spacing is added to is decided by the shaper, cosmic-text: every
glyph it puts out gets it, the last one on a line included. Most of the time a
glyph is a character, but not always:

- a ligature — Arabic lam and alef, drawn as one shape — takes one spacing for
  the two letters;
- an accent the font can combine with its letter, such as `e` and a combining
  acute, becomes one glyph `é` and takes one; an accent it cannot combine is a
  glyph of its own, takes a second, and is drawn that far to the right of its
  letter;
- joined Arabic letters are pulled apart, and the joins between them are not
  stretched to cover the gap.

A value that is not a number — a division by a zero somewhere upstream — is
treated as no spacing at all.

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

### Text Alignment

`align` says where each line sits across the text's box. The box is the
text's own: as wide as its widest line, so a wrapped label lines its lines up
against each other, and as wide as the stretch when a layout stretches it.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .width(160.0)
    .child(text("A label long enough to wrap onto a second line").align(TextAlign::Center))
# ;
# }
```

- `TextAlign::Start` (the default) — where the text's direction begins: the
  left for Latin, the right for Arabic or Hebrew
- `TextAlign::Center` — each line centred
- `TextAlign::End` — where the text's direction ends
- `TextAlign::Justified` — every line but a paragraph's last stretched to the
  full width. A justified text that wraps is as wide as the width its
  container offered, since that is what its lines are stretched across, as a
  justified paragraph is in CSS; one that fits on its line hugs it

A text whose box fits its only line looks the same whichever alignment it has:
there is no room for the line to move into. To centre a short label in a wider
space, centre the text in its container, or stretch it across the container
and centre its line.

A line wider than its box — an unwrapped text in a narrow container — starts
at the start and runs off the end, whatever its alignment, as it does in CSS.

`align` takes a signal like the other text properties. A change moves the
glyphs inside the box, and lays the text out again, since a justified text
that wraps is wider than one aligned any other way.

### Limiting Lines

A text that has to fit a fixed box — a window title in a bar, a track name in
a player — can be held to a number of lines, with the cut marked:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let title = "a window title far too long for the bar";
text(title).max_lines(1).overflow(TextOverflow::Ellipsis)
# ;
# }
```

`max_lines` says how many lines the text may take, and the text is measured as
those lines, not as everything it holds. `overflow` says what marks the cut:

- `TextOverflow::Clip` (the default) — the lines past the limit are not drawn
- `TextOverflow::Ellipsis` — the last line drawn ends in `…`
- `TextOverflow::EllipsisStart` — the `…` opens the last line, which keeps the
  end of the text: a path whose file name is what matters
- `TextOverflow::EllipsisMiddle` — the `…` stands in the middle, keeping both
  ends

A line break counts toward the limit like a wrapped line does. A text that fits
is drawn exactly as it would be with no limit.

An unwrapped text is cut by its box already, so an ellipsis on a `nowrap()`
text needs no limit — each line that runs past the box ends in `…`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .width(120.0)
    .child(text("a long track name").nowrap().overflow(TextOverflow::Ellipsis))
# ;
# }
```

Both are reactive like every other text property: `max_lines` takes an
`Option<u32>`, so a signal can lift the limit with `None`.

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

```rust,no_run
# extern crate guido;
# use guido::prelude::*;
# fn view() -> Container { container() }
# fn main() {
# let config = SurfaceConfig::new();
App::new()
    .default_font_family(FontFamily::name("Inter"))
    .run(|app| {
        app.add_surface(config, || view());
    });
# ;
# }
```

All text widgets will use this font family unless they explicitly override it.

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
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
# fn main() {}
```

## API Reference

All properties accept static values, signals, or closures.

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
text(content: impl IntoSignal<String, M>) -> Text

impl Text {
    pub fn font_size<M>(self, size: impl IntoSignal<f32, M>) -> Self;  // numbers work: .font_size(16) or .font_size(16.5)
    pub fn color<M>(self, color: impl IntoSignal<Color, M>) -> Self;
    pub fn font_family<M>(self, family: impl IntoSignal<FontFamily, M>) -> Self;
    pub fn font_weight<M>(self, weight: impl IntoSignal<FontWeight, M>) -> Self;
    pub fn bold(self) -> Self;      // Shorthand for FontWeight::BOLD
    pub fn mono(self) -> Self;      // Shorthand for FontFamily::Monospace
    pub fn letter_spacing<M>(self, spacing: impl IntoSignal<f32, M>) -> Self;  // logical pixels
    pub fn wrap<M>(self, wrap: impl IntoSignal<bool, M>) -> Self;
    pub fn nowrap(self) -> Self;    // Shorthand for wrap(false)
    pub fn align<M>(self, align: impl IntoSignal<TextAlign, M>) -> Self;
    pub fn max_lines<M>(self, lines: impl IntoSignal<Option<u32>, M>) -> Self;  // .max_lines(2)
    pub fn overflow<M>(self, overflow: impl IntoSignal<TextOverflow, M>) -> Self;
}
```
