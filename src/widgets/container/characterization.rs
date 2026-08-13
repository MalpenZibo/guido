//! Characterization tests: what `Container` does *today*.
//!
//! These pin observable behaviour — resulting sizes, emitted draw commands,
//! event outcomes, invalidation job types — without asserting anything about
//! how `layout`/`paint`/`event` are structured internally. They are the net
//! the container refactor moves under: a test that fails here means the
//! refactor changed behaviour, which is the one thing it must not do.
//!
//! When a test documents something that looks wrong rather than merely
//! surprising, it says so in its own comment instead of asserting the
//! desirable value — pinning reality is the job; fixing it is a separate
//! change with its own test.

use rustc_hash::FxHashSet;

use super::*;
use crate::jobs::{self, JobType};
use crate::layout::{Constraints, Flex, at_least, at_most, fill, fraction};
use crate::reactive::create_signal;
use crate::renderer::{DrawCommand, PaintContext, RenderNode};
use crate::widgets::widget::{Event, EventResponse, MouseButton};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct H {
    tree: Tree,
    root: WidgetId,
}

impl H {
    fn new(widget: impl Widget + 'static) -> Self {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        Self { tree, root }
    }

    fn layout(&mut self, constraints: Constraints) -> Size {
        let root = self.root;
        self.tree
            .with_widget_mut(root, |w, id, t| w.layout(t, id, constraints))
            .expect("root is registered")
    }

    /// Lay out loose within `w` x `h`.
    fn fit(&mut self, w: f32, h: f32) -> Size {
        self.layout(Constraints::new(0.0, 0.0, w, h))
    }

    fn paint(&mut self) -> RenderNode {
        let root = self.root;
        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
        node
    }

    fn send(&mut self, event: Event) -> EventResponse {
        let root = self.root;
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event))
            .expect("root is registered")
    }

    fn children(&self) -> Vec<WidgetId> {
        self.tree.get_children(self.root).to_vec()
    }

    fn roots(&self) -> FxHashSet<WidgetId> {
        [self.root].into_iter().collect()
    }

    /// Discard every queued job so the next drain only sees what follows.
    fn drain_jobs(&mut self) {
        let roots = self.roots();
        jobs::distribute_jobs(&self.tree, &roots);
        jobs::recycle_job_buffer(jobs::drain_surface_jobs(self.root));
        jobs::recycle_job_buffer(jobs::drain_orphan_jobs());
    }

    /// Job types queued for the root widget by `write`.
    fn jobs_from(&mut self, write: impl FnOnce()) -> Vec<JobType> {
        self.drain_jobs();
        write();
        let roots = self.roots();
        jobs::distribute_jobs(&self.tree, &roots);
        let drained = jobs::drain_surface_jobs(self.root);
        let mut types: Vec<JobType> = drained
            .iter()
            .filter(|j| j.widget_id == self.root)
            .map(|j| j.job_type)
            .collect();
        types.sort_by_key(|t| format!("{t:?}"));
        types.dedup();
        jobs::recycle_job_buffer(drained);
        types
    }
}

/// A leaf of an exactly known size.
fn box_of(w: f32, h: f32) -> Container {
    container().width(w).height(h)
}

/// Every `RoundedRect` drawn by a node, in order, as (bounds, fill).
fn rects(node: &RenderNode) -> Vec<(Rect, Color)> {
    node.commands
        .iter()
        .filter_map(|c| match &**c {
            DrawCommand::RoundedRect { rect, color, .. } => Some((*rect, *color)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Sizing — the box model matrix
// ---------------------------------------------------------------------------

#[test]
fn content_sizing_is_child_plus_padding() {
    let mut h = H::new(container().padding(8.0).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0), Size::new(56.0, 36.0));
}

#[test]
fn exact_length_wins_over_content() {
    let mut h = H::new(container().width(60.0).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 60.0);
}

#[test]
fn fill_takes_the_whole_offered_axis() {
    let mut h = H::new(container().width(fill()).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 500.0);
}

#[test]
fn fraction_resolves_against_the_incoming_max() {
    let mut h = H::new(container().width(fraction(0.5)).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 250.0);
}

#[test]
fn at_least_raises_a_smaller_content() {
    let mut h = H::new(container().width(at_least(100.0)).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 100.0);
}

#[test]
fn at_most_caps_a_larger_content() {
    let mut h = H::new(container().width(at_most(30.0)).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 30.0);
}

/// An explicit width answers only to the parent's maximum, never to its
/// minimum — that is what lets `.width(60)` stay 60 inside a stretching parent.
#[test]
fn explicit_width_ignores_the_parent_minimum() {
    let mut h = H::new(container().width(60.0).child(box_of(40.0, 20.0)));
    assert_eq!(
        h.layout(Constraints::new(200.0, 0.0, 500.0, 500.0)).width,
        60.0
    );
}

#[test]
fn content_sizing_obeys_the_parent_minimum() {
    let mut h = H::new(container().child(box_of(40.0, 20.0)));
    assert_eq!(
        h.layout(Constraints::new(200.0, 0.0, 500.0, 500.0)).width,
        200.0
    );
}

#[test]
fn the_default_layout_stacks_children_in_a_column() {
    let mut h = H::new(
        container()
            .child(box_of(40.0, 20.0))
            .child(box_of(10.0, 30.0)),
    );
    assert_eq!(h.fit(500.0, 500.0), Size::new(40.0, 50.0));
}

#[test]
fn a_row_sums_along_the_main_axis() {
    let mut h = H::new(
        container()
            .layout(Flex::row().spacing(4.0))
            .child(box_of(40.0, 20.0))
            .child(box_of(10.0, 30.0)),
    );
    assert_eq!(h.fit(500.0, 500.0), Size::new(54.0, 30.0));
}

#[test]
fn padding_shrinks_what_children_are_offered() {
    let mut h = H::new(
        container()
            .width(100.0)
            .padding(10.0)
            .child(container().width(fill()).height(5.0)),
    );
    h.fit(500.0, 500.0);
    let child = h.children()[0];
    assert_eq!(h.tree.get_bounds(child).unwrap().width, 80.0);
}

/// Content larger than the container grows it back unless shrinking was
/// explicitly allowed (hidden overflow, scrolling, an exact length, or a
/// running size animation).
#[test]
fn overflowing_content_grows_a_visible_container() {
    let mut h = H::new(container().child(box_of(400.0, 10.0)));
    assert_eq!(h.fit(100.0, 500.0).width, 100.0, "still clamped to the max");

    let mut hidden = H::new(
        container()
            .overflow(Overflow::Hidden)
            .child(box_of(400.0, 10.0)),
    );
    assert_eq!(hidden.fit(100.0, 500.0).width, 100.0);
}

/// Attaching an animation must not change what children are offered. It used
/// to: the animated path skipped the `at_most` clamp, so children were laid out
/// against the parent's full width and then the container was cut back to the
/// cap — content overflowing a box that had told it there was room.
#[test]
fn at_most_bounds_children_with_or_without_a_size_animation() {
    use crate::animation::{TimingFunction, Transition};

    for animated in [false, true] {
        let inner = container().width(fill()).height(5.0);
        let outer = container().width(at_most(60.0));
        let outer = if animated {
            outer.animate_width(Transition::new(200, TimingFunction::EaseOut))
        } else {
            outer
        };

        let mut h = H::new(outer.child(inner));
        h.fit(500.0, 500.0);
        let child = h.children()[0];
        assert_eq!(
            h.tree.get_bounds(child).unwrap().width,
            60.0,
            "child must be bounded by the cap (animated: {animated})"
        );
        assert_eq!(h.tree.cached_size(h.root).unwrap().width, 60.0);
    }
}

/// The worst of the same bug: a wrapping child measured against the wrong width
/// gets the wrong *height* too, and no clamp downstream can recover it. Text
/// laid out as one 366px line inside a 100px box came out 16.8 tall instead of
/// 84 — the container was the right width and the wrong shape.
///
/// Font-independent by construction: it asserts the two agree, never a metric.
#[test]
fn a_size_animation_does_not_change_how_content_wraps() {
    use crate::animation::{TimingFunction, Transition};
    use crate::widgets::text::text;

    let sentence = "una frase abbastanza lunga da dover andare a capo piu volte";

    let mut plain = H::new(container().width(at_most(100.0)).child(text(sentence)));
    let plain_size = plain.fit(500.0, 500.0);

    let mut animated = H::new(
        container()
            .width(at_most(100.0))
            .animate_width(Transition::new(200, TimingFunction::EaseOut))
            .child(text(sentence)),
    );
    let animated_size = animated.fit(500.0, 500.0);

    assert_eq!(
        plain_size, animated_size,
        "an attached animation must not change how the content wraps"
    );
}

#[test]
fn an_invisible_container_measures_zero() {
    let mut h = H::new(container().visible(false).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0), Size::zero());
}

// ---------------------------------------------------------------------------
// Layout protocol
// ---------------------------------------------------------------------------

/// The early-out that makes partial layout worth having: same constraints and
/// nothing dirty means the cached size comes back without re-running.
#[test]
fn a_clean_relayout_with_equal_constraints_is_skipped() {
    let mut h = H::new(container().padding(8.0).child(box_of(40.0, 20.0)));
    let first = h.fit(500.0, 500.0);
    let child = h.children()[0];
    h.tree.set_origin(child, 999.0, 999.0); // would be overwritten by a real layout

    let second = h.fit(500.0, 500.0);
    assert_eq!(second, first);
    assert_eq!(
        h.tree.get_bounds(child).unwrap().x,
        999.0,
        "the skipped layout must not have repositioned children"
    );
}

#[test]
fn changed_constraints_force_a_relayout() {
    let mut h = H::new(container().width(fill()).child(box_of(40.0, 20.0)));
    assert_eq!(h.fit(500.0, 500.0).width, 500.0);
    assert_eq!(h.fit(300.0, 500.0).width, 300.0);
}

/// Fixing both axes makes a container a relayout boundary, so a dirty
/// descendant stops bubbling there instead of reaching the surface root.
#[test]
fn fixed_size_makes_a_relayout_boundary() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(50.0)
            .child(box_of(40.0, 20.0)),
    );
    h.fit(500.0, 500.0);
    assert!(h.tree.is_relayout_boundary(h.root));

    let mut loose = H::new(container().child(box_of(40.0, 20.0)));
    loose.fit(500.0, 500.0);
    assert!(!loose.tree.is_relayout_boundary(loose.root));
}

// ---------------------------------------------------------------------------
// Scroll
// ---------------------------------------------------------------------------

/// Whether a scrollbar exists at all comes down to one comparison — content
/// against viewport — so the viewport a scroller records has to be its
/// *visible* extent, never the unbounded constraint the scrolled axis is laid
/// out with. An infinite viewport is never exceeded, and the scrollbar
/// silently stops being drawn. This asserts on the paint output because that
/// is exactly what "the scrollbars stopped appearing" means.
#[test]
fn an_overflowing_scroller_paints_its_scrollbar() {
    let mut h = H::new(
        container()
            .height(100.0)
            .scrollable(ScrollAxis::Vertical)
            .child(box_of(40.0, 900.0)),
    );
    h.fit(500.0, 500.0);

    let node = h.paint();
    assert!(
        node.children.len() > 1,
        "expected the content plus a scrollbar, got {} node(s)",
        node.children.len()
    );
}

#[test]
fn an_overflowing_horizontal_scroller_paints_its_scrollbar() {
    let mut h = H::new(
        container()
            .width(100.0)
            .scrollable(ScrollAxis::Horizontal)
            .child(box_of(900.0, 40.0)),
    );
    h.fit(500.0, 500.0);
    assert!(h.paint().children.len() > 1);
}

/// The other half of the same contract: content that fits draws no scrollbar.
#[test]
fn a_scroller_whose_content_fits_paints_no_scrollbar() {
    let mut h = H::new(
        container()
            .height(100.0)
            .scrollable(ScrollAxis::Vertical)
            .child(box_of(40.0, 20.0)),
    );
    h.fit(500.0, 500.0);
    assert_eq!(
        h.paint().children.len(),
        1,
        "only the content should be painted"
    );
}

#[test]
fn a_vertical_scroller_offers_children_unbounded_height() {
    let mut h = H::new(
        container()
            .height(100.0)
            .scrollable(ScrollAxis::Vertical)
            .child(box_of(40.0, 900.0)),
    );
    assert_eq!(
        h.fit(500.0, 500.0).height,
        100.0,
        "the viewport stays fixed"
    );
    let child = h.children()[0];
    assert_eq!(
        h.tree.get_bounds(child).unwrap().height,
        900.0,
        "the content keeps its natural height"
    );
}

// ---------------------------------------------------------------------------
// Paint
// ---------------------------------------------------------------------------

#[test]
fn a_background_emits_one_rounded_rect() {
    let mut h = H::new(container().width(50.0).height(20.0).background(Color::RED));
    h.fit(500.0, 500.0);
    let node = h.paint();
    let drawn = rects(&node);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].0, Rect::new(0.0, 0.0, 50.0, 20.0));
    assert_eq!(drawn[0].1, Color::RED);
}

#[test]
fn a_fully_transparent_background_draws_nothing() {
    let mut h = H::new(container().width(50.0).height(20.0));
    h.fit(500.0, 500.0);
    assert!(rects(&h.paint()).is_empty());
}

#[test]
fn a_border_is_its_own_command() {
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .background(Color::RED)
            .border(2.0, Color::BLUE),
    );
    h.fit(500.0, 500.0);
    assert_eq!(rects(&h.paint()).len(), 2, "background + border frame");
}

#[test]
fn an_invisible_container_paints_nothing() {
    let mut h = H::new(
        container()
            .visible(false)
            .width(50.0)
            .height(20.0)
            .background(Color::RED)
            .child(box_of(10.0, 10.0)),
    );
    h.fit(500.0, 500.0);
    let node = h.paint();
    assert!(node.commands.is_empty());
    assert!(node.children.is_empty());
}

#[test]
fn children_become_child_nodes_positioned_by_transform() {
    let mut h = H::new(
        container()
            .padding(8.0)
            .child(box_of(40.0, 20.0).background(Color::RED)),
    );
    h.fit(500.0, 500.0);
    let node = h.paint();
    assert_eq!(node.children.len(), 1);
    let child = &node.children[0];
    assert_eq!(child.bounds, Rect::new(0.0, 0.0, 40.0, 20.0));
    assert_eq!(
        (child.local_transform.tx(), child.local_transform.ty()),
        (8.0, 8.0)
    );
}

#[test]
fn hidden_overflow_sets_a_clip_region() {
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .overflow(Overflow::Hidden)
            .child(box_of(400.0, 10.0)),
    );
    h.fit(500.0, 500.0);
    assert!(h.paint().clip.is_some());

    let mut visible = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .child(box_of(400.0, 10.0)),
    );
    visible.fit(500.0, 500.0);
    assert!(visible.paint().clip.is_none());
}

#[test]
fn a_zero_area_container_skips_its_children() {
    let mut h = H::new(container().width(0.0).height(0.0).child(box_of(40.0, 20.0)));
    h.fit(500.0, 500.0);
    assert!(h.paint().children.is_empty());
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn click_at(x: f32, y: f32) -> [Event; 2] {
    [
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
        },
        Event::MouseUp {
            x,
            y,
            button: MouseButton::Left,
        },
    ]
}

#[test]
fn a_click_inside_the_bounds_fires_on_click() {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = clicks.clone();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .on_click(move || counter.set(counter.get() + 1)),
    );
    h.fit(500.0, 500.0);

    for e in click_at(25.0, 10.0) {
        h.send(e);
    }
    assert_eq!(clicks.get(), 1);

    for e in click_at(200.0, 10.0) {
        h.send(e);
    }
    assert_eq!(clicks.get(), 1, "a click outside must not fire");
}

/// Hit testing follows the rounded shape, not its bounding box.
#[test]
fn a_corner_radius_excludes_the_corner() {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = clicks.clone();
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .corner_radius(20.0)
            .on_click(move || counter.set(counter.get() + 1)),
    );
    h.fit(500.0, 500.0);

    for e in click_at(1.0, 1.0) {
        h.send(e);
    }
    assert_eq!(clicks.get(), 0, "the top-left corner is outside the circle");

    for e in click_at(20.0, 20.0) {
        h.send(e);
    }
    assert_eq!(clicks.get(), 1);
}

#[test]
fn hover_tracks_entering_and_leaving() {
    let state = std::rc::Rc::new(std::cell::Cell::new(false));
    let sink = state.clone();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .on_hover(move |hovered| sink.set(hovered)),
    );
    h.fit(500.0, 500.0);

    h.send(Event::MouseMove { x: 25.0, y: 10.0 });
    assert!(state.get());

    h.send(Event::MouseMove { x: 300.0, y: 10.0 });
    assert!(!state.get());
}

/// Children see the event first, and a handled event stops there.
#[test]
fn a_child_handles_before_its_parent() {
    let order = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let child_sink = order.clone();
    let parent_sink = order.clone();

    let mut h = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .on_click(move || parent_sink.borrow_mut().push("parent"))
            .child(
                container()
                    .width(50.0)
                    .height(50.0)
                    .on_click(move || child_sink.borrow_mut().push("child")),
            ),
    );
    h.fit(500.0, 500.0);

    for e in click_at(25.0, 25.0) {
        h.send(e);
    }
    assert_eq!(*order.borrow(), vec!["child"]);
}

#[test]
fn scroll_reaches_the_callback_inside_the_bounds() {
    let deltas = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let sink = deltas.clone();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .on_scroll(move |_dx, dy, _src| sink.set(sink.get() + dy)),
    );
    h.fit(500.0, 500.0);

    h.send(Event::Scroll {
        x: 25.0,
        y: 10.0,
        delta_x: 0.0,
        delta_y: 12.0,
        source: crate::widgets::widget::ScrollSource::Wheel,
    });
    assert_eq!(deltas.get(), 12.0);
}

// ---------------------------------------------------------------------------
// Reactivity: which job type each property invalidates
// ---------------------------------------------------------------------------

/// Layout-affecting properties queue a Layout job when their signal changes.
#[test]
fn padding_invalidates_layout() {
    let pad = create_signal(4.0f32);
    let mut h = H::new(
        container()
            .padding(move || pad.get())
            .child(box_of(10.0, 10.0)),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| pad.set(12.0));
    assert!(
        queued.contains(&JobType::Layout),
        "padding must invalidate layout, got {queued:?}"
    );
}

#[test]
fn width_invalidates_layout() {
    let w = create_signal(10.0f32);
    let mut h = H::new(container().width(move || w.get()).child(box_of(10.0, 10.0)));
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| w.set(40.0));
    assert!(queued.contains(&JobType::Layout), "got {queued:?}");
}

/// Paint-only properties must NOT drag layout along with them.
#[test]
fn background_invalidates_paint_only() {
    let bg = create_signal(Color::RED);
    let mut h = H::new(
        container()
            .width(10.0)
            .height(10.0)
            .background(move || bg.get()),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| bg.set(Color::BLUE));
    assert!(queued.contains(&JobType::Paint), "got {queued:?}");
    assert!(
        !queued.contains(&JobType::Layout),
        "a colour change must not relayout, got {queued:?}"
    );
}

#[test]
fn corner_radius_invalidates_paint_only() {
    let r = create_signal(2.0f32);
    let mut h = H::new(
        container()
            .width(10.0)
            .height(10.0)
            .background(Color::RED)
            .corner_radius(move || r.get()),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| r.set(6.0));
    assert!(queued.contains(&JobType::Paint), "got {queued:?}");
    assert!(!queued.contains(&JobType::Layout), "got {queued:?}");
}

#[test]
fn visibility_invalidates_layout() {
    let shown = create_signal(true);
    let mut h = H::new(
        container()
            .visible(move || shown.get())
            .child(box_of(10.0, 10.0)),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| shown.set(false));
    assert!(queued.contains(&JobType::Layout), "got {queued:?}");
}

// The remaining row of this table — a *layout's* reactive property (Flex's
// spacing, and with it every property of every user-written `Layout`) must
// invalidate its container too — is covered by
// `tests::a_layout_property_invalidates_its_container` in `mod.rs`, where it
// carries the history of the bug it regressed on.
