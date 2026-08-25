# Transform Basics

Learn the fundamental transform operations: translate, rotate, and scale.

## Translation

Move a widget by offset values:

```rust
container()
    .translate((20.0, 10.0))  // Move 20px right, 10px down
```

Negative values move in the opposite direction:

```rust
container()
    .translate((-10.0, 0.0))  // Move 10px left
```

## Rotation

Rotate a widget around its center (default):

```rust
container()
    .rotate(45.0)  // Rotate 45 degrees clockwise
```

Degrees, always, and the number is kept as written. A full turn is `360.0` and
not `0.0`, two turns are `720.0`, and nothing is normalised behind your back:

```rust
container().rotate(360.0)   // one full turn — animates as one
container().rotate(-90.0)   // a quarter turn anticlockwise
```

That matters as soon as the angle is animated. See
[animated transforms](animated.md).

## Scale

### Uniform Scale

Scale equally in both dimensions:

```rust
container().scale(1.5)   // 150% size
container().scale(0.8)   // 80% size
```

### Non-Uniform Scale

Scale differently on each axis:

```rust
container().scale((2.0, 0.5))  // 200% width, 50% height
```

## Combining Them

Declare as many as you need on the same container. They apply in one fixed
order — **translate, then rotate, then scale** — whatever order you write them
in:

```rust
container()
    .rotate(30.0)
    .translate((50.0, 0.0))
    .scale(1.2)
```

The order is fixed rather than taken from the call site so that two containers
that declare the same three values are the same shape. It is the order CSS uses
for its `translate` / `rotate` / `scale` properties, for the same reason.

Rotation and scale both happen about the [pivot](origins.md), which is the
centre of the container unless you say otherwise — so rotating about a corner is
`.pivot(Pivot::TOP_LEFT)`, not a translate-rotate-translate sandwich.

## Reactive Transforms

Transforms can use signals for dynamic updates:

```rust
let rotation = create_signal(0.0f32);

container()
    .rotate(rotation)
    .on_click(move || rotation.update(|r| *r += 45.0))
```

When the signal changes, the container repaints. Nothing re-runs layout: all
three are paint-only, so the space the container was given does not move and
neither does anything around it.

## Complete Example

```rust
fn transform_demo() -> impl Widget {
    let rotation = create_signal(0.0f32);
    let scale_factor = create_signal(1.0f32);

    container()
        .layout(Flex::row().spacing(20.0))
        .children([
            // Static rotation
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.8, 0.3, 0.3))
                .corners(8.0)
                .rotate(45.0)
                .child(container().child(text("45°").color(Color::WHITE))),

            // Click to rotate
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.6, 0.8))
                .corners(8.0)
                .rotate(rotation)
                .when_hovered(|s| s.lighter(0.1))
                .on_click(move || rotation.update(|r| *r += 45.0))
                .child(container().child(text("Click").color(Color::WHITE))),

            // Click to scale
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.8, 0.4))
                .corners(8.0)
                .scale(move || scale_factor.get())
                .when_hovered(|s| s.lighter(0.1))
                .on_click(move || {
                    let new = if scale_factor.get() > 1.0 { 1.0 } else { 1.3 };
                    scale_factor.set(new);
                })
                .child(container().child(text("Scale").color(Color::WHITE))),
        ])
}
```

## API Reference

### Container Methods

All transform properties accept static values, signals, or closures. Integers also work (e.g., `.rotate(45)`, `.scale(2)`).

```rust
impl Container {
    pub fn translate<M>(self, t: impl IntoSignal<Translate, M>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoSignal<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoSignal<Scale, M>) -> Self;
    pub fn pivot<M>(self, pivot: impl IntoSignal<Pivot, M>) -> Self;
}
```

### The Value Types

A pair builds either one; a bare number builds a uniform `Scale`.

```rust
Translate::NONE                  // no displacement
Translate::new(20.0, 10.0)       // or just (20.0, 10.0)

Scale::NONE                      // unscaled — 1.0 on both axes
Scale::uniform(1.5)              // or just 1.5
Scale::new(2.0, 0.5)             // or just (2.0, 0.5)
```

There is no matrix in the application-facing API. The three components are what
an application declares, and a matrix is what they compose into on the way to
the renderer — a widget written outside the crate reaches it through
`guido::widget_prelude`.
