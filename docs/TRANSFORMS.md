# Transform System

Guido provides a complete 2D transform system for translating, rotating, and scaling widgets.

## Basic Transforms

### Translation

Move a widget by offset values:

```rust
container()
    .translate((20.0, 10.0))  // Move 20px right, 10px down
```

### Rotation

Rotate a widget about its pivot, in degrees, clockwise:

```rust
container()
    .rotate(45.0)  // Rotate 45 degrees clockwise
```

**The angle is not normalised.** `360.0` is a full turn and not zero, `720.0` is
two, and `370.0` is twenty degrees past a full turn rather than ten degrees from
the start. This is what makes a rotation animatable at all: an angle folded into
a matrix, or wrapped into `0..360`, has lost how far round it went, and
interpolating it interpolates something that no longer holds the answer. See
issue #212.

It also means nothing takes a shorter way round on a caller's behalf. An angle
that arrives already wrapped — from `atan2`, in a drag-to-rotate, where the
value jumps from 179 to -179 — wraps the animation with it, and unwrapping it
belongs to the caller, who is the only one who knows which way was meant. A
declared shortest-path policy, as QML's `RotationAnimation.direction` has, is
the shape that would answer it; there is no case for one yet.

### Scale

Scale a widget uniformly or non-uniformly:

```rust
container().scale(1.5)           // 150% size
container().scale((2.0, 0.5))   // 200% width, 50% height
```

## Combining Them

```rust
container().rotate(30.0).scale(0.8)
```

**The order is fixed, not taken from the call site**: translate, then rotate,
then scale, however they are written. Two containers declaring the same three
values are the same shape. Same order, and same reason, as CSS's individual
`translate` / `rotate` / `scale` properties.

## Transform Origin

By default, rotation and scale occur around the widget's center. Use transform origin to change the pivot point:

```rust
// Rotate around top-left corner
container()
    .rotate(45.0)
    .pivot(Pivot::TOP_LEFT)

// Scale from bottom-right
container()
    .scale(0.8)
    .pivot(Pivot::BOTTOM_RIGHT)
```

### Built-in Origins

```rust
Pivot::CENTER        // 50%, 50% (default)
Pivot::TOP_LEFT      // 0%, 0%
Pivot::TOP_RIGHT     // 100%, 0%
Pivot::BOTTOM_LEFT   // 0%, 100%
Pivot::BOTTOM_RIGHT  // 100%, 100%
Pivot::TOP           // 50%, 0%
Pivot::BOTTOM        // 50%, 100%
Pivot::LEFT          // 0%, 50%
Pivot::RIGHT         // 100%, 50%
```

### Custom Origin

```rust
// 25% from left, 75% from top
Pivot::percent(25.0, 75.0)
```

## Reactive Transforms

Transforms can be reactive using signals:

```rust
let rotation = create_signal(0.0f32);

container()
    .rotate(rotation)  // Updates when signal changes
    .on_click(move || rotation.update(|r| *r += 45.0))
```

## Animated Transforms

Animate transform changes with transitions:

```rust
let rotation = create_signal(0.0f32);

container()
    .rotate(rotation)
    .animate_rotate(Transition::new(300.0, TimingFunction::EaseOut))
    .on_click(move || rotation.update(|r| *r += 45.0))
```

### Spring Animation

For physics-based animation:

```rust
container()
    .scale(scale_signal)
    .animate_scale(Transition::spring(SpringConfig::BOUNCY))
```

## Nested Transforms

Transforms compose through the widget hierarchy. A child inherits its parent's transform:

```rust
container()
    .rotate(20.0)  // Parent rotated
    .child(
        container()
            .scale(0.8)  // Child scaled within rotated parent
            .child(text("Nested transforms"))
    )
```

## Transform in State Layers

Apply transforms on interaction:

```rust
container()
    .when_pressed(|s| s.scale(0.98))
```

## Hit Testing

Transforms are properly accounted for in hit testing. A rotated button will correctly detect clicks within its rotated bounds.

## Transform API Reference

### Transform Struct

Not in `guido::prelude`, and **`Container` does not accept one** — an
application declares the three components and nothing else. This is what they
compose into on the way to the renderer, and it is in `guido::widget_prelude`
for the other job: a widget written outside the crate positioning what it
paints, through `PaintContext::set_transform`.

So a shear, or a composition order other than translate → rotate → scale, is
not expressible on a container. Where the old `.transform()` built a rotated
offset with `rotate(5).then(&translate(10, 15))`, the equivalent now is a
nested container: the outer one turns, the inner one moves inside the turned
frame.

```rust
impl Transform {
    // Creation
    pub const IDENTITY: Self;
    pub fn translate(x: f32, y: f32) -> Self;
    pub fn rotate(angle_radians: f32) -> Self;
    pub fn rotate_degrees(angle_degrees: f32) -> Self;
    pub fn scale(s: f32) -> Self;
    pub fn scale_xy(sx: f32, sy: f32) -> Self;

    // Composition
    pub fn then(&self, other: &Transform) -> Transform;
    pub fn center_at(self, cx: f32, cy: f32) -> Self;

    // The matrix itself, `[a, b, tx, c, d, ty]`
    pub fn a(&self) -> f32;  // and b, c, d, tx, ty

    // Utilities
    pub fn inverse(&self) -> Transform;
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32);
    pub fn is_identity(&self) -> bool;
    pub fn is_translation_only(&self) -> bool;
}
```

### Container Transform Methods

```rust
impl Container {
    pub fn translate<M>(self, t: impl IntoSignal<Translate, M>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoSignal<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoSignal<Scale, M>) -> Self;
    pub fn pivot<M>(self, pivot: impl IntoSignal<Pivot, M>) -> Self;
    pub fn animate_translate(self, transition: impl Into<TransitionConfig>) -> Self;
    pub fn animate_rotate(self, transition: impl Into<TransitionConfig>) -> Self;
    pub fn animate_scale(self, transition: impl Into<TransitionConfig>) -> Self;
}
```
