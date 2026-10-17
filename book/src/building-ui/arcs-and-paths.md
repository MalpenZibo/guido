# Arcs and Paths

A container draws boxes. For a progress ring, a gauge, a sparkline or an
outline that is not a rounded rect, there are two more things to draw: an
`arc()`, which is a widget, and a path, which a widget of your own draws.

## Arcs

`arc()` draws part of a ring centred in a square box — a battery level, a
countdown, a volume gauge.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let level = create_signal(64.0_f32);

arc()
    .size(18.0)                              // the box, in logical pixels
    .stroke(2.6)                             // the line's width
    .track(Color::WHITE.with_alpha(0.16))    // the whole sweep, underneath
    .color(Color::rgb(0.4, 0.85, 0.5))       // the covered part
    .caps(Cap::Round)
    .progress((move || level.get() / 100.0).transition(1000.0))
# ;
# }
```

The line runs inside the box, its outer edge touching the box's sides. It
starts at `start` degrees, measured clockwise from twelve o'clock, and runs
`sweep` degrees — a full turn unless you say otherwise. `progress`, from 0 to 1,
is how much of that sweep is covered in `color`; the `track`, if you give one,
is drawn under all of it.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// A gauge open at the bottom: three quarters of a turn, from lower left.
arc()
    .size(48.0)
    .stroke(5.0)
    .start(-135.0)
    .sweep(270.0)
    .track(Color::rgba(1.0, 1.0, 1.0, 0.12))
    .caps(Cap::Round)
    .progress(0.6)
# ;
# }
```

A negative sweep runs anticlockwise. Progress 0 draws the track alone, even
with round caps — not a dot.

`progress` and `color` are animatable, like a container's background: give
either a `.transition(..)` and it eases to every new value. That costs nothing
per frame beyond the frame itself, because an arc is drawn exactly, as one
shape, rather than cut into triangles — a ring easing for a second is a number
changing sixty times, never a new shape to build.

| Property | Default | Reactive | Animatable |
| --- | --- | --- | --- |
| `size` | 24 | yes | |
| `stroke` | 2 | yes | |
| `color` | white | yes | yes |
| `track` | none | yes | |
| `caps` | `Cap::Butt` | yes | |
| `start` | 0 | yes | |
| `sweep` | 360 | yes | |
| `progress` | 0 | yes | yes |

## Paths

Anything else is a `Path`, drawn by a widget of your own — see [Custom
Widgets](../advanced/custom-widgets.md) for the rest of what one is. A path is
built a segment at a time, in the widget's local coordinates:

```rust
# extern crate guido;
# use guido::prelude::*;
# use guido::widget_prelude::*;
# fn main() {
let outline = Path::new()
    .move_to((0.0, 20.0))
    .line_to((10.0, 4.0))
    .quad_to((15.0, -2.0), (20.0, 4.0))
    .cubic_to((24.0, 10.0), (28.0, 18.0), (30.0, 20.0))
    .close();
# let _ = outline;
# }
```

`PaintContext::draw_path` fills it, strokes it, or both — the fill first, and
the stroke centred on the outline over it:

```rust
# extern crate guido;
# use guido::prelude::*;
# use guido::widget_prelude::*;
struct Sparkline {
    samples: Signal<Vec<f32>>,
}

impl Widget for Sparkline {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        constraints.constrain(Size::new(80.0, 20.0))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Read while painting, so a new sample repaints this widget alone.
        let samples = self.samples.get();
        let step = 80.0 / (samples.len().max(2) - 1) as f32;
        let line = samples.iter().enumerate().fold(Path::new(), |path, (i, v)| {
            let point = (i as f32 * step, 20.0 * (1.0 - v));
            if i == 0 { path.move_to(point) } else { path.line_to(point) }
        });
        let area = line.clone().line_to((80.0, 20.0)).line_to((0.0, 20.0)).close();

        let blue = Color::rgb(0.45, 0.8, 1.0);
        ctx.draw_path(&area, Fill::color(blue.with_alpha(0.2)), None);
        ctx.draw_path(&line, None, Stroke::new(1.6, blue).joins(Join::Round));
    }
}
# fn main() {}
```

A `Fill` is a colour and a `FillRule`: `NonZero` by default, as in SVG and
Canvas, or `EvenOdd`, under which a subpath inside another cuts a hole whichever
way it winds. A `Stroke` is a width, a colour, a `Cap` for the ends of an open
line — `Butt`, `Round` or `Square` — a `Join` where two segments meet —
`Miter`, `Round` or `Bevel` — and a miter limit, 4 by default as in SVG.

The edges are anti-aliased, a pixel wide at any scale, and a translucent stroke
is one tone from end to end: a join is not drawn twice.

### Dashes

`Stroke::dashes` cuts the line into dashes: the lengths of a dash, a gap, a dash
and so on, in logical pixels, and how far into that pattern the path starts.

```rust
# extern crate guido;
# use guido::prelude::*;
# use guido::widget_prelude::*;
# fn main() {
let threshold = Stroke::new(1.0, Color::WHITE).dashes(&[6.0, 4.0], 0.0);
let dotted = Stroke::new(2.0, Color::WHITE)
    .caps(Cap::Round)
    .dashes(&[0.0, 5.0], 0.0);
# let _ = (threshold, dotted);
# }
```

Each dash is a line of its own, capped at both ends and joined at every corner
it turns, so a dash of no length with round caps is a dot. The pattern starts
again on each subpath, and on a closed one the dash that runs through the start
is one dash. An odd number of lengths is repeated, as in SVG and Canvas; a
pattern that adds up to nothing draws the line solid.

`draw_arc` takes a dashed `Stroke` too. A solid arc is one exact shape, cheap
to animate; a dashed one is drawn as a path, so it is cut and its triangles
kept like any other, and a sweep or an offset that moves every frame is new
triangles every frame.

### What a path costs

A path is cut into triangles the first time it is drawn at a scale, and the
triangles are kept, keyed by the path's segments and how it is filled or
stroked. Drawing the same segments again — moved, turned, faded or in another
colour — costs no new triangles. New segments do: a sparkline that shifts by a
sample is a new path, which is fine once a second and worth knowing about at
sixty frames a second. An animated sweep is what `arc()` is for.

The dash pattern and its offset are part of that key, the offset kept within
one pass of the pattern. Dashes that march along a path — an offset that moves
every frame — are new triangles every frame for that one path, until the
offset comes back round to one it has drawn.

### What there is not

No gradient fills, no boolean operations between paths, no text
along a path, and no reading an SVG into a path — an SVG is an
[image](images.md).
