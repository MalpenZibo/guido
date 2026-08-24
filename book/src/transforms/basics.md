# Transform Basics

Learn the fundamental transform operations: translate, rotate, and scale.

## Translation

Move a widget by offset values:

```rust
container()
    .transform(Transform::translate(20.0, 10.0))  // Move 20px right, 10px down
```

Negative values move in the opposite direction:

```rust
container()
    .transform(Transform::translate(-10.0, 0.0))  // Move 10px left
```

## Rotation

Rotate a widget around its center (default):

```rust
container()
    .transform(Transform::rotate_degrees(45.0))  // Rotate 45 degrees clockwise
```

Rotation uses degrees by default. For radians:

```rust
use std::f32::consts::PI;
Transform::rotate(PI / 4.0)  // 45 degrees in radians
```

## Scale

### Uniform Scale

Scale equally in both dimensions:

```rust
container().transform(Transform::scale(1.5))   // 150% size
container().transform(Transform::scale(0.8))   // 80% size
```

### Non-Uniform Scale

Scale differently on each axis:

```rust
container().transform(Transform::scale_xy(2.0, 0.5))  // 200% width, 50% height
```

## Using the Transform Type

For more control, use `Transform` directly:

```rust
container().transform(Transform::rotate_degrees(30.0))
container().transform(Transform::translate(10.0, 20.0))
container().transform(Transform::scale(1.2))
```

## Transform Composition

Combine multiple transforms using `.then()`:

```rust
// Rotate then translate
let t = Transform::rotate_degrees(30.0)
    .then(&Transform::translate(50.0, 0.0));

container().transform(t)
```

**Order matters**: `a.then(&b)` applies `b` first, then `a`.

### Example: Rotate Around Point

To rotate around a specific point, translate, rotate, then translate back:

```rust
// Rotate 45° around point (100, 100)
let pivot = Transform::translate(100.0, 100.0);
let rotate = Transform::rotate_degrees(45.0);
let un_pivot = Transform::translate(-100.0, -100.0);

let t = pivot.then(&rotate).then(&un_pivot);
```

(Or use [transform origins](origins.md) for easier pivot control.)

## Reactive Transforms

Transforms can use signals for dynamic updates:

```rust
let rotation = create_signal(0.0f32);

container()
    .transform(Transform::rotate_degrees(rotation))
    .on_click(move || rotation.update(|r| *r += 45.0))
```

When the signal changes, the transform updates automatically.

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
                .transform(Transform::rotate_degrees(45.0))
                .child(container().child(text("45°").color(Color::WHITE))),

            // Click to rotate
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.6, 0.8))
                .corners(8.0)
                .transform(Transform::rotate_degrees(rotation))
                .when_hovered(|s| s.lighter(0.1))
                .on_click(move || rotation.update(|r| *r += 45.0))
                .child(container().child(text("Click").color(Color::WHITE))),

            // Click to scale
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.8, 0.4))
                .corners(8.0)
                .transform(Transform::scale(scale_factor))
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
    pub fn translate<M1, M2>(self, x: impl IntoSignal<f32, M1>, y: impl IntoSignal<f32, M2>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoSignal<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoSignal<f32, M>) -> Self;
    pub fn scale_xy<M1, M2>(self, sx: impl IntoSignal<f32, M1>, sy: impl IntoSignal<f32, M2>) -> Self;
    pub fn transform<M>(self, transform: impl IntoSignal<Transform, M>) -> Self;
}
```

### Transform Type

```rust
impl Transform {
    pub const IDENTITY: Self;
    pub fn translate(x: f32, y: f32) -> Self;
    pub fn rotate(angle_radians: f32) -> Self;
    pub fn rotate_degrees(angle_degrees: f32) -> Self;
    pub fn scale(s: f32) -> Self;
    pub fn scale_xy(sx: f32, sy: f32) -> Self;
    pub fn then(&self, other: &Transform) -> Transform;
}
```
