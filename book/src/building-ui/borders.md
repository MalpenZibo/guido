# Borders & Corners

Guido renders crisp, anti-aliased borders using SDF (Signed Distance Field) techniques.

![Showcase](../images/showcase.png)

## Basic Border

```rust
container()
    .border(2.0, Color::WHITE)  // 2px white border
```

A border is always both halves at once — everywhere. A width with no colour and
a colour with no width are the same thing, which is no border, so there is
nothing for a half-declaration to mean, and no way to write one. Each half takes
a signal of its own, which covers anything that has to change:

```rust
container().border(1.5, move || if failed.get() { theme.danger } else { theme.line })
```

A state layer says it the same way, and replaces the whole border:

```rust
container()
    .border(1.5, theme.line)
    .when_focused(|s| s.border(1.5, theme.accent))
```

## Corners

How far the corners are rounded, and how — one property, because a curvature
with no radius has no arc to apply itself to.

A bare size means rounded corners, and takes what `padding` takes: one value
for all four, `[top, bottom]` for the two pairs, or
`[top-left, top-right, bottom-right, bottom-left]` clockwise as CSS writes it.

```rust
container().corners(8.0)              // all four
container().corners([16.0, 0.0])      // the top pair only
container().corners([16.0, 4.0, 16.0, 4.0])
```

The shape reaches everything: the box, its border and shadow, the blur behind
it, the clip its children are cut to, and the region that answers a click.

### The shape of the corner (Superellipse)

A constructor names the shape and takes the size. The curve is a CSS K-value —
how far from the corner the arc starts.

### Squircle (K=2)

iOS-style smooth corners. The curve starts further from the corner for a smoother transition.

```rust
container()
    .corners(Corners::squircle(12.0))
```

### Circle (K=1)

Standard circular corners. This is the default.

```rust
container()
    .corners(12.0)  // Default is circular
```

### Bevel (K=0)

Diagonal cut corners. Creates a chamfered look.

```rust
container()
    .corners(Corners::bevel(12.0))
```

### Scoop (K=-1)

Concave/inward corners. Creates a scooped appearance.

```rust
container()
    .corners(Corners::scoop(12.0))
```

### Custom Curvature

For values between the presets:

```rust
container()
    .corners(Corners::superellipse(12.0, 1.5))  // Between circle and squircle
```

## Curvature Reference

| Style | K Value | Description |
|-------|---------|-------------|
| Squircle | 2.0 | Smooth, iOS-style |
| Circle | 1.0 | Standard rounded (default) |
| Bevel | 0.0 | Diagonal/chamfered |
| Scoop | -1.0 | Concave inward |

## Animated Borders

Borders can animate on state changes:

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
    .when_pressed(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))
```

## Borders with Different Curvatures

Borders respect corner curvature:

```rust
container()
    .border(2.0, Color::rgb(0.5, 0.3, 0.7))
    .corners(Corners::squircle(12.0))  // Border follows squircle shape
```

## Borders with Gradients

Borders work with gradient backgrounds:

```rust
container()
    .gradient(LinearGradient::horizontal(Color::rgb(0.3, 0.1, 0.4), Color::rgb(0.1, 0.3, 0.5)))
    .corners(8.0)
    .border(2.0, Color::rgba(1.0, 1.0, 1.0, 0.3))  // Semi-transparent white
```

## Complete Example

```rust
fn card_with_border() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.12, 0.12, 0.16))
        .corners(Corners::squircle(12.0))
        .border(1.0, Color::rgb(0.2, 0.2, 0.25))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
        .when_hovered(|s| s
            .border(2.0, Color::rgb(0.4, 0.6, 0.9))
            .lighter(0.03)
        )
        .child(container().child(text("Hover to see border change").color(Color::WHITE)))
}
```
