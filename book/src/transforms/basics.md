# Transform Basics

Learn the fundamental transform operations: translate, rotate, and scale.

## Translation

Move a widget by offset values:

```rust
container()
    .translate(20.0, 10.0)  // Move 20px right, 10px down
```

Negative values move in the opposite direction:

```rust
container()
    .translate(-10.0, 0.0)  // Move 10px left
```

## Rotation

Rotate a widget around its center (default):

```rust
container()
    .rotate(45.0)  // Rotate 45 degrees clockwise
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
container().scale(1.5)   // 150% size
container().scale(0.8)   // 80% size
```

### Non-Uniform Scale

Scale differently on each axis:

```rust
container().scale_xy(2.0, 0.5)  // 200% width, 50% height
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
    .rotate(rotation)
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
                .corner_radius(8.0)
                .rotate(45.0)
                .child(text("45°").color(Color::WHITE)),

            // Click to rotate
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.6, 0.8))
                .corner_radius(8.0)
                .rotate(rotation)
                .hover_state(|s| s.lighter(0.1))
                .on_click(move || rotation.update(|r| *r += 45.0))
                .child(text("Click").color(Color::WHITE)),

            // Click to scale
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.8, 0.4))
                .corner_radius(8.0)
                .scale(scale_factor)
                .hover_state(|s| s.lighter(0.1))
                .on_click(move || {
                    let new = if scale_factor.get() > 1.0 { 1.0 } else { 1.3 };
                    scale_factor.set(new);
                })
                .child(text("Scale").color(Color::WHITE)),
        ])
}
```

## API Reference

### Container Methods

```rust
impl Container {
    pub fn translate(self, x: f32, y: f32) -> Self;
    pub fn rotate(self, degrees: impl IntoMaybeDyn<f32>) -> Self;
    pub fn scale(self, factor: impl IntoMaybeDyn<f32>) -> Self;
    pub fn scale_xy(self, sx: f32, sy: f32) -> Self;
    pub fn transform(self, transform: Transform) -> Self;
}
```

### Transform Type

```rust
impl Transform {
    pub fn identity() -> Self;
    pub fn translate(x: f32, y: f32) -> Self;
    pub fn rotate(angle_radians: f32) -> Self;
    pub fn rotate_degrees(angle_degrees: f32) -> Self;
    pub fn scale(s: f32) -> Self;
    pub fn scale_xy(sx: f32, sy: f32) -> Self;
    pub fn then(&self, other: &Transform) -> Transform;
}
```
