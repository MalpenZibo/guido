# Transforms

Guido provides a complete 2D transform system for translating, rotating, and scaling widgets.

![Transform Example](../images/transform_example.png)

## Transform Types

- **Translate** - Move widgets by offset values
- **Rotate** - Spin widgets around a pivot point
- **Scale** - Resize widgets uniformly or non-uniformly

## Quick Example

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn main() {
// All three at once — they compose, in the order
// translate, then rotate, then scale.
container()
    .translate((20.0, 10.0))
    .rotate(45.0);
    .scale(1.5);

// Or one on its own.
container().translate((20.0, 10.0));  // move 20px right, 10px down
container().rotate(45.0)             // turn 45 degrees clockwise
container().scale(1.5)               // 150% size
# ;
# }
```

## In This Section

- [Transform Basics](basics.md) - Translate, rotate, and scale
- [Pivots](origins.md) - Control the point rotation and scale act about
- [Animated Transforms](animated.md) - Smooth transform animations
- [Nested Transforms](nested.md) - Parent-child transform composition

## Key Features

- **Reactive** - Transforms can use signals for dynamic updates
- **Animated** - Smooth transitions with spring or duration-based animations
- **Hit Testing** - Clicks correctly detect transformed bounds
- **Composable** - Combine multiple transforms with proper ordering
