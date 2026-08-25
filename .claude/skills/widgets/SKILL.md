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
`padding`, `visible`, `elevation`.

Structural: `layout`, `child`, `children`, `scrollable`, `scrollbar`,
`scrollbar_visibility`, `control`, `animate_*`, the event handlers.

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

Position and bounds live in the `Tree`, never on the widget. Writing a widget
from outside the crate needs `guido::widget_prelude::*` alongside the ordinary
prelude — see `tests/external_widget.rs`.

## Layout

`Container` is the one box. Layouts plug into it: `.layout(Flex::row())`,
`Flex::column()`, `ZStack`. The `Layout` trait is public, so an application can
write its own.

Children: `.child()` and `.maybe_child()` for static ones, `.children()` for a
list, `.children_dyn()` for keyed reconciliation that preserves widget state
across reorders.

## State layers

Hover and pressed are overrides, not separate widgets:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
    .on_click(move || count.update(|c| *c += 1))
```

See [docs/STATE_LAYER.md](../../../docs/STATE_LAYER.md).

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
