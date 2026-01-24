# Transform Origins

By default, rotation and scale occur around the widget's center. Transform origins let you change this pivot point.

## Setting Transform Origin

```rust
container()
    .rotate(45.0)
    .transform_origin(TransformOrigin::TOP_LEFT)
```

Now the container rotates around its top-left corner instead of its center.

## Built-in Origins

| Origin | Position |
|--------|----------|
| `CENTER` | 50%, 50% (default) |
| `TOP_LEFT` | 0%, 0% |
| `TOP_RIGHT` | 100%, 0% |
| `BOTTOM_LEFT` | 0%, 100% |
| `BOTTOM_RIGHT` | 100%, 100% |
| `TOP` | 50%, 0% |
| `BOTTOM` | 50%, 100% |
| `LEFT` | 0%, 50% |
| `RIGHT` | 100%, 50% |

## Visual Examples

### Rotation from Different Origins

```
CENTER (default):        TOP_LEFT:
    ┌───┐                ┌───┐
    │ ↻ │                ↻
    └───┘

BOTTOM_RIGHT:
                         ┌───┐
                         │   │↻
                         └───┘
```

## Examples

### Rotate from Top-Left

```rust
container()
    .width(80.0)
    .height(80.0)
    .background(Color::rgb(0.3, 0.5, 0.8))
    .rotate(30.0)
    .transform_origin(TransformOrigin::TOP_LEFT)
```

### Scale from Bottom-Right

```rust
container()
    .scale(1.5)
    .transform_origin(TransformOrigin::BOTTOM_RIGHT)
```

### Pivot from Top Edge

```rust
container()
    .rotate(15.0)
    .transform_origin(TransformOrigin::TOP)
```

## Custom Origin

Specify exact percentages:

```rust
// 25% from left, 75% from top
TransformOrigin::custom(0.25, 0.75)
```

Values are percentages of the widget's size:
- `0.0` = left/top edge
- `0.5` = center
- `1.0` = right/bottom edge

## Reactive Origins

Transform origins can be reactive:

```rust
let origin = create_signal(TransformOrigin::CENTER);

container()
    .rotate(45.0)
    .transform_origin(origin)
    .on_click(move || {
        // Cycle through origins
        let next = match origin.get() {
            TransformOrigin::CENTER => TransformOrigin::TOP_LEFT,
            TransformOrigin::TOP_LEFT => TransformOrigin::BOTTOM_RIGHT,
            _ => TransformOrigin::CENTER,
        };
        origin.set(next);
    })
```

## Complete Example

```rust
fn origin_demo() -> impl Widget {
    container()
        .layout(Flex::row().spacing(40.0))
        .children([
            // Rotate from center (default)
            create_rotating_box(TransformOrigin::CENTER, "Center"),

            // Rotate from top-left
            create_rotating_box(TransformOrigin::TOP_LEFT, "Top-Left"),

            // Rotate from bottom-right
            create_rotating_box(TransformOrigin::BOTTOM_RIGHT, "Bottom-Right"),
        ])
}

fn create_rotating_box(origin: TransformOrigin, label: &'static str) -> Container {
    let rotation = create_signal(0.0f32);

    container()
        .layout(Flex::column().spacing(8.0))
        .children([
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.5, 0.8))
                .corner_radius(8.0)
                .rotate(rotation)
                .transform_origin(origin)
                .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
                .hover_state(|s| s.lighter(0.1))
                .on_click(move || rotation.update(|r| *r += 45.0)),
            text(label).font_size(12.0).color(Color::WHITE),
        ])
}
```

## API Reference

```rust
impl Container {
    pub fn transform_origin(
        self,
        origin: impl IntoMaybeDyn<TransformOrigin>
    ) -> Self;
}

impl TransformOrigin {
    pub const CENTER: TransformOrigin;
    pub const TOP_LEFT: TransformOrigin;
    pub const TOP_RIGHT: TransformOrigin;
    pub const BOTTOM_LEFT: TransformOrigin;
    pub const BOTTOM_RIGHT: TransformOrigin;
    pub const TOP: TransformOrigin;
    pub const BOTTOM: TransformOrigin;
    pub const LEFT: TransformOrigin;
    pub const RIGHT: TransformOrigin;

    pub fn custom(x: f32, y: f32) -> TransformOrigin;
}
```
