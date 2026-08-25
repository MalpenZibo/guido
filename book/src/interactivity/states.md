# Hover & Pressed States

Define visual changes for different interaction states.

Every value inside a state layer is reactive, like the base property it covers.
`when_hovered(|s| s.background(theme.accent))` follows the theme; it does not
take a snapshot of it when the widget is built.

## Hover State

Applied when the mouse cursor is over the widget:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
```

### Common Hover Patterns

**Lighten background:**
```rust
.when_hovered(|s| s.lighter(0.1))
```

**Explicit color change:**
```rust
.when_hovered(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))
```

**Border highlight:**
```rust
.border(1.0, Color::rgb(0.3, 0.3, 0.4))
.when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
```

**Elevation lift:**
```rust
.elevation(2.0)
.when_hovered(|s| s.elevation(4.0))
```

## Pressed State

Applied when the mouse button is held down:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_pressed(|s| s.darker(0.1))
```

### Common Pressed Patterns

**Darken background:**
```rust
.when_pressed(|s| s.darker(0.1))
```

**Scale down (tactile feedback):**
```rust
.when_pressed(|s| s.scale(0.98))
```

**Reduce elevation (press into surface):**
```rust
.elevation(4.0)
.when_pressed(|s| s.elevation(1.0))
```

**Ripple effect:**
```rust
.when_pressed(|s| s.ripple())
```

## Combining Hover and Pressed

Most interactive elements use both states:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple().darker(0.05))
```

## Text Colour

A state layer can change the colour of the text below the container, not just
the box around it:

```rust
container()
    
    .control()   // the text below declares .when_hovered(|s| s.color(theme.text))
    .child(text("Label").color(theme.text_weak))
```

This works the same way the ordinary declaration does — the container publishes
its text colour to descendants, and here what it publishes folds in the hover.
A text that declares its own colour nearer down is unaffected, and a container
with no state layer mentioning text creates nothing extra.

The base it returns to when the state ends is the colour that would have been
inherited anyway, so a container can override on hover without restating what
its ancestors already said:

```rust
container()
                  // set once, further up
    .child(
        container()
            .control()   // the text below declares .when_hovered(|s| s.color(theme.text))
            .child(text("Label").color(theme.text_weak)),            // weak, then strong, then weak
    )
```

It can be animated like any other container property:

```rust
container()
    
    .control()   // the text below declares .when_hovered(|s| s.color(theme.text))
    .animate_text_color(Transition::new(200.0, TimingFunction::EaseOut))
    .child(text("Label").color(theme.text_weak))
```

A transition declared on two levels — an animated colour whose own base comes
from an ancestor that is itself animating — currently retargets every frame,
which gives a damped chase rather than a transition with its own curve. It
converges either way; CSS instead starts the inner one once, towards the
outer's final value.

## States Your App Owns

Hover, pressed and focus are noticed by the container. The rest — the last
submit failed, this row is selected, this value is out of range — are
conditions your app already holds, so there is nothing to propagate: pass the
signal and it is read where the style is resolved.

```rust
let wrong_password = create_signal(false);

container()
    .border(1.0, theme.line)
    .state(wrong_password, |s| s.border(2.0, theme.error))
    .child(text_input(password))
```

Because it is just a signal, anything else that needs it reads the same one,
independently — a label beside the field does not have to be told:

```rust
container()
    .state(wrong_password, |s| s.border(2.0, theme.error))
    .child(text("Wrong password"))
```

## Whose Hover?

A label inside a button has to light up while the pointer is on the button's
*padding*, nowhere near the glyphs. And it has to stay dark while the pointer
is over a different button in the same row. So neither "my own bounds" nor "any
ancestor" is the answer.

The answer is the nearest enclosing **control** — the interaction unit the
widget belongs to:

```rust
container()
    .padding(12.0)
    .on_click(save)
    .child(text("Save").color(theme.weak).when_hovered(|s| s.color(theme.strong)))
```

`on_click` makes that container a control, because anything the pointer can act
on is a unit by necessity. So do `on_hover`, `on_scroll`, `scrollable` and any
declared state layer. Write `control()` yourself where the boundary is real but
nothing else announces it:

```rust
container().control()
    .child(text("Password").when_focused(|s| s.color(theme.accent)))
    .child(text_input(password))
```

That is the case the old rule could not express at all: the focus is in a
*sibling*, not below the label. Both belong to the same unit, so the label can
ask about it.

Leaves get `when_hovered`, `when_pressed`, `when_focused` and `state` from the
`Stateful` trait, and the closure hands back the same builder the widget itself
uses.

**Nesting scopes resolution, not state.** A list row stays hovered while the
pointer is over a button nested in it — the pointer is plainly still on the
row. What the nested control changes is only who a descendant asks.

**With nothing marked above it**, a widget is its own unit and notices the
pointer over its own bounds. It is never pressed: being pressed means being
activated, and it has nothing to activate.

## Which Layer Wins

Layers are resolved in reverse declaration order, one property at a time: the
last one you wrote that is active and speaks about that property wins.

This is what lets a field say that its error outranks its focus ring — which
matters on a password field, where the focus is held essentially all the time:

```rust
container()
    .border(1.0, theme.line)
    .when_focused(|s| s.border(2.0, theme.accent))
    .state(wrong_password, |s| s.border(2.0, theme.error))   // written after,
    .child(text_input(password))                             // so it wins
```

Swap the two lines and the focus ring wins instead. A layer that says nothing
about a property is skipped rather than ending the search, so the pressed layer
below only takes over the transform and leaves the hover's background alone:

```rust
container()
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.scale(0.98))
```

Keep in mind that a layer *replaces* the base rather than ranking against it. A
border that already carries a meaning does not belong in the base — put it in a
layer with a condition, where the ordering above can speak about it.

## Combining Multiple Overrides

Each state can override multiple properties:

```rust
.when_hovered(|s| s
    .lighter(0.1)
    .border(2.0, Color::rgb(0.5, 0.7, 1.0))
    .elevation(6.0)
)

.when_pressed(|s| s
    .ripple()
    .darker(0.05)
    .scale(0.98)
    .elevation(2.0)
)
```

## With Animations

Add transitions for smooth state changes:

```rust
container()
    .background(Color::rgb(0.3, 0.5, 0.8))
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
    .animate_scale(Transition::spring(SpringConfig::SMOOTH))
    .when_hovered(|s| s.lighter(0.1).border(2.0, Color::WHITE))
    .when_pressed(|s| s.darker(0.1).scale(0.98))
```

## Button Patterns

### Simple Button

```rust
container()
    .padding(12.0)
    .background(Color::rgb(0.3, 0.5, 0.8))
    .corners(6.0)
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
    .on_click(|| println!("Clicked!"))
    .child(container().child(text("Click me").color(Color::WHITE)))
```

### Outlined Button

```rust
container()
    .padding(12.0)
    .background(Color::TRANSPARENT)
    .corners(6.0)
    .border(1.0, Color::rgb(0.5, 0.5, 0.6))
    .when_hovered(|s| s.background(Color::rgba(1.0, 1.0, 1.0, 0.1)))
    .when_pressed(|s| s.ripple())
    .child(container().child(text("Outlined").color(Color::WHITE)))
```

### Card with Lift

```rust
container()
    .padding(16.0)
    .background(Color::rgb(0.15, 0.15, 0.2))
    .corners(8.0)
    .elevation(2.0)
    .animate_elevation(Transition::new(200.0, TimingFunction::EaseOut))
    .when_hovered(|s| s.elevation(6.0).lighter(0.03))
    .when_pressed(|s| s.elevation(1.0))
    .children([...])
```

## API Reference

### StateStyle Builder

```rust
impl StateStyleBuilder {
    // Background
    pub fn background(self, color: Color) -> Self;
    pub fn lighter(self, amount: f32) -> Self;
    pub fn darker(self, amount: f32) -> Self;

    // Border — both halves, always; half a border is no border
    pub fn border(self, width: f32, color: Color) -> Self;

    // Other
    pub fn corner_radius(self, radius: f32) -> Self;
    // Every value is reactive here too, like the container's own
    pub fn translate<M>(self, translate: impl IntoSignal<Translate, M>) -> Self;
    pub fn rotate<M>(self, degrees: impl IntoSignal<f32, M>) -> Self;
    pub fn scale<M>(self, factor: impl IntoSignal<Scale, M>) -> Self;
    pub fn elevation(self, level: f32) -> Self;

    // Ripple
    pub fn ripple(self) -> Self;
    pub fn ripple_with_color(self, color: Color) -> Self;
}
```
