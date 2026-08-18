//! A leaf written from outside the crate, against the public API only.
//!
//! Two things have to hold for a third-party widget to be a first-class one:
//! it has to compile without reaching into the crate, and its signal reads
//! have to belong to *it*. The second is the reason `with_signal_tracking` and
//! `JobType` are exported — without them a leaf still updates, but its reads
//! register against the nearest ancestor that opened a scope, so a change to
//! its own content re-lays-out every sibling it has.
//!
//! This file covers the first: the two preludes are the only imports, which is
//! the point of `widget_prelude` existing — the tree, the paint context and the
//! tracking scope used to have to be reached for one module at a time.

use guido::prelude::*;
use guido::widget_prelude::*;

/// A box whose size follows a signal. Deliberately minimal: `layout` and
/// `paint` are the only required methods, and the tracking scope is the only
/// thing it needs from the reactive system.
struct Bar {
    extent: Signal<f32>,
    measured: f32,
}

impl Widget for Bar {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        let extent = with_signal_tracking(id, JobType::Layout, || self.extent.get());
        self.measured = extent;
        let size = Size::new(extent, extent);
        tree.cache_layout(id, constraints, size);
        tree.clear_needs_layout(id);
        size
    }

    fn paint(&self, _tree: &Tree, _id: WidgetId, ctx: &mut PaintContext) {
        ctx.draw_rounded_rect(
            Rect::new(0.0, 0.0, self.measured, self.measured),
            Color::RED,
            0.0,
        );
    }
}

fn lay_out(widget: impl Widget + 'static) -> (Tree, WidgetId, Size) {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    let size = tree
        .with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 400.0))
        })
        .expect("root is registered");
    (tree, root, size)
}

#[test]
fn a_widget_from_outside_the_crate_lays_out_and_paints() {
    let extent = create_signal(20.0f32);
    let (mut tree, root, size) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
    });

    assert_eq!(size, Size::new(20.0, 20.0));

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });
    assert!(!node.commands.is_empty(), "the widget drew nothing");
}

/// The value follows the signal on a later layout, which is the half of
/// reactivity this test can see from outside: driving the job queue that turns
/// a write into `needs_layout` is crate-internal, so *which widget* gets
/// marked is asserted where that is reachable —
/// `reactive::invalidation::the_innermost_scope_owns_the_read`.
#[test]
fn it_re_measures_from_the_signal_it_read() {
    let extent = create_signal(20.0f32);
    let (mut tree, root, _) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
    });

    extent.set(40.0);
    let size = tree
        .with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 400.0))
        })
        .expect("root is registered");

    assert_eq!(size, Size::new(40.0, 40.0));
}
