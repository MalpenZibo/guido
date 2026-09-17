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
/// `paint` are the only required methods, and what a layout has to do is read
/// a signal and answer with a size — the scope that attributes that read, the
/// check that decides whether it runs at all, and the cache it answers into
/// belong to the framework, around the call.
struct Bar {
    extent: Signal<f32>,
    measured: f32,
    /// How many times the framework has asked this widget to lay itself out.
    /// A leaf that writes no skip check of its own still must not be asked
    /// twice for an answer it has already given.
    layouts: Rc<Cell<u32>>,
    /// Where the last pointer event said it was, if it said anywhere. Shared
    /// with the test, which has no other way to look inside a boxed widget.
    pointed_at: Rc<Cell<Option<Option<Point>>>>,
}

impl Widget for Bar {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        self.layouts.set(self.layouts.get() + 1);
        let extent = self.extent.get();
        self.measured = extent;
        Size::new(extent, extent)
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
        .layout_widget(root, Constraints::new(0.0, 0.0, 400.0, 400.0))
        .expect("root is registered");
    (tree, root, size)
}

#[test]
fn a_widget_from_outside_the_crate_lays_out_and_paints() {
    let extent = create_signal(20.0f32);
    let (mut tree, root, size) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
        layouts: Rc::new(Cell::new(0)),
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

/// The value follows the signal on the layout that runs after a write.
///
/// Marked by hand, because driving the job queue that turns a write into
/// `needs_layout` is crate-internal — as is *which* widget it marks, which is
/// asserted where that is reachable:
/// `a_layout_is_attributed_to_the_widget_that_ran_it` in `src/lib.rs`, and
/// `reactive::invalidation::the_innermost_scope_owns_the_read` beneath it. What
/// this half says is that the widget re-reads: a leaf laid out again answers
/// from the signal, not from what it measured last time.
#[test]
fn it_re_measures_from_the_signal_it_read() {
    let extent = create_signal(20.0f32);
    let (mut tree, root, _) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
        layouts: Rc::new(Cell::new(0)),
        pointed_at: Rc::new(Cell::new(None)),
    });

    extent.set(40.0);
    tree.mark_needs_layout(root);
    let size = tree
        .layout_widget(root, Constraints::new(0.0, 0.0, 400.0, 400.0))
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
        layouts: Rc::new(Cell::new(0)),
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

/// A leaf that writes no skip check of its own is not asked twice for the same
/// answer.
///
/// The check belongs to the framework, around the call, rather than to each
/// widget inside it: `Container` and `Text` wrote one, `TextInput` and `Image`
/// did not, and a widget written outside the crate had to know it should.
#[test]
fn a_leaf_with_no_check_of_its_own_is_not_laid_out_twice_for_one_answer() {
    let extent = create_signal(20.0f32);
    let layouts = Rc::new(Cell::new(0));
    let (mut tree, root, _) = lay_out(Bar {
        extent: extent.into(),
        measured: 0.0,
        layouts: Rc::clone(&layouts),
        pointed_at: Rc::new(Cell::new(None)),
    });
    assert_eq!(layouts.get(), 1, "the first layout runs");

    tree.layout_widget(root, Constraints::new(0.0, 0.0, 400.0, 400.0));

    assert_eq!(
        layouts.get(),
        1,
        "the same constraints, and nothing dirty: there is nothing to ask"
    );
}
