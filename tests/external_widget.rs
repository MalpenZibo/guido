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
//!
//! It also covers `event`, which is not required but is what a widget needs to
//! be pointed at. That method's signature names `Event`, and a positional
//! `Event` names `Point`, so this is where those two being reachable from the
//! documented pair of preludes is checked.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::widget_prelude::*;

/// A box whose size follows a signal. Deliberately minimal: `layout` and
/// `paint` are the only required methods, and the tracking scope is the only
/// thing it needs from the reactive system.
struct Bar {
    extent: Signal<f32>,
    measured: f32,
    /// Where the last pointer event said it was, if it said anywhere. Shared
    /// with the test, which has no other way to look inside a boxed widget.
    pointed_at: Rc<Cell<Option<Option<Point>>>>,
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

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, event: &Event) -> EventResponse {
        // A pointer event may have no position — see `Event::coords`. A widget
        // outside the crate has to be able to say so, which means naming the
        // `Option` rather than unwrapping it.
        if let Event::MouseMove { at } = event {
            self.pointed_at.set(Some(*at));
            return EventResponse::Handled;
        }
        EventResponse::Ignored
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
        pointed_at: Rc::new(Cell::new(None)),
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
        pointed_at: Rc::new(Cell::new(None)),
    });

    extent.set(40.0);
    let size = tree
        .with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 400.0))
        })
        .expect("root is registered");

    assert_eq!(size, Size::new(40.0, 40.0));
}

/// A pointer event reaches a widget written outside the crate, and it can tell
/// a position from the absence of one.
///
/// `Event`'s positional variants carry `Option<Point>` rather than a pair of
/// floats, because a subtree collapsed to nothing has no position to give and
/// no number could have said so (#227). That is public API, so a third-party
/// widget has to be able to spell both halves — with nothing imported but the
/// two preludes.
#[test]
fn a_widget_from_outside_the_crate_can_tell_a_position_from_none() {
    let extent = create_signal(20.0f32);
    let pointed_at = Rc::new(Cell::new(None));
    let (mut tree, root, _) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
        pointed_at: Rc::clone(&pointed_at),
    });

    let mut send = |at| {
        tree.with_widget_mut(root, |w, id, t| w.event(t, id, &Event::MouseMove { at }));
    };

    send(Some(Point::new(3.0, 4.0)));
    assert_eq!(
        pointed_at.get(),
        Some(Some(Point::new(3.0, 4.0))),
        "a positioned move arrives with its position"
    );

    send(None);
    assert_eq!(
        pointed_at.get(),
        Some(None),
        "and one that lost its position arrives without it, rather than not \
         arriving or arriving at the origin"
    );
}
