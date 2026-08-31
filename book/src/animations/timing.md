# Timing Functions

Timing functions (also called easing curves) control how animations progress over time.

## Available Functions

### Linear

Constant speed throughout:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
TimingFunction::Linear
# ;
# }
```

Use for: Progress indicators, mechanical motion

### EaseIn

Starts slow, accelerates:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
TimingFunction::EaseIn
# ;
# }
```

Use for: Elements leaving the screen

### EaseOut

Starts fast, decelerates:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
TimingFunction::EaseOut
# ;
# }
```

Use for: **Most UI animations** - feels responsive and natural

### EaseInOut

Slow start and end, fast middle:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
TimingFunction::EaseInOut
# ;
# }
```

Use for: On-screen transitions, modal appearances

## Visual Comparison

```text
Linear:    ────────────────
EaseIn:    ___──────────
EaseOut:   ──────────___
EaseInOut: ___────────___
```

## Recommendations

### For State Changes (Hover, Press)

Use `EaseOut` - immediate response, smooth finish:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let c = Color::WHITE;
# container()
.background(c.transition(Transition::new(200.0, TimingFunction::EaseOut)))
# ;
# }
```

### For Expanding/Collapsing

Use `EaseInOut` - smooth start and stop:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.width(200.0.transition(Transition::new(300.0, TimingFunction::EaseInOut)))
# ;
# }
```

### For Enter Animations

Use `EaseOut` - quick appearance, smooth settle:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
Transition::new(250.0, TimingFunction::EaseOut)
# ;
# }
```

### For Exit Animations

Use `EaseIn` - quick exit, fades out:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
Transition::new(200.0, TimingFunction::EaseIn)
# ;
# }
```

## Examples

### Button Hover

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.3, 0.5, 0.8).transition(Transition::new(200.0, TimingFunction::EaseOut)))
    .when_hovered(|s| s.lighter(0.1))
# ;
# }
```

### Card Expansion

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let expanded = create_signal(false);

container()
    .width(
        (move || if expanded.get() { 400.0 } else { 200.0 })
            .transition(Transition::new(300.0, TimingFunction::EaseInOut)),
    )
    .on_click(move || expanded.update(|e| *e = !*e))
# ;
# }
```

### Smooth Transform

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .scale(Scale::NONE.transition(Transition::new(300.0, TimingFunction::EaseOut)))
    .when_pressed(|s| s.scale(0.98))
# ;
# }
```

## When to Use Springs Instead

For physical motion (bouncing, overshooting), use spring animations:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let c = Color::WHITE;
# container()
// Spring for bouncy physical motion
.scale(Scale::NONE.transition(Transition::spring(SpringConfig::BOUNCY)))

// Duration for smooth UI transitions
.background(c.transition(Transition::new(200.0, TimingFunction::EaseOut)))
# ;
# }
```

See [Spring Physics](springs.md) for more on spring animations.

## API Reference

```rust,ignore
pub enum TimingFunction {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
```
