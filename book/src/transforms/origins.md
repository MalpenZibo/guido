# Pivots

By default, rotation and scale happen about the widget's centre. A pivot moves
that point.

One pivot serves both `rotate` and `scale`, as CSS gives one `transform-origin`
to the whole group. `translate` ignores it: moving a box is the same movement
wherever the pivot sits.

## Setting a Pivot

```rust
container()
    .rotate(45.0)
    .pivot(Pivot::TOP_LEFT)
```

Now the container rotates around its top-left corner instead of its center.

## Built-in Pivots

| Pivot | Position |
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
    .pivot(Pivot::TOP_LEFT)
```

### Scale from Bottom-Right

```rust
container()
    .scale(1.5)
    .pivot(Pivot::BOTTOM_RIGHT)
```

### Pivot from Top Edge

```rust
container()
    .rotate(15.0)
    .pivot(Pivot::TOP)
```

## Custom Origin

Specify exact percentages:

```rust
// 25% from left, 75% from top
Pivot::percent(25.0, 75.0)
```

Values are percentages of the widget's size:
- `0.0` = left/top edge
- `0.5` = center
- `1.0` = right/bottom edge

## Reactive Origins

Pivots can be reactive:

```rust
let origin = create_signal(Pivot::CENTER);

container()
    .rotate(45.0)
    .pivot(origin)
    .on_click(move || {
        // Cycle through origins
        let next = match origin.get() {
            Pivot::CENTER => Pivot::TOP_LEFT,
            Pivot::TOP_LEFT => Pivot::BOTTOM_RIGHT,
            _ => Pivot::CENTER,
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
            create_rotating_box(Pivot::CENTER, "Center"),

            // Rotate from top-left
            create_rotating_box(Pivot::TOP_LEFT, "Top-Left"),

            // Rotate from bottom-right
            create_rotating_box(Pivot::BOTTOM_RIGHT, "Bottom-Right"),
        ])
}

fn create_rotating_box(origin: Pivot, label: &'static str) -> Container {
    let rotation = create_signal(0.0f32);

    container()
        .layout(Flex::column().spacing(8.0))
        .children([
            container()
                .width(60.0)
                .height(60.0)
                .background(Color::rgb(0.3, 0.5, 0.8))
                .corners(8.0)
                .rotate(rotation)
                .pivot(origin)
                .animate_rotate(Transition::new(300.0, TimingFunction::EaseOut))
                .when_hovered(|s| s.lighter(0.1))
                .on_click(move || rotation.update(|r| *r += 45.0)),
            container().child(text(label).font_size(12.0).color(Color::WHITE)),
        ])
}
```

## API Reference

```rust
impl Container {
    pub fn pivot(
        self,
        origin: impl IntoSignal<Pivot, M>
    ) -> Self;
}

impl Pivot {
    pub const CENTER: Pivot;
    pub const TOP_LEFT: Pivot;
    pub const TOP_RIGHT: Pivot;
    pub const BOTTOM_LEFT: Pivot;
    pub const BOTTOM_RIGHT: Pivot;
    pub const TOP: Pivot;
    pub const BOTTOM: Pivot;
    pub const LEFT: Pivot;
    pub const RIGHT: Pivot;

    pub fn percent(x_percent: f32, y_percent: f32) -> Pivot;  // 0-100
    pub fn px(x: f32, y: f32) -> Pivot;
}
```
