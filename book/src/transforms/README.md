# Transforms

Guido provides a complete 2D transform system for translating, rotating, and scaling widgets.

![Transform Example](../images/transform_example.png)

## Transform Types

- **Translate** - Move widgets by offset values
- **Rotate** - Spin widgets around a pivot point
- **Scale** - Resize widgets uniformly or non-uniformly

## Quick Example

```rust
container()
    .translate(20.0, 10.0)  // Move 20px right, 10px down
    .rotate(45.0)           // Rotate 45 degrees
    .scale(1.5)             // Scale to 150%
```

## In This Section

- [Transform Basics](basics.md) - Translate, rotate, and scale
- [Transform Origins](origins.md) - Control pivot points
- [Animated Transforms](animated.md) - Smooth transform animations
- [Nested Transforms](nested.md) - Parent-child transform composition

## Key Features

- **Reactive** - Transforms can use signals for dynamic updates
- **Animated** - Smooth transitions with spring or duration-based animations
- **Hit Testing** - Clicks correctly detect transformed bounds
- **Composable** - Combine multiple transforms with proper ordering
