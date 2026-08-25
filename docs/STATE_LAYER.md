# State Layer API

The state layer system provides declarative style overrides based on widget interaction state (hover, pressed, focused). This enables rich visual feedback without manual signal management.

## Overview

State layers allow containers to define how they should look when:
- **Hovered**: Mouse cursor is over the widget
- **Pressed**: Mouse button is held down on the widget
- **Focused**: Any child widget has keyboard focus (e.g., text input)
- **A condition the app owns**: `state(condition, |s| ...)` — the last submit failed, this row is selected

Style changes are defined declaratively using builder methods, and the framework handles all state transitions, animations, and rendering automatically.

Every value inside a state layer is a signal, exactly like the base property it covers, so a state override follows a theme instead of taking a snapshot of it.

## Basic Usage

```rust
use guido::prelude::*;

container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .corners(8.0)
    .when_hovered(|s| s.lighter(0.1))      // Lighten on hover
    .when_pressed(|s| s.ripple())         // Ripple on press
    .child(text("Click me"))
```

### Focused State for Input Containers

The `when_focused` is applied when any child widget has keyboard focus. This is particularly useful for styling input containers:

```rust
container()
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corners(6.0)
    .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))  // Highlight when focused
    .child(text_input(value))
```

### App-Declared States

The first three states are noticed by the container. The fourth is a condition the app already holds, so it needs no propagation — the container just reads the signal:

```rust
let wrong_password = create_signal(false);

container()
    .border(1.0, theme.line)
    .state(wrong_password, |s| s.border(2.0, theme.error))
    .child(text_input(password))
```

The same signal can be read anywhere else that needs it, independently. That is why there is one generic `state` rather than a named method per case.

## Interaction Units

When a text declares a hover style, hover *of what*? Not its own glyphs — a
button's label has to light up while the pointer is on the button's padding.
Not any ancestor either — a label inside one button must stay dark while the
pointer is over a sibling button in the same row.

The unit that holds the state is the nearest enclosing **control**:

```rust
container().control()
    .child(text("Password").when_focused(|s| s.color(theme.accent)))
    .child(text_input(password))
```

Everything inside asks the same question — *is my control in this state?* —
whatever the state is. Direction stops being part of the mechanism and becomes
only how the control notices: the pointer is inside its bounds, the focus is
inside its subtree. Which is what makes the case above work at all — the focus
is in a *sibling*, and both belong to the same unit.

`control()` is rarely written by hand. Anything the pointer can act on is a
unit by necessity, so `on_click`, `on_hover`, `on_scroll`, `scrollable` and a
declared state layer all imply it. Write it where the boundary is real but
nothing else announces it — a field's label and input, a row whose highlight
belongs to the row.

Leaves declare states with `when_hovered`, `when_pressed`, `when_focused` and
`state`, from the `Stateful` trait. The closure receives the same partial style
the widget itself is built with, because an override *is* another partial
style:

```rust
text("Save").color(theme.weak).when_hovered(|s| s.color(theme.strong))
```

### Nesting scopes resolution, not state

Every control notices its own state independently: a list row is hovered
because the pointer is inside the row, even when the pointer is over a button
nested in it. What the nested control changes is only *who a descendant asks* —
the label inside the button asks the button, not the row.

### With no control above

The widget is its own unit and notices the pointer over its own bounds. It is
never *pressed*, though: being pressed means being activated, and a widget that
is its own unit has nothing to activate.

## Precedence: last declared wins

Layers are resolved in reverse declaration order, per property. Writing the error after the focus is what makes an error outrank a focus ring on a field that holds the focus essentially always:

```rust
container()
    .border(1.0, theme.line)
    .when_focused(|s| s.border(2.0, theme.accent))
    .state(wrong_password, |s| s.border(2.0, theme.error))  // wins while it holds
```

A layer that says nothing about a property is passed over rather than ending the search, so a pressed layer that only scales still lets the hover's background through.

Note that a layer *replaces* the base value rather than ranking against it. A property that already carries a meaning of its own belongs in a layer with a condition, not in the base.

## State Style Methods

### Background Color

```rust
// Explicit color
.when_hovered(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))

// Relative to base color
.when_hovered(|s| s.lighter(0.1))   // 10% lighter
.when_pressed(|s| s.darker(0.1))  // 10% darker
```

### Border

```rust
// Change border on hover
.border(1.0, Color::rgb(0.3, 0.3, 0.4))
.when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
.when_pressed(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))
.when_hovered(|s| s.border(2.0, Color::WHITE))   // both halves, always
```

### Transform

```rust
// Scale down on press for tactile feedback
.when_pressed(|s| s.scale(0.98))

// Combine with other effects
.when_pressed(|s| s.darker(0.1).scale(0.98))
```

### Corner Radius

```rust
.corners(8.0)
.when_hovered(|s| s.corners(12.0))
```

### Elevation (Shadow)

```rust
.elevation(2.0)
.when_hovered(|s| s.elevation(4.0))
.when_pressed(|s| s.elevation(1.0))
```

## Ripple Effects

Ripple effects provide Material Design-style touch feedback. The disc appears at
the click point already at about a third of its final size and spreads from
there; a release **completes** the expansion while fading the disc out, and the
pointer leaving without a release fades it quickly without completing anything.

The radius never goes backwards, a short press does not truncate the growth, and
the disc always finishes appearing before it starts to leave — those three are
the whole feel of the effect. Its final size comes from the container rather
than the contact point, so every ripple on a button is the same size. Each press
is its own ripple and they overlap; up to four are alive at a time.

### Default Ripple

```rust
.when_pressed(|s| s.ripple())
```

### Colored Ripple

```rust
.when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
```

### Ripple with Other Effects

```rust
.when_pressed(|s| s.ripple().scale(0.98))
```

## Animations

State transitions can be animated using the `animate_*` methods:

```rust
container()
    .background(Color::rgb(0.3, 0.6, 0.4))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.lighter(0.15))
    .when_pressed(|s| s.darker(0.1))
```

Available animation methods:
- `animate_background(Transition)` - Animate background color changes
- `animate_border_width(Transition)` - Animate border width changes
- `animate_border_color(Transition)` - Animate border color changes
- `animate_translate(Transition)` / `animate_rotate(..)` / `animate_scale(..)`

### Transition Types

```rust
// Duration-based with timing function
Transition::new(200.0, TimingFunction::EaseOut)
Transition::new(150.0, TimingFunction::EaseInOut)

// Spring-based for physics-driven animation
Transition::spring(SpringConfig::BOUNCY)
Transition::spring(SpringConfig::SMOOTH)
```

## Complete Example

```rust
fn create_button(label: &str) -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corners(8.0)
        .border(1.0, Color::rgb(0.4, 0.6, 0.9))
        
        // The label follows the state too, not just the box
        .control()   // the text below declares .when_hovered(|s| s.color(Color::rgb(0.95, 0.98, 1.0)))
        // Animations
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        // State overrides
        .when_hovered(|s| s.lighter(0.1).border(2.0, Color::rgb(0.5, 0.7, 1.0)))
        .when_pressed(|s| s.ripple().darker(0.05).scale(0.98))
        .child(text(label).color(Color::WHITE))
}
```

## Implementation Notes

### StateStyle Struct

The `StateStyle` struct holds all possible overrides:

```rust
pub struct StateStyle {
    pub background: Option<BackgroundOverride>,
    /// Both halves or neither — half a border is no border.
    pub border: Option<BorderOverride>,
    pub corner_radius: Option<Signal<f32>>,
    pub transform: Option<Signal<Transform>>,
    pub elevation: Option<Signal<f32>>,
    pub text_color: Option<Signal<Color>>,
    pub alpha: Option<Signal<f32>>,
    pub ripple: Option<RippleConfig>,
}
```

Each layer is stored with the trigger that turns it on, in declaration order:

```rust
pub enum StateWhen {
    Hovered,
    Pressed,
    Focused,
    When(Signal<bool>),
}
```

A trigger is read only where a layer uses it, so a container carrying a single `state(..)` subscribes to neither the pointer flags nor the focus path.

### BackgroundOverride Enum

Background can be set absolutely or relatively:

```rust
pub enum BackgroundOverride {
    Exact(Signal<Color>),   // Use this exact color
    Lighter(Signal<f32>),   // Blend toward white by amount (0.0-1.0)
    Darker(Signal<f32>),    // Blend toward black by amount (0.0-1.0)
}
```

### Ripple Rendering

Ripples are rendered in the overlay layer (on top of text and other content). They:
- Expand from the click point to fill the container bounds
- Respect corner radius and container clipping
- Contract toward the release point when the mouse is released
- Work correctly with transformed containers (rotated, scaled, translated)


Benefits of the new API:
- Less boilerplate code
- No manual signal management
- Built-in animation support
- Ripple effects included
- Better separation of concerns
