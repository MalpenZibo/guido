# Transform System

Guido provides a complete 2D transform system for translating, rotating, and scaling widgets.

## Basic Transforms

### Translation

Move a widget by offset values:

```rust
container()
    .translate(20.0, 10.0)  // Move 20px right, 10px down
```

### Rotation

Rotate a widget around its center:

```rust
container()
    .rotate(45.0)  // Rotate 45 degrees clockwise
```

### Scale

Scale a widget uniformly or non-uniformly:

```rust
container().scale(1.5)           // 150% size
container().scale_xy(2.0, 0.5)   // 200% width, 50% height
```

## Transform Composition

Combine multiple transforms using `.then()`:

```rust
// Rotate then scale
Transform::rotate_degrees(30.0).then(&Transform::scale(0.8))

// Or use the .transform() method with composed transform
container()
    .transform(Transform::rotate_degrees(30.0).then(&Transform::scale(0.8)))
```

**Order matters**: `a.then(&b)` applies `b` first, then `a`.

## Transform Origin

By default, rotation and scale occur around the widget's center. Use transform origin to change the pivot point:

```rust
// Rotate around top-left corner
container()
    .rotate(45.0)
    .transform_origin(TransformOrigin::TOP_LEFT)

// Scale from bottom-right
container()
    .scale(0.8)
    .transform_origin(TransformOrigin::BOTTOM_RIGHT)
```

### Built-in Origins

```rust
TransformOrigin::CENTER        // 50%, 50% (default)
TransformOrigin::TOP_LEFT      // 0%, 0%
TransformOrigin::TOP_RIGHT     // 100%, 0%
TransformOrigin::BOTTOM_LEFT   // 0%, 100%
TransformOrigin::BOTTOM_RIGHT  // 100%, 100%
TransformOrigin::TOP           // 50%, 0%
TransformOrigin::BOTTOM        // 50%, 100%
TransformOrigin::LEFT          // 0%, 50%
TransformOrigin::RIGHT         // 100%, 50%
```

### Custom Origin

```rust
// 25% from left, 75% from top
TransformOrigin::custom(0.25, 0.75)
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
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .on_click(move || rotation.update(|r| *r += 45.0))
```

### Spring Animation

For physics-based animation:

```rust
container()
    .scale(scale_signal)
    .animate_transform(Transition::spring(SpringConfig::BOUNCY))
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
    .pressed_state(|s| s.transform(Transform::scale(0.98)))
```

## Hit Testing

Transforms are properly accounted for in hit testing. A rotated button will correctly detect clicks within its rotated bounds.

## Transform API Reference

### Transform Struct

```rust
impl Transform {
    // Creation
    pub fn identity() -> Self;
    pub fn translate(x: f32, y: f32) -> Self;
    pub fn rotate(angle_radians: f32) -> Self;
    pub fn rotate_degrees(angle_degrees: f32) -> Self;
    pub fn scale(s: f32) -> Self;
    pub fn scale_xy(sx: f32, sy: f32) -> Self;

    // Composition
    pub fn then(&self, other: &Transform) -> Transform;
    pub fn center_at(self, cx: f32, cy: f32) -> Self;

    // Utilities
    pub fn inverse(&self) -> Transform;
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32);
    pub fn is_identity(&self) -> bool;
    pub fn has_rotation(&self) -> bool;
    pub fn extract_scale(&self) -> f32;
}
```

### Container Transform Methods

```rust
impl Container {
    pub fn translate(self, x: f32, y: f32) -> Self;
    pub fn rotate(self, degrees: impl IntoMaybeDyn<f32>) -> Self;
    pub fn scale(self, factor: impl IntoMaybeDyn<f32>) -> Self;
    pub fn scale_xy(self, sx: f32, sy: f32) -> Self;
    pub fn transform(self, transform: Transform) -> Self;
    pub fn transform_origin(self, origin: impl IntoMaybeDyn<TransformOrigin>) -> Self;
    pub fn animate_transform(self, transition: Transition) -> Self;
}
```
