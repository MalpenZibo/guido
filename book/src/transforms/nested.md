# Nested Transforms

Transforms compose through the widget hierarchy. A child inherits and builds upon its parent's transform.

## How Nesting Works

When a parent has a transform, children are affected:

```rust
container()
    .rotate(20.0)  // Parent rotated
    .child(
        container()
            .scale(0.8)  // Child scaled within rotated parent
            .child(text("Nested"))
    )
```

The child appears both rotated (from parent) and scaled (its own).

## Transform Order

Parent transforms apply first, then child transforms:

```
World Space
    ↓
Parent Transform (rotate 20°)
    ↓
Child Space (already rotated)
    ↓
Child Transform (scale 0.8)
    ↓
Final Position
```

## Example: Rotated Cards

```rust
container()
    .rotate(15.0)  // Tilt the whole group
    .layout(Flex::row().spacing(10.0))
    .children([
        // Each card inherits the rotation
        card("One"),
        card("Two"),
        card("Three"),
    ])
```

All cards appear tilted by 15°.

## Example: Scaled Child with Rotation

```rust
container()
    .width(100.0)
    .height(100.0)
    .background(Color::rgb(0.3, 0.3, 0.4))
    .rotate(30.0)
    .child(
        container()
            .width(50.0)
            .height(50.0)
            .background(Color::rgb(0.5, 0.7, 0.9))
            .scale(1.2)  // Child is 20% larger within rotated parent
    )
```

## Hit Testing with Nested Transforms

Guido properly handles hit testing through nested transforms. A click on a nested, transformed element correctly detects the widget.

```rust
container()
    .rotate(45.0)
    .child(
        container()
            .scale(0.8)
            .on_click(|| println!("Clicked!"))  // Works correctly
    )
```

## Transform Independence

Each container has its own transform that doesn't affect siblings:

```rust
container()
    .layout(Flex::row().spacing(20.0))
    .children([
        // Each has independent transform
        container().rotate(15.0).child(...),
        container().rotate(-15.0).child(...),
        container().scale(0.9).child(...),
    ])
```

## Complete Example

```rust
fn nested_transforms_demo() -> impl Widget {
    container()
        .padding(40.0)
        .child(
            // Outer container with rotation
            container()
                .width(200.0)
                .height(200.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .corner_radius(16.0)
                .rotate(10.0)
                .layout(Flex::column().spacing(16.0).main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
                .children([
                    text("Parent (rotated 10°)").color(Color::WHITE).font_size(12.0),

                    // Inner container with scale
                    container()
                        .width(120.0)
                        .height(80.0)
                        .background(Color::rgb(0.3, 0.5, 0.8))
                        .corner_radius(8.0)
                        .scale(0.9)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .layout(Flex::column().main_alignment(MainAlignment::Center).cross_alignment(CrossAlignment::Center))
                        .child(
                            text("Child (scaled 0.9)")
                                .color(Color::WHITE)
                                .font_size(10.0)
                        ),
                ])
        )
}
```

## Caveats

### Transform Origin Applies Locally

A child's transform origin is relative to its own bounds, not the parent's:

```rust
container()
    .rotate(45.0)  // Rotates around its own center
    .child(
        container()
            .rotate(30.0)
            .transform_origin(TransformOrigin::TOP_LEFT)  // Relative to child's top-left
    )
```

### Performance

Deep nesting with many transforms is fine for typical UIs. The transform matrices multiply efficiently.

## Tips

1. **Keep it simple** - Deep transform nesting can be hard to reason about
2. **Use for grouping** - Apply a transform to a parent to affect all children
3. **Independent animations** - Each level can have its own animated transform

```rust
// Group animation
container()
    .rotate(group_rotation)
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .children([
        // Individual children can have their own animations
        container()
            .scale(child_scale)
            .animate_transform(Transition::spring(SpringConfig::BOUNCY))
            .child(...),
    ])
```
