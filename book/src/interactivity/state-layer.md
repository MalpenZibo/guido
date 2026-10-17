# State Layer API

The state layer system provides declarative style overrides based on widget interaction state.

## Overview

State layers let containers define how they should look when:
- **Hovered** - Mouse cursor is over the widget
- **Pressed** - Mouse button is held down on the widget
- **Focused** - The keyboard focus is inside the widget
- **Disabled** - The widget, or something above it, was declared `enabled(false)` — see [Disabling a Subtree](disabled.md)

Changes are defined declaratively, and the framework handles state transitions, animations, and rendering.

## Basic Usage

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corners(8.0)
    .when_hovered(|s| s.lighter(0.1))      // Style when hovered
    .when_pressed(|s| s.ripple())         // Style when pressed
    .child(text("Click me"))
# ;
# }
```

## Available Overrides

State layers can override these properties:

### Background

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
// Explicit color
.when_hovered(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))

// Relative to base
.when_hovered(|s| s.lighter(0.1))   // 10% lighter
.when_pressed(|s| s.darker(0.1))  // 10% darker
# ;
# }
```

On a container with a gradient, `lighter`, `darker` and `background_alpha` apply to both
of its ends, and an explicit `background` replaces the gradient while the state
is active.

`background_alpha` sets the alpha of the background and of nothing else: what
the container draws inside it keeps its colours. To fade the whole control, use
`opacity`.

### Opacity

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .opacity(1.0.transition(150.0))
    .when_disabled(|s| s.opacity(0.4))
    .child(text("Power off"))
# ;
# }
```

The container's own [`opacity`](../building-ui/styling.md#opacity), replaced
while the state holds: the background, the border and everything inside it fade
together, by the declared `opacity_mode`, and the transition declared on
`opacity` carries the change both ways.

### Border

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.when_hovered(|s| s.border(1.0, Color::WHITE))   // both halves, always
# ;
# }
```

A border takes room inside the box, so a layer that changes its width relays the
container out and moves its content by the difference; a colour change moves
nothing.

### Outline

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.outline(2.0, Color::TRANSPARENT, 2.0)
.when_focused(|s| s.outline(2.0, Color::WHITE, 2.0))   // width, colour, offset
# ;
# }
```

A ring outside the box that takes no room: a layer that shows, thickens or moves
it only repaints. It is the focus or hover ring.

### Transform

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.when_pressed(|s| s.scale(0.98))
# ;
# }
```

### Corner Radius

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.when_hovered(|s| s.corners(12.0))
# ;
# }
```

### Shadow

```rust
# extern crate guido;
# use guido::prelude::*;
# const LOW: Shadow = Shadow::simple((0.0, 1.0), 3.0, Color::rgba(0.0, 0.0, 0.0, 0.12));
# const HIGH: Shadow = Shadow::simple((0.0, 6.0), 10.0, Color::rgba(0.0, 0.0, 0.0, 0.22));
# fn main() {
# container()
.when_hovered(|s| s.shadow(HIGH))
.when_pressed(|s| s.shadow(LOW))
# ;
# }
```

### Ripple

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# container()
.when_pressed(|s| s.ripple())
.when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
# ;
# }
```

## Combining Overrides

Chain multiple overrides in a single state:

```rust
# extern crate guido;
# use guido::prelude::*;
# const RAISED: Shadow = Shadow::simple((0.0, 3.0), 6.0, Color::rgba(0.0, 0.0, 0.0, 0.19));
# fn main() {
# container()
.when_hovered(|s| s
    .lighter(0.1)
    .border(2.0, Color::WHITE)
    .shadow(RAISED)
)

.when_pressed(|s| s
    .ripple()
    .darker(0.05)
    .scale(0.98)
)
# ;
# }
```

## With Animations

The motion belongs to the property, declared once beside its base value. A
layer supplies a value for a property somebody else owns, and carries no timing
of its own — so a hover eases under whatever the base declared:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
    .when_hovered(|s| s.lighter(0.15))
    .when_pressed(|s| s.darker(0.1))
# ;
# }
```

Writing a timing on the override does not compile. It would be a declaration
that is legal and does nothing:

```rust,ignore
# // not compiled: the line is the mistake the paragraph above names.
.when_hovered(|s| s.background(HOT.transition(900.0)))   // error
```

## Complete Example

```rust
# extern crate guido;
# use guido::prelude::*;
fn interactive_button(label: &str) -> Container {
    container()
        .padding(16.0)
        // Each property carries its own motion
        .background(Color::rgb(0.3, 0.5, 0.8).transition(200.0))
        .corners(8.0)
        .border(1.0.transition(150.0), Color::rgb(0.4, 0.6, 0.9))
        .scale(Scale::NONE.transition(Transition::spring(SpringConfig::GENTLE)))

        // State layers
        .when_hovered(|s| s
            .lighter(0.1)
            .border(2.0, Color::rgb(0.5, 0.7, 1.0))
        )
        .when_pressed(|s| s
            .ripple()
            .darker(0.05)
            .scale(0.98)
        )

        .child(container().child(text(label).color(Color::WHITE)))
}
# fn main() {}
```

## How It Works

Internally, `StateStyle` holds all possible overrides:

```rust,ignore
# // not compiled: a field listing of the real struct, whose halves
# // (`BackgroundOverride`, `BorderOverride`, `OutlineOverride`) are named
# // without being built.
pub struct StateStyle {
    pub background: Option<BackgroundOverride>,
    // Both halves or neither: half a border is no border.
    pub border: Option<BorderOverride>,
    // All three halves or none, for the same reason.
    pub outline: Option<OutlineOverride>,
    pub corners: Prop<Corners>,
    pub translate: Prop<Translate>,
    pub rotate: Prop<f32>,
    pub scale: Prop<Scale>,
    pub shadow: Option<Box<Prop<Shadows>>>,
    pub background_alpha: Prop<f32>,
    pub opacity: Prop<f32>,
    pub ripple: Option<RippleConfig>,
}
```

`RippleConfig` is in the prelude, because `ripple_config` takes one.
`BackgroundOverride`, `BorderOverride` and `OutlineOverride` are not: nothing
asks you to build any of them — `background`, `lighter`, `darker`, `border` and
`outline` take colours and numbers and build them for you — so they stay in
`guido::widgets`, for the rare caller that reads a `StateStyle` back.

When the container paints, it checks the current state and applies overrides accordingly, blending with animations when configured.
