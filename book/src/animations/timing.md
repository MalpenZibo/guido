# Timing Functions

Timing functions (also called easing curves) control how animations progress over time.

## Available Functions

### Linear

Constant speed throughout:

```rust
TimingFunction::Linear
```

Use for: Progress indicators, mechanical motion

### EaseIn

Starts slow, accelerates:

```rust
TimingFunction::EaseIn
```

Use for: Elements leaving the screen

### EaseOut

Starts fast, decelerates:

```rust
TimingFunction::EaseOut
```

Use for: **Most UI animations** - feels responsive and natural

### EaseInOut

Slow start and end, fast middle:

```rust
TimingFunction::EaseInOut
```

Use for: On-screen transitions, modal appearances

## Visual Comparison

```
Linear:    ────────────────
EaseIn:    ___──────────
EaseOut:   ──────────___
EaseInOut: ___────────___
```

## Recommendations

### For State Changes (Hover, Press)

Use `EaseOut` - immediate response, smooth finish:

```rust
.animate_background(Transition::new(200.0, TimingFunction::EaseOut))
```

### For Expanding/Collapsing

Use `EaseInOut` - smooth start and stop:

```rust
.animate_width(Transition::new(300.0, TimingFunction::EaseInOut))
```

### For Enter Animations

Use `EaseOut` - quick appearance, smooth settle:

```rust
Transition::new(250.0, TimingFunction::EaseOut)
```

### For Exit Animations

Use `EaseIn` - quick exit, fades out:

```rust
Transition::new(200.0, TimingFunction::EaseIn)
```

## Examples

### Button Hover

```rust
container()
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .hover_state(|s| s.lighter(0.1))
```

### Card Expansion

```rust
let expanded = create_signal(false);

container()
    .width(move || if expanded.get() { 400.0 } else { 200.0 })
    .animate_width(Transition::new(300.0, TimingFunction::EaseInOut))
    .on_click(move || expanded.update(|e| *e = !*e))
```

### Smooth Transform

```rust
container()
    .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
    .pressed_state(|s| s.transform(Transform::scale(0.98)))
```

## When to Use Springs Instead

For physical motion (bouncing, overshooting), use spring animations:

```rust
// Spring for bouncy physical motion
.animate_transform(Transition::spring(SpringConfig::BOUNCY))

// Duration for smooth UI transitions
.animate_background(Transition::new(200.0, TimingFunction::EaseOut))
```

See [Spring Physics](springs.md) for more on spring animations.

## API Reference

```rust
pub enum TimingFunction {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
```
