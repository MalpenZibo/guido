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
use crate::animation::{TimingFunction, Transition};
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
        // Event dispatch runs inside a snapshot zone in the real loop
        // (`render_surface`, lib.rs): hit testing reads animated values for
        // *this* event and must not subscribe to them. Outside the zone those
        // reads trip the non-reactive-read diagnostic, which would be the
        // harness reporting on itself.
        crate::reactive::diagnostics::snapshot_zone(|| {
            self.tree
                .with_widget_mut(root, |w, id, t| w.event(t, id, &event))
                .expect("root is registered")
        })
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

/// The width and colour of every border drawn, in paint order.
fn borders(node: &RenderNode) -> Vec<(f32, Color)> {
    node.commands
        .iter()
        .filter_map(|c| match &**c {
            DrawCommand::RoundedRect {
                border: Some(border),
                ..
            } => Some((border.width, border.color)),
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

/// Advancing animations reads each animated property's target. That read is a
/// snapshot — the subscription is established by `seed_animations` at the first
/// layout and refreshed by `resync_animation_targets` at every paint — so the
/// debug diagnostic must stay quiet about it.
///
/// It did not, and reported every animated container's border width and corner
/// radius as a value that "will not update", which is the opposite of true. A
/// warning that cries wolf on correct code is worse than no warning: it teaches
/// people to scroll past the ones that are right.
///
/// Both halves are asserted together on purpose — silence is only correct if
/// the property really is subscribed.
#[cfg(debug_assertions)]
#[test]
fn advancing_animations_does_not_report_a_missing_scope() {
    use crate::animation::{TimingFunction, Transition};
    use crate::reactive::diagnostics::report_count;

    let t = || Transition::new(200, TimingFunction::EaseOut);
    let wide = create_signal(false);
    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .background(Color::RED)
            .border(move || if wide.get() { 6.0 } else { 2.0 }, Color::BLUE)
            .corner_radius(move || if wide.get() { 20.0 } else { 4.0 })
            .animate_border_width(t())
            .animate_corner_radius(t()),
    );
    h.fit(200.0, 200.0);
    h.paint();

    let before = report_count();
    let root = h.root;
    h.tree.with_widget_mut(root, |w, id, tree| {
        w.advance_animations(tree, id);
    });
    assert_eq!(
        report_count() - before,
        0,
        "advancing animations must not be reported as a non-reactive read"
    );

    let queued = h.jobs_from(|| wide.set(true));
    assert!(
        queued.contains(&JobType::Animation),
        "and the properties must genuinely be subscribed, got {queued:?}"
    );
}

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

// ---------------------------------------------------------------------------
// A state layer reaching the text
// ---------------------------------------------------------------------------

/// Run the queued jobs, returning whether any of them was an animation step.
///
/// Processing rather than discarding matters twice over: it is what turns a
/// flag write into `needs_paint` on the text that subscribed to it, and it is
/// what advances an animation on the widget that owns it. `advance_animations`
/// does not recurse — in the real loop the job carries the widget id — so a
/// test that calls it on the root never touches an animated child.
fn pump(h: &mut H) -> bool {
    let roots = h.roots();
    jobs::distribute_jobs(&h.tree, &roots);
    let drained = jobs::drain_surface_jobs(h.root);
    let animating = drained.iter().any(|job| job.job_type == JobType::Animation);
    let mut layout_roots = Vec::new();
    jobs::process_jobs(&drained, &mut h.tree, &mut layout_roots);
    jobs::recycle_job_buffer(drained);
    jobs::recycle_job_buffer(jobs::drain_orphan_jobs());
    animating
}

/// Lay out, run the queued jobs, paint, and report the first text colour.
fn painted_text_color(h: &mut H) -> Color {
    pump(h);
    h.fit(400.0, 400.0);
    let node = h.paint();

    fn find(node: &RenderNode) -> Option<Color> {
        for cmd in &node.commands {
            if let DrawCommand::Text { color, .. } = &**cmd {
                return Some(*color);
            }
        }
        node.children.iter().find_map(|child| find(child))
    }
    find(&node).expect("a text command")
}

fn set_hover(h: &mut H, inside: bool) {
    let (x, y) = if inside { (5.0, 5.0) } else { (-50.0, -50.0) };
    h.send(Event::MouseMove { x, y });
}

/// The failure this guards against is cache-shaped and silent: the container
/// repaints with its new background while the text's cached render node is
/// reused with the old colour still inside its draw commands.
#[test]
fn a_hover_state_reaches_the_text() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::rgb(0.5, 0.5, 0.5))
            .hover_state(|s| s.text_color(Color::WHITE))
            .child(crate::widgets::text("Label")),
    );

    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);
    set_hover(&mut h, false);
    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
}

#[test]
fn a_hover_state_reaches_text_below_a_plain_container() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::rgb(0.5, 0.5, 0.5))
            .hover_state(|s| s.text_color(Color::WHITE))
            .child(
                container()
                    .layout(Flex::row())
                    .child(crate::widgets::text("Label")),
            ),
    );

    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);
}

/// The hovered container declares no colour of its own, so releasing the hover
/// has to land on what an ancestor said. That base is resolved by a walk done
/// once at registration — a derived closure has no tree to walk.
#[test]
fn a_state_colour_falls_back_to_the_inherited_base() {
    let mut h = H::new(
        container().text_color(Color::RED).child(
            container()
                .width(100.0)
                .height(40.0)
                .hover_state(|s| s.text_color(Color::WHITE))
                .child(crate::widgets::text("Label")),
        ),
    );

    assert_eq!(painted_text_color(&mut h), Color::RED);
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);
    set_hover(&mut h, false);
    assert_eq!(painted_text_color(&mut h), Color::RED);
}

#[test]
fn a_nearer_declaration_wins_over_an_outer_hover() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::rgb(0.5, 0.5, 0.5))
            .hover_state(|s| s.text_color(Color::WHITE))
            .child(
                container()
                    .text_color(Color::BLUE)
                    .child(crate::widgets::text("Label")),
            ),
    );

    painted_text_color(&mut h);
    set_hover(&mut h, true);
    assert_eq!(
        painted_text_color(&mut h),
        Color::BLUE,
        "a text told its own colour must not follow someone else's hover"
    );
}

/// Nothing is created when no state layer mentions text, so a hover cannot
/// disturb the published colour at all.
#[test]
fn a_container_with_no_state_text_colour_publishes_the_base_signal() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::RED)
            .hover_state(|s| s.lighter(0.1))
            .child(crate::widgets::text("Label")),
    );

    painted_text_color(&mut h);
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::RED);
}

// ---------------------------------------------------------------------------
// The published derived must not outlive its container
// ---------------------------------------------------------------------------

/// The one reactive resource a container creates outside any user scope.
///
/// Everything else is built in the builder chain and freed with the caller's
/// scope. The published derived is created at *registration*, where the
/// ambient owner is the surface's and outlives any single container — so
/// without the explicit teardown in `Drop`, a dynamic-children update that
/// replaces this container leaks one derived per rebuild.
///
/// The shape below is what makes this test able to fail: the builder runs in
/// a short-lived scope that is disposed each round, while registration happens
/// under a long-lived one that is not. Building and dropping inside a single
/// scope would free the derived as a side effect of freeing its parent, and
/// the test would pass with the teardown removed.
#[test]
fn the_published_text_derived_is_freed_with_its_container() {
    use crate::reactive::owner::{dispose_owner_now, with_owner};
    use crate::reactive::storage::live_signal_count;

    fn round() {
        // Builder signals belong to this scope, as a component's would.
        let (widget, item) = with_owner(|| {
            container()
                .text_color(Color::RED)
                .hover_state(|s| s.text_color(Color::WHITE))
                .child(crate::widgets::text("x"))
        });
        // Registration happens under the surrounding (surface) owner.
        let mut h = H::new(widget);
        h.fit(100.0, 100.0);
        drop(h);
        dispose_owner_now(item);
    }

    let (counts, surface) = with_owner(|| {
        round(); // warm whatever is allocated lazily
        let before = live_signal_count();
        for _ in 0..50 {
            round();
        }
        (before, live_signal_count())
    });
    dispose_owner_now(surface);

    let (before, after) = counts;
    assert_eq!(
        after,
        before,
        "50 build-and-drop rounds leaked {} signals",
        after as i64 - before as i64
    );
}

// ---------------------------------------------------------------------------
// Focus, now that it is stored state
// ---------------------------------------------------------------------------

/// Focus used to be a bare generational id, which made this self-correcting:
/// a dead widget stopped matching any live one and `focused_state` resolved to
/// false on its own. The focus *path* is stored state and has no such
/// property — its ancestors would go on answering "the focus is inside me" for
/// a widget that no longer exists.
#[test]
fn unregistering_a_focused_widget_releases_the_focus() {
    use crate::reactive::{focus_path, request_focus};

    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .child(box_of(10.0, 10.0)),
    );
    h.fit(100.0, 100.0);
    let child = h.children()[0];

    request_focus(&h.tree, child);
    assert!(focus_path().contains(child));
    assert!(
        focus_path().contains(h.root),
        "the ancestor is on the path, which is what focused_state asks about"
    );

    h.tree.unregister(child);
    assert!(
        !focus_path().contains(h.root),
        "an ancestor must stop claiming focus once the focused widget is gone"
    );
}

/// A container resolves `focused_state` by asking the path, not by walking its
/// descendants — that is what lets the same question be answered from inside a
/// derived closure, which has no tree.
#[test]
fn a_focused_state_applies_while_a_descendant_holds_focus() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;

    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .background(Color::RED)
            .focused_state(|s| s.background(Color::BLUE))
            .child(box_of(10.0, 10.0)),
    );
    h.fit(100.0, 100.0);
    let child = h.children()[0];

    assert_eq!(rects(&h.paint())[0].1, Color::RED);
    request_focus(&h.tree, child);
    assert_eq!(rects(&h.paint())[0].1, Color::BLUE);
    clear_focus();
    assert_eq!(rects(&h.paint())[0].1, Color::RED);
}

/// A state layer *overrides* the base; it does not rank against it.
///
/// Which is the whole answer to "why would a field not use `focused_state` for
/// its focus ring". A border that already carries a meaning — an error colour
/// here — puts that meaning in the base, and the base is exactly what the layer
/// replaces. On a field that holds the focus essentially all the time, that is
/// an error colour that never appears.
#[test]
fn a_focused_state_replaces_the_base_rather_than_ranking_against_it() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;
    use crate::widgets::text_input;

    clear_focus();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .border(1.5, Color::RED)
            .focused_state(|s| s.border(1.5, Color::BLUE))
            .child(text_input(create_signal(String::new()))),
    );
    h.fit(100.0, 100.0);
    let input = h.children()[0];

    assert_eq!(borders(&h.paint())[0].1, Color::RED);

    request_focus(&h.tree, input);
    assert_eq!(
        borders(&h.paint())[0].1,
        Color::BLUE,
        "the layer wins, and the base has no way to say it should not"
    );
}

/// The other way to follow the focus, for when it has to rank *below* something:
/// a colour that asks a handle instead of a state layer that overrides.
///
/// It works because the closure is read inside paint's tracking scope — asking
/// about the focus is what subscribes the container to it, with no
/// `focused_state` declared anywhere.
#[test]
fn a_border_colour_that_asks_a_handle_follows_the_focus() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;
    use crate::widget_ref::create_widget_ref;
    use crate::widgets::text_input;

    clear_focus();
    let handle = create_widget_ref();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .border(2.0, move || {
                if handle.is_focused() {
                    Color::BLUE
                } else {
                    Color::RED
                }
            })
            .child(text_input(create_signal(String::new())).widget_ref(handle)),
    );
    h.fit(100.0, 100.0);
    let input = h.children()[0];

    assert_eq!(borders(&h.paint())[0], (2.0, Color::RED));

    request_focus(&h.tree, input);
    assert_eq!(borders(&h.paint())[0], (2.0, Color::BLUE));

    // And the container has to *hear* about a focus change. Nothing else would
    // repaint a border whose colour is a closure: the two repaints
    // `request_focus` asks for name the input and the widget losing the focus,
    // neither of which is this container.
    let queued = h.jobs_from(clear_focus);
    assert!(
        queued.contains(&JobType::Paint),
        "asking about the focus has to subscribe to it, got {queued:?}"
    );
    assert_eq!(borders(&h.paint())[0], (2.0, Color::RED));
}

/// A state override holds a signal, so it follows a theme the way the base does.
///
/// Before this it held a plain `Color`, taken once when the builder ran. A
/// themed button tracked the theme through `background(..)` and stopped
/// tracking it the moment the pointer arrived, because the layer covering the
/// base had no way to say it depended on anything.
#[test]
fn a_state_override_follows_the_signal_it_was_given() {
    let accent = create_signal(Color::RED);
    let mut h = H::new(
        box_of(50.0, 50.0)
            .background(Color::BLACK)
            .hover_state(move |s: StateStyle| s.background(accent)),
    );
    h.fit(100.0, 100.0);

    set_hover(&mut h, true);
    assert_eq!(rects(&h.paint())[0].1, Color::RED);

    // Reading the override is what subscribes to it: nothing else would tell
    // this container that a colour it draws has moved.
    let queued = h.jobs_from(|| accent.set(Color::BLUE));
    assert!(
        queued.contains(&JobType::Paint),
        "the active override has to subscribe, got {queued:?}"
    );
    assert_eq!(rects(&h.paint())[0].1, Color::BLUE);
}

/// A state the app owns needs no mechanism: it is a signal, read where the
/// style is resolved.
#[test]
fn a_state_layer_may_hang_on_a_condition_the_app_owns() {
    let wrong = create_signal(false);
    let mut h = H::new(
        box_of(50.0, 50.0)
            .background(Color::BLACK)
            .state(wrong, |s| s.background(Color::RED)),
    );
    h.fit(100.0, 100.0);

    assert_eq!(rects(&h.paint())[0].1, Color::BLACK);

    let queued = h.jobs_from(|| wrong.set(true));
    assert!(
        queued.contains(&JobType::Paint),
        "the condition has to subscribe, got {queued:?}"
    );
    assert_eq!(rects(&h.paint())[0].1, Color::RED);
}

/// The answer to the field whose focus ring hid its error: declaration order
/// decides, so the error written last outranks the focus.
#[test]
fn the_last_layer_declared_wins_over_the_ones_before_it() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;
    use crate::widgets::text_input;

    clear_focus();
    let wrong = create_signal(false);
    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .border(1.5, Color::WHITE)
            .focused_state(|s| s.border(1.5, Color::BLUE))
            .state(wrong, |s| s.border(1.5, Color::RED))
            .child(text_input(create_signal(String::new()))),
    );
    h.fit(100.0, 100.0);
    let input = h.children()[0];

    request_focus(&h.tree, input);
    assert_eq!(borders(&h.paint())[0].1, Color::BLUE);

    wrong.set(true);
    assert_eq!(
        borders(&h.paint())[0].1,
        Color::RED,
        "declared after the focused layer, so it outranks it"
    );

    // And it is the order, not the kind: the focus is still held.
    wrong.set(false);
    assert_eq!(borders(&h.paint())[0].1, Color::BLUE);
}

/// A layer is passed over for a property it says nothing about, rather than
/// ending the search — otherwise a pressed layer that only scales would drop
/// the hover's background on the way down.
#[test]
fn a_layer_silent_on_a_property_does_not_shadow_the_one_below_it() {
    let mut h = H::new(
        box_of(50.0, 50.0)
            .background(Color::BLACK)
            .hover_state(|s| s.background(Color::RED))
            .pressed_state(|s| s.transform(Transform::scale(0.5))),
    );
    h.fit(100.0, 100.0);

    set_hover(&mut h, true);
    h.send(Event::MouseDown {
        x: 5.0,
        y: 5.0,
        button: MouseButton::Left,
    });
    assert_eq!(rects(&h.paint())[0].1, Color::RED);
}

// ---------------------------------------------------------------------------
// An animated text colour
// ---------------------------------------------------------------------------

/// Run frames until nothing is animating, or `limit` frames pass.
///
/// Animations step against the wall clock, so a tight loop would advance them
/// by microseconds and never arrive — the frames have to take real time.
fn settle(h: &mut H, limit: usize) -> usize {
    for frame in 0..limit {
        std::thread::sleep(std::time::Duration::from_millis(4));
        let animating = pump(h);
        h.fit(400.0, 400.0);
        h.paint();
        if !animating {
            return frame;
        }
    }
    limit
}

/// One frame: let time pass, run the jobs, and report what the text ended up.
fn frame(h: &mut H) -> Color {
    std::thread::sleep(std::time::Duration::from_millis(8));
    painted_text_color(h)
}

/// The in-flight value has to reach a widget that is not the one animating.
/// Every other animated property is consumed by the paint of the container
/// that owns it; this one is drawn by a descendant, so each step leaves
/// through a signal — and if that write did not happen, the text would simply
/// jump to the final colour on the first frame while the box eased.
#[test]
fn an_animated_text_colour_passes_through_intermediate_values() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::BLACK)
            .hover_state(|s| s.text_color(Color::WHITE))
            .animate_text_color(Transition::new(80.0, TimingFunction::Linear))
            .child(crate::widgets::text("Label")),
    );

    assert_eq!(painted_text_color(&mut h), Color::BLACK);

    set_hover(&mut h, true);
    let mid = frame(&mut h);
    assert!(
        mid != Color::BLACK && mid != Color::WHITE,
        "expected a value between the two, got {mid:?}"
    );

    settle(&mut h, 80);
    assert_eq!(
        painted_text_color(&mut h),
        Color::WHITE,
        "and it has to arrive"
    );
}

/// Once settled the derived goes back to the ordinary fold, so the animation
/// is not left pinning its last frame.
#[test]
fn a_settled_animation_releases_the_colour_back_to_the_fold() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::BLACK)
            .hover_state(|s| s.text_color(Color::WHITE))
            .animate_text_color(Transition::new(80.0, TimingFunction::Linear))
            .child(crate::widgets::text("Label")),
    );

    painted_text_color(&mut h);
    set_hover(&mut h, true);
    settle(&mut h, 80);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);

    set_hover(&mut h, false);
    settle(&mut h, 80);
    assert_eq!(painted_text_color(&mut h), Color::BLACK);
}

/// The animation has to start from the colour a descendant would actually have
/// shown — which, for a container with no colour of its own, is the inherited
/// one.
///
/// Two notions of "base" have to agree: the one the published derived folds
/// (declared, else inherited) and the one the animation is seeded from and
/// aims at. Where they diverge the transition departs from somewhere the text
/// never was, so hovering flashes through a third colour on the way.
#[test]
fn an_animated_colour_starts_from_the_inherited_base() {
    let mut h = H::new(
        container().text_color(Color::RED).child(
            container()
                .width(100.0)
                .height(40.0)
                .hover_state(|s| s.text_color(Color::BLUE))
                .animate_text_color(Transition::new(80.0, TimingFunction::Linear))
                .child(crate::widgets::text("Label")),
        ),
    );

    assert_eq!(painted_text_color(&mut h), Color::RED);

    set_hover(&mut h, true);
    let first = frame(&mut h);
    assert!(
        first.r > first.b,
        "the transition must leave from the inherited red, got {first:?}"
    );

    settle(&mut h, 80);
    assert_eq!(painted_text_color(&mut h), Color::BLUE);

    set_hover(&mut h, false);
    settle(&mut h, 80);
    assert_eq!(
        painted_text_color(&mut h),
        Color::RED,
        "and come back to it"
    );
}
