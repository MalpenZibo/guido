# Custom Widgets and Layouts

Guido ships few widgets on purpose. `Container` is the box — padding, colours,
corners, borders, events — and `Text`, `Image` and `TextInput` are the things
that draw content. A checkbox, a toggle, a slider are compositions of those,
written as functions or as `#[component]`s in your own app.

Occasionally that is not enough: you need to draw something the primitives
cannot express, or arrange children in a way `Flex` and `ZStack` do not. Both
are extension points, and both are reachable from outside the crate.

## The two preludes

`guido::prelude` is the application's vocabulary. Writing a widget or a layout
needs a second one:

```rust
# extern crate guido;
# fn main() {
use guido::prelude::*;
use guido::widget_prelude::*;
# ;
# }
```

`widget_prelude` adds what the `Widget` and `Layout` signatures name — `Tree`,
`WidgetId`, `Constraints`, `PaintContext`, `RenderNode`, `LayoutHints`,
`ChildPaintOptions` — and `Transform`, which is what a widget positions what
it draws with. An application never names `Transform`: it declares `translate`,
`rotate` and `scale`, and those compose into one.

It also carries `with_signal_tracking` and `JobType`. Neither is needed to
write a widget any more — the framework scopes both passes — but they are what
a widget reaches for to claim a *narrower* attribution than its own pass: one
dynamic-children segment rather than the whole of it.

## A widget

`Widget` has two required methods. Everything else — `event`,
`advance_animations`, `reconcile_children`, `layout_hints`,
`register_children`, `refresh_paint_bounds` — has a default.

One of those defaults is worth knowing about if your widget draws outside the
box it was given. A parent narrows its children to the visible region before
painting them, and it does that by their laid-out bounds — so a widget that
paints elsewhere, because it transforms itself or casts something past its
edges, has to say how far. That is `refresh_paint_bounds`, which reports it with
`Tree::set_own_paint_reach`. It runs from the paint job rather than from layout,
so saying it never costs a reflow.

Which pass you publish from follows what your reach depends on. If it moves with
something layout already tracks — a size, a font — publish it from `layout`,
which is what the built-in text widgets do for their stroke and shadow. If it
moves with a paint-only property such as a transform, publish it here.

```rust,ignore
struct Bar {
    extent: Signal<f32>,
    measured: f32,
}

impl Widget for Bar {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        let extent = self.extent.get();
        self.measured = extent;
        Size::new(extent, extent)
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Local coordinates: the parent has already placed this node.
        ctx.draw_rounded_rect(
            Rect::new(0.0, 0.0, self.measured, self.measured),
            Color::RED,
            0.0,
        );
    }
}
```

Three things are worth spelling out.

**Position lives in the `Tree`, not on the widget.** `layout` measures and
reports; the parent decides where the result goes. Paint therefore happens in
local coordinates, with `(0, 0)` at the widget's own origin.

**A layout measures; it does not decide whether it runs.** The framework calls
it through `LayoutCtx::layout_child`, and that call holds the check that skips a
widget with the same constraints and nothing dirty, the scope that makes the
widget's reads belong to *it*, the pass it is in, and the cache the size goes
into. So there is nothing to remember: read the signals plainly and return a
size.

**A paint draws; it does not scope itself either.** The framework calls it
through `PaintContext::paint_child`, and that call holds the cull test that
skips a widget nobody can see, the cache a clean one is served from, and the
same scope layout gets — so a signal read while drawing belongs to this widget
and repaints this widget. `ctx.id()` is which widget it is and `ctx.tree()` is
the tree it is in.

This is newer than it looks: until #390 a paint really did have to open its
own scope, and a widget that did not subscribed to *nothing* — it drew once and
then stayed as it was. The advice here used to say that the read fell through
to the parent container. It did not.

**Which pass is running is `ctx.measuring()`.** True while a natural size is
being measured, before a surface exists, which is when an animated value reads
as where it is going rather than where it is. A widget that animates nothing
never has to ask.

## A layout

`Layout` has one method: given the children and the constraints, place them and
report the size the parent should use.

```rust,ignore
struct Stagger {
    step: f32,
}

impl Layout for Stagger {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let mut size = Size::zero();
        for (i, &child_id) in children.iter().enumerate() {
            let child = tree
                .with_widget_mut(child_id, |w, id, t| w.layout(t, id, constraints))
                .unwrap_or_default();

            let x = origin.0 + i as f32 * self.step;
            let y = origin.1 + i as f32 * self.step;
            tree.set_origin(child_id, x, y);

            size.width = size.width.max(x + child.width - origin.0);
            size.height = size.height.max(y + child.height - origin.1);
        }
        constraints.constrain(size)
    }
}
```

It plugs into any container:

```rust,ignore
# extern crate guido;
# use guido::prelude::*;
# fn card(_label: &str) -> Container { container() }
# fn main() {
container()
    .layout(Stagger { step: 8.0 })
    .child(card("one"))
    .child(card("two"))
# ;
# }
```

The absence of a built-in grid is not a gap — this is where a grid goes.

If your layout reads signals of its own (a reactive spacing, an alignment that
follows a setting), read them straight from `layout`: the container runs it
inside its own tracking scope, so those reads are attributed correctly without
you opening one.

`tree.with_widget(child_id, |w| w.layout_hints())` reports whether a child
wants to fill an axis, which is what lets a layout give the remaining space to
the children that asked for it — `ZStack` in the guido source is the short
example to read.

## Checking your work

`tests/external_widget.rs` in the guido repository is a widget written against
the public API only, with a test that lays it out and paints it. It is the
shortest complete example of everything on this page.
