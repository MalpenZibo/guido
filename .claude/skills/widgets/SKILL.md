---
name: widgets
description: Guido's widget layer — the Widget trait, Container's builder API, layouts, state layers, the #[component] macro, and the rule that decides which properties are reactive. Use when adding or changing a widget, a widget property, a layout, or a conversion accepted by a property.
---

# Widgets

## The reactivity rule

**Everything that survives to paint takes `impl IntoSignal<T, M>`. Structural
declarations do not.**

Reactive: `background`, `gradient`, `backdrop_blur`, `overflow`, `corners`,
`border`, `translate`, `rotate`, `scale`, `pivot`, `width`, `height`,
`padding`, `visible`, `shadow` — and beyond `Container`, `Text`'s `wrap`,
`Image`'s `content_fit`, `TextInput`'s `password`, `mask_char` and `caret`,
`RippleConfig`'s colour, and the direction a `Flex` is built with.

Structural: `layout`, `child`, `children`, `control`, the axis a `Scroll` is
built with, the event handlers — and the *motion* a value is declared with,
which is the next section.

`Scroll` is what a family of setters looks like once it is one value.
`scrollable`, `scrollbar_visibility` and `scrollbar` were three, and the last
two were silent no-ops on a container that never became scrollable. Now
`container().scroll(Scroll::vertical())`, and the parts cannot be spelled apart
from the thing they configure. Its measurements — `width`, `hover_width`,
`margin`, `min_handle_size`, `reserve_gutter`, `visibility` — are reactive; its
appearance is `Container`'s own vocabulary, reached through `.track(|t| ..)`
and `.handle(|h| ..)`, because the scrollbar *is* two containers. Styling a
part means styling a container, so nothing has to be mirrored into a second
vocabulary as `Container` grows.

`autofocus` is the one that looks structural and is neither: it is a one-shot
consumed at the first layout, so "take focus when this appears" is an event,
and a signal form would mean something else.

A zero-argument setter naming the off case — `nowrap()`, `no_caret()` — is a
shorthand for the property beside it (`wrap(false)`, `caret(false)`), not a
second way to say it. The property is what takes the signal.

**A value read from an event handler is the exception, and it is a real one.**
`TextInput`'s two clipboard guards ask whether the field masks *now*, untracked,
rather than reading the copy layout took: an event does not wait for a layout
pass, so the cached answer would export a secret for the frame after the field
was told to hide it. Reading a declared value outside layout or paint means
asking why the cache is not good enough, and saying so where it is read.

An animatable property takes `impl IntoAnimated<T, M>` instead, which is
everything `IntoSignal` accepts plus a value carrying its own motion:
`background`, `corners`, `padding`, `border` (each half), `shadow`, `width`,
`height`, `translate`, `rotate`, `scale`. The others keep plain `IntoSignal`,
and so does every setter on `StateStyle` — a state layer supplies a value for a
property somebody else declared, so a timing there is a compile error rather
than a value quietly ignored.

A new property lands on one side of that line. If it changes what a pixel looks
like, it is reactive.

## A reactive property has three spellings, and they must agree

`IntoSignal` accepts a value, a closure, or a signal. A property is only
properly reactive when the *same expression* compiles in all three positions:

```rust
container().width(100.0)             // value
container().width(move || w.get())   // closure
container().width(w)                 // signal
```

Three different sets of impls serve them, and they drift apart silently —
nothing fails to build when one is missing; a call site refuses months later.

**Adding a conversion to a property type means adding all three:**

| spelling | impl | where |
| --- | --- | --- |
| value | `From<S> for T` | beside `T` |
| closure | `IntoVal<T> for S` | beside `T` |
| signal | `converting_signals!(S => T)` | beside `T` |

They cannot be collapsed into one blanket impl: `IntoVal` is reflexive, so a
blanket `S: IntoVal<T>` also covers `Signal<T> -> T`, collides with the
passthrough impl, and leaves the marker generic undecidable. Excluding the
reflexive case needs negative bounds or specialisation, neither stable.

So the lists are hand-kept, and `tests/signal_conversions.rs` is the only thing
standing between them and drift: **add a spelling there in the same change.**
Tracked in #226.

## The Widget trait

Two methods are required, the rest default:

- `layout(&mut self, tree, id, constraints) -> Size` — measure, then
  `tree.cache_layout(..)`
- `paint(&self, tree, id, ctx)` — draw into the `PaintContext`
- `event(&mut self, tree, id, event) -> EventResponse` — defaults to `Ignored`
- `advance_animations`, `reconcile_children`, `layout_hints`,
  `register_children` — defaults
- `refresh_paint_bounds` — a default too, and the one a widget that transforms
  itself has to override: a parent narrows its children to the visible rect
  before painting them, by their laid-out bounds, so a widget that draws
  somewhere else says how far with `Tree::set_own_paint_reach`. Called from the
  Paint job, which is where a reach that follows a paint-only property belongs —
  a transform must not reflow anything. A reach that follows a layout-tracked
  signal is published from `layout` instead, as `Text` and `TextInput` do for
  their stroke and shadow

Position and bounds live in the `Tree`, never on the widget. Writing a widget
from outside the crate needs `guido::widget_prelude::*` alongside the ordinary
prelude — see `tests/external_widget.rs`.

## Layout

`Container` is the one box. Layouts plug into it: `.layout(Flex::row())`,
`Flex::column()`, `ZStack`. The `Layout` trait is public, so an application can
write its own.

Children: `.child()` for a static one — a widget, or an `Option` of one that
is simply absent when `None`. `.children()` takes
whatever `IntoChildren` fits — an iterator of widgets is static, a closure is
dynamic and re-runs when what it read changes, and `keyed(data, key, build)`
reconciles by key so widget state survives a reorder:

```rust
container().children(keyed(
    move || tabs.get(),
    |tab| tab.title.clone(),
    tab_button,
))
```

## State layers

Hover, press and focus are overrides, not separate widgets, and they come from
the `Stateful` trait (`src/widgets/state_layer.rs`) — so anything implementing
it takes them, text as well as containers:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
    .when_focused(|s| s.border(2.0, Color::WHITE))
    .on_click(move || count.update(|c| *c += 1))
```

The closure receives a default style and returns the overrides; only what it
sets is overridden, and the rest is inherited. See
[docs/STATE_LAYER.md](../../../docs/STATE_LAYER.md).

## Corners

Radius and curvature are one property. A bare size means circular corners and
takes what `padding` takes; a constructor names another shape.

```rust
container().corners(12.0)
container().corners([16.0, 0.0])                  // rounded on top, square below
container().corners(Corners::squircle(12.0))      // K=2, iOS-style
container().corners(Corners::bevel(12.0))         // K=0, diagonal
container().corners(Corners::scoop(12.0))         // K=-1, concave
container().corners(Corners::superellipse(12.0, 1.5))
```

## Adding a widget property, end to end

1. Decide reactive or structural (the rule above).
2. Add the setter; if it takes a new conversion, add all three spellings and a
   line in `tests/signal_conversions.rs`.
3. Make it reach paint, and add a scenario to `tests/golden_images.rs` if it
   changes pixels — see the `visual-verification` skill.
4. Update `docs/` where the pattern is described, and the book chapter that
   teaches it — see the `book` skill.

## The motion rides with the value

`.transition(..)` and `.timeline(..)` hang off `IntoSignal` itself, so they are
available on everything a property setter already accepts, and each returns an
`Animated<T>`:

```rust
container()
    .background(theme.surface.transition(200.0))
    .width((move || if open.get() { 520.0 } else { 120.0 }).transition(SpringConfig::SNAPPY))
    .rotate(0.0.timeline(shake(), rejections))
```

A bare number is milliseconds, eased out — the one place the animation
vocabulary has two defaults, because `Transition::default()` is a spring and
has no duration for a number to attach to.

Three rules the shape depends on, in order of how easily they are broken:

- **A declaration is the whole property.** Restating one replaces the value
  *and* the motion, so `.background(x.transition(200.0)).background(y)` leaves
  no animation behind. There are never two declarations for one property to
  reconcile.
- **`Animated<T>` is not an `IntoSignal`.** That is what makes a timing on a
  state-layer override a compile error. Adding an `IntoSignal` impl for it
  would put the rule back into prose.
- **`timeline` needs `T: Animatable`.** That is what keeps a timeline off a
  property whose declared type is not the type it animates — `width` and
  `height` declare a `Length` and move an `f32` — so `width(w.timeline(..))`
  does not compile rather than compiling and playing nothing.

A new animatable property adds its name to four lists that nothing keeps in
step: `ContainerAnims`'s fields, `start_timeline!` and the `advance_anim!`
block in `advance_animations`, and the drift block in
`resync_animation_targets`. Miss one and the property silently never plays or
never wakes.
