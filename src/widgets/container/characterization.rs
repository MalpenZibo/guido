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
use crate::animation::{SpringConfig, TimingFunction, Transition};
use crate::backdrop::BackdropSources;
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

    /// One frame, in the phases and the order the loop runs them: queued jobs,
    /// layout, paint, flatten, the compositor blur region read off the result,
    /// and `crate::cache_paint_results` to finish.
    ///
    /// That last one is the real function, not a stand-in for the half of it a
    /// test happened to need. Writing `clear_needs_paint` by hand instead left
    /// `cache_paint` undone, so `reuse_cached` never found a cache and every
    /// test repainted everything — which made a whole class of bug, the one
    /// about a widget that is *not* repainted, structurally invisible here.
    ///
    /// Use this, not `paint()`, for anything whose correctness is about when in
    /// the frame it happens, or about which widgets painted at all.
    fn frame(&mut self, w: f32, h: f32) -> Frame {
        pump(self);
        self.fit(w, h);

        let root = self.root;
        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });

        let node = std::rc::Rc::new(node);
        let mut commands = Vec::new();
        let mut layers = Vec::new();
        crate::renderer::flatten_root_into(&node, &mut commands, &mut layers);
        let blur = crate::blur::regions_from_commands(&commands);

        crate::cache_paint_results(&mut self.tree, &node);

        Frame { node, blur }
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

/// What one frame produced: the render tree, and the compositor blur region the
/// paint left registered.
struct Frame {
    node: std::rc::Rc<RenderNode>,
    blur: Vec<crate::blur::BlurRect>,
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
fn a_hover_layer_reaches_the_text() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::rgb(0.5, 0.5, 0.5))
            .when_hovered(|s| s.text_color(Color::WHITE))
            .child(crate::widgets::text("Label")),
    );

    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);
    set_hover(&mut h, false);
    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
}

#[test]
fn a_hover_layer_reaches_text_below_a_plain_container() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .text_color(Color::rgb(0.5, 0.5, 0.5))
            .when_hovered(|s| s.text_color(Color::WHITE))
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
                .when_hovered(|s| s.text_color(Color::WHITE))
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
            .when_hovered(|s| s.text_color(Color::WHITE))
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
            .when_hovered(|s| s.lighter(0.1))
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
                .when_hovered(|s| s.text_color(Color::WHITE))
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
// Keyframes
// ---------------------------------------------------------------------------

/// A timeline plays because a signal the caller owns changed, so the container
/// has to be subscribed to it — reading the trigger where the targets are read
/// is what does that, and without it a shake would wait for the next frame
/// somebody else asked for.
#[test]
fn a_container_wakes_when_its_timeline_is_asked_to_play() {
    use crate::animation::Keyframes;

    let plays = create_signal(0u32);
    let mut h = H::new(
        box_of(50.0, 50.0).keyframes_transform(
            Keyframes::new(200.0)
                .at(0.0, Transform::IDENTITY)
                .at(0.5, Transform::rotate_degrees(2.0))
                .at(1.0, Transform::IDENTITY),
            plays,
        ),
    );
    h.fit(100.0, 100.0);
    h.paint();

    let queued = h.jobs_from(|| plays.set(1));
    assert!(
        queued.contains(&JobType::Animation),
        "a play has to wake the container that would show it, got {queued:?}"
    );
}

/// The transform this container is painted with, once the queued jobs have
/// run. A rotation shows up as a matrix that is no longer the identity.
fn played_transform(h: &mut H) -> Transform {
    pump(h);
    h.fit(200.0, 200.0);
    h.paint().children[0].local_transform
}

/// Waking is not playing. The test above passes whether or not the sequence
/// survived, because the job comes from the trigger — so this one asks the
/// only question that matters: did the property move?
#[test]
fn a_played_sequence_actually_moves_the_transform() {
    use crate::animation::Keyframes;

    let plays = create_signal(0u32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0).keyframes_transform(
                Keyframes::new(200.0)
                    .at(0.0, Transform::IDENTITY)
                    .at(0.5, Transform::rotate_degrees(20.0))
                    .at(1.0, Transform::IDENTITY),
                plays,
            ),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();

    plays.set(1);
    let played = played_transform(&mut h);
    assert_ne!(
        played,
        Transform::IDENTITY,
        "the sequence has to reach the paint, not only the job queue"
    );
}

/// And it survives the transition being declared after it.
///
/// `animate_transform` builds a fresh `AnimationState`, so the sequence used
/// to be thrown away by whichever builder came second — with the trigger
/// still firing, the job still queued and nothing at all to show for it.
#[test]
fn declaring_a_transition_after_a_sequence_does_not_lose_it() {
    use crate::animation::Keyframes;

    let plays = create_signal(0u32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0)
                .keyframes_transform(
                    Keyframes::new(200.0)
                        .at(0.0, Transform::IDENTITY)
                        .at(0.5, Transform::rotate_degrees(20.0))
                        .at(1.0, Transform::IDENTITY),
                    plays,
                )
                // Written after, which used to decide the whole thing.
                .animate_transform(Transition::new(100.0, TimingFunction::EaseOut)),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();

    plays.set(1);
    assert_ne!(
        played_transform(&mut h),
        Transform::IDENTITY,
        "the order the two builders were written in must not decide whether \
         the sequence exists"
    );
}

// ---------------------------------------------------------------------------
// Focus, now that it is stored state
// ---------------------------------------------------------------------------

/// Focus used to be a bare generational id, which made this self-correcting:
/// a dead widget stopped matching any live one and `when_focused` resolved to
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
        "the ancestor is on the path, which is what a focused layer asks about"
    );

    h.tree.unregister(child);
    assert!(
        !focus_path().contains(h.root),
        "an ancestor must stop claiming focus once the focused widget is gone"
    );
}

/// A container resolves `when_focused` by asking the path, not by walking its
/// descendants — that is what lets the same question be answered from inside a
/// derived closure, which has no tree.
#[test]
fn a_focused_layer_applies_while_a_descendant_holds_focus() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;

    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .background(Color::RED)
            .when_focused(|s| s.background(Color::BLUE))
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
/// Which is the whole answer to "why would a field not use `when_focused` for
/// its focus ring". A border that already carries a meaning — an error colour
/// here — puts that meaning in the base, and the base is exactly what the layer
/// replaces. On a field that holds the focus essentially all the time, that is
/// an error colour that never appears.
#[test]
fn a_focused_layer_replaces_the_base_rather_than_ranking_against_it() {
    use crate::reactive::focus::clear_focus;
    use crate::reactive::request_focus;
    use crate::widgets::text_input;

    clear_focus();
    let mut h = H::new(
        container()
            .width(50.0)
            .height(50.0)
            .border(1.5, Color::RED)
            .when_focused(|s| s.border(1.5, Color::BLUE))
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
/// `when_focused` declared anywhere.
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
            .when_hovered(move |s: StateStyle| s.background(accent)),
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
            .when_focused(|s| s.border(1.5, Color::BLUE))
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
            .when_hovered(|s| s.background(Color::RED))
            .when_pressed(|s| s.transform(Transform::scale(0.5))),
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
            .when_hovered(|s| s.text_color(Color::WHITE))
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
            .when_hovered(|s| s.text_color(Color::WHITE))
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
                .when_hovered(|s| s.text_color(Color::BLUE))
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

// ---------------------------------------------------------------------------
// Reactivity — properties that used to accept only a constant
// ---------------------------------------------------------------------------

/// A gradient follows its signal, and does so as a repaint: nothing about the
/// fill can move the box.
#[test]
fn a_gradient_is_reactive_and_paint_only() {
    let warm = create_signal(true);
    let mut h = H::new(container().width(20.0).height(20.0).gradient(move || {
        Some(if warm.get() {
            LinearGradient::horizontal(Color::RED, Color::YELLOW)
        } else {
            LinearGradient::horizontal(Color::BLUE, Color::CYAN)
        })
    }));
    h.fit(100.0, 100.0);
    assert_eq!(gradient_ends(&h.paint()), Some((Color::RED, Color::YELLOW)));

    let queued = h.jobs_from(|| warm.set(false));
    assert_eq!(queued, vec![JobType::Paint], "got {queued:?}");
    assert_eq!(gradient_ends(&h.paint()), Some((Color::BLUE, Color::CYAN)));
}

/// Two constant endpoints must not cost a derived that boxes a closure and
/// recomputes, every read, a value that cannot change.
#[test]
fn a_constant_gradient_shorthand_stays_constant() {
    let constant = container().gradient_vertical(Color::RED, Color::BLUE);
    assert_eq!(
        constant.gradient.and_then(|g| g.constant()).flatten(),
        Some(LinearGradient::vertical(Color::RED, Color::BLUE)),
        "two constants make one constant"
    );

    let end = create_signal(Color::GREEN);
    let reactive = container().gradient_vertical(Color::RED, end);
    assert!(
        reactive.gradient.expect("declared").constant().is_none(),
        "a reactive endpoint has to stay reactive"
    );
}

/// The endpoints are reactive on their own, so the common case needs no
/// closure building a whole gradient.
#[test]
fn gradient_endpoints_are_reactive_one_by_one() {
    let end = create_signal(Color::YELLOW);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .gradient_vertical(Color::RED, end),
    );
    h.fit(100.0, 100.0);
    assert_eq!(gradient_ends(&h.paint()), Some((Color::RED, Color::YELLOW)));

    end.set(Color::GREEN);
    assert_eq!(gradient_ends(&h.paint()), Some((Color::RED, Color::GREEN)));
}

/// `overflow` decides both whether children are clipped and whether the box may
/// shrink below its content, so a write to it has to reach layout as well as
/// paint.
#[test]
fn overflow_is_reactive_and_invalidates_layout() {
    let clipped = create_signal(false);
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .overflow(move || {
                if clipped.get() {
                    Overflow::Hidden
                } else {
                    Overflow::Visible
                }
            })
            .child(box_of(400.0, 10.0)),
    );
    h.fit(500.0, 500.0);
    assert!(h.paint().clip.is_none());

    let queued = h.jobs_from(|| clipped.set(true));
    assert!(
        queued.contains(&JobType::Layout) && queued.contains(&JobType::Paint),
        "got {queued:?}"
    );
    h.fit(500.0, 500.0);
    assert!(h.paint().clip.is_some());
}

/// A blur can be switched off by the same signal that switches it on: a radius
/// of zero draws no blur command, which is the contract `Text` already had.
#[test]
fn a_backdrop_blur_is_reactive_and_zero_means_off() {
    let radius = create_signal(0.0f32);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .backdrop_blur(move || radius.get()),
    );
    h.fit(100.0, 100.0);
    assert_eq!(blur_radii(&h.paint()), Vec::<f32>::new());

    let queued = h.jobs_from(|| radius.set(12.0));
    assert_eq!(queued, vec![JobType::Paint], "got {queued:?}");
    assert_eq!(blur_radii(&h.paint()), vec![12.0]);

    radius.set(0.0);
    assert_eq!(blur_radii(&h.paint()), Vec::<f32>::new());
}

/// A state layer replaces the whole border, both halves at once, because that is
/// the only way to say "border" anywhere in this API.
#[test]
fn a_state_layer_replaces_the_whole_border() {
    let danger = create_signal(false);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .border(1.0, Color::GRAY)
            .state(danger, |s| s.border(2.0, Color::RED)),
    );
    h.fit(100.0, 100.0);
    assert_eq!(borders(&h.paint()), vec![(1.0, Color::GRAY)]);

    danger.set(true);
    assert_eq!(borders(&h.paint()), vec![(2.0, Color::RED)]);

    danger.set(false);
    assert_eq!(borders(&h.paint()), vec![(1.0, Color::GRAY)], "and back");
}

/// Two layers speaking about the border resolve last-declared-wins, as every
/// other property does — and as a unit, since half of one cannot be declared.
#[test]
fn the_last_declared_border_layer_wins() {
    let hovered = create_signal(true);
    let failed = create_signal(true);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .border(1.0, Color::GRAY)
            .state(hovered, |s| s.border(2.0, Color::WHITE))
            .state(failed, |s| s.border(3.0, Color::RED)),
    );
    h.fit(100.0, 100.0);
    assert_eq!(borders(&h.paint()), vec![(3.0, Color::RED)]);

    failed.set(false);
    assert_eq!(borders(&h.paint()), vec![(2.0, Color::WHITE)]);
}

/// Elevation transitions like every other paint property now, instead of
/// jumping.
#[test]
fn elevation_animates_towards_its_state_layer() {
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(0.0)
            .when_hovered(|s| s.elevation(8.0))
            .animate_elevation(Transition::new(80.0, TimingFunction::Linear)),
    );
    h.fit(100.0, 100.0);
    h.paint();

    assert_eq!(shadow_count(&h.paint()), 0, "flat on the surface at rest");

    set_hover(&mut h, true);
    std::thread::sleep(std::time::Duration::from_millis(20));
    pump(&mut h);
    h.fit(100.0, 100.0);
    assert_eq!(
        shadow_count(&h.paint()),
        1,
        "it casts a shadow while it rises"
    );

    settle(&mut h, 80);
    set_hover(&mut h, false);
    settle(&mut h, 80);
    assert_eq!(shadow_count(&h.paint()), 0, "and settles back down");
}

/// Every `BackdropBlur` radius drawn by a node.
fn blur_radii(node: &RenderNode) -> Vec<f32> {
    node.commands
        .iter()
        .filter_map(|c| match &**c {
            DrawCommand::BackdropBlur { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect()
}

/// The endpoints of the first gradient drawn by a node.
fn gradient_ends(node: &RenderNode) -> Option<(Color, Color)> {
    node.commands.iter().find_map(|c| match &**c {
        DrawCommand::RoundedRect {
            gradient: Some(gradient),
            ..
        } => Some((gradient.start_color, gradient.end_color)),
        _ => None,
    })
}

/// How many of the node's rects carry a shadow.
fn shadow_count(node: &RenderNode) -> usize {
    node.commands
        .iter()
        .filter(|c| {
            matches!(
                &***c,
                DrawCommand::RoundedRect {
                    shadow: Some(_),
                    ..
                }
            )
        })
        .count()
}

/// A shadow belongs to the box, not to the fill, so a gradient must not lose
/// it. Both are reactive, so a signal can move a container from one branch of
/// the decoration to the other between frames — an elevation animation that
/// crossed into the gradient branch stopped drawing while still asking for a
/// frame at every step.
#[test]
fn a_gradient_keeps_its_shadow() {
    let mut h = H::new(
        container()
            .width(40.0)
            .height(20.0)
            .elevation(3.0)
            .gradient_horizontal(Color::RED, Color::BLUE),
    );
    h.fit(100.0, 100.0);
    let node = h.paint();

    assert_eq!(gradient_ends(&node), Some((Color::RED, Color::BLUE)));
    assert_eq!(shadow_count(&node), 1, "the gradient is still elevated");
}

/// A border resolving to nothing visible must draw nothing, rather than send an
/// invisible frame to the instance buffer every frame.
///
/// Every border names both halves, so this is only reachable deliberately — a
/// transparent colour, a zero width — or in passing, while an animated colour
/// crosses transparent.
#[test]
fn a_border_with_nothing_to_show_draws_nothing() {
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .border(2.0, Color::TRANSPARENT),
    );
    h.fit(100.0, 100.0);
    assert_eq!(borders(&h.paint()), Vec::new(), "an invisible colour");

    let mut h = H::new(container().width(20.0).height(20.0).border(0.0, Color::RED));
    h.fit(100.0, 100.0);
    assert_eq!(borders(&h.paint()), Vec::new(), "a zero width");
}

/// A colour fading in behind a declared width has to start drawing the moment it
/// is visible, so the gate cannot be a build-time decision.
#[test]
fn a_border_appears_when_its_colour_does() {
    let visible = create_signal(false);
    let mut h = H::new(container().width(20.0).height(20.0).border(2.0, move || {
        if visible.get() {
            Color::RED
        } else {
            Color::TRANSPARENT
        }
    }));
    h.fit(100.0, 100.0);
    assert_eq!(borders(&h.paint()), Vec::new());

    visible.set(true);
    assert_eq!(borders(&h.paint()), vec![(2.0, Color::RED)]);
}

/// A clipped child is invisible, and an invisible child must not answer for a
/// click landing outside its parent's bounds.
///
/// Content overflows by *summing*: each child answers to the parent's maximum,
/// their row does not. So the second pill sits beyond the parent's right edge.
fn clip_host(clipped: Signal<bool>, clicked: RwSignal<i32>) -> Container {
    container()
        .width(40.0)
        .height(20.0)
        .layout(Flex::row())
        .overflow(move || {
            if clipped.get() {
                Overflow::Hidden
            } else {
                Overflow::Visible
            }
        })
        .child(box_of(40.0, 20.0))
        .child(
            container()
                .width(40.0)
                .height(20.0)
                .on_click(move || clicked.update(|c| *c += 1)),
        )
}

#[test]
fn hidden_overflow_stops_events_reaching_a_clipped_child() {
    let clicked = create_signal(0);
    let clipped = create_signal(true);
    let mut h = H::new(clip_host(clipped.into(), clicked));
    h.fit(500.0, 500.0);
    h.paint();

    // x = 60 is inside the second pill and outside the clipping parent.
    for event in click_at(60.0, 10.0) {
        h.send(event);
    }
    assert_eq!(clicked.get_untracked(), 0, "clipped away, so not clickable");

    clipped.set(false);
    pump(&mut h);
    h.fit(500.0, 500.0);
    h.paint();

    for event in click_at(60.0, 10.0) {
        h.send(event);
    }
    assert_eq!(clicked.get_untracked(), 1, "unclipped, so clickable");
}

/// Events resolve against the frame on screen, not against the frame about to
/// be drawn: the pointer is aimed at what the user can see. `hit.bounds` already
/// come from the last layout, and the clip has to agree with them — which is
/// also why the event path reads a cached value rather than running the
/// container's own closure once per container per coalesced MouseMove.
#[test]
fn events_use_the_clip_of_the_frame_on_screen() {
    let clicked = create_signal(0);
    let clipped = create_signal(false);
    let mut h = H::new(clip_host(clipped.into(), clicked));
    h.fit(500.0, 500.0);
    h.paint();

    // The signal flips, but nothing has been laid out or drawn since.
    clipped.set(true);
    for event in click_at(60.0, 10.0) {
        h.send(event);
    }
    assert_eq!(
        clicked.get_untracked(),
        1,
        "the pill the user can see is still clickable"
    );

    // Once the clip is on screen, it applies.
    pump(&mut h);
    h.fit(500.0, 500.0);
    h.paint();
    for event in click_at(60.0, 10.0) {
        h.send(event);
    }
    assert_eq!(clicked.get_untracked(), 1);
}

/// A shadow falls outside the box that casts it, so the damage a container
/// reports has to reach past its own bounds — and it has to reach far enough for
/// the *largest* shadow it can cast, not the one showing.
///
/// Elevation animates paint-only, so a hover that lifts a card never re-runs the
/// layout that records the reach. Sized to the resting value, the shadow ring
/// falls outside every damage rect: absent on the way up, left on screen on the
/// way down.
#[test]
fn the_damage_reach_covers_the_elevation_a_hover_can_reach() {
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(0.0)
            .when_hovered(|s| s.elevation(8.0))
            .animate_elevation(Transition::new(80.0, TimingFunction::Linear)),
    );
    h.fit(100.0, 100.0);

    let reach = h.tree.paint_overflow(h.root);
    let lifted = style::elevation_to_shadow(8.0).extent();
    assert!(
        reach >= lifted && lifted > 0.0,
        "reach {reach} must cover the hovered shadow {lifted}"
    );

    // And a container that can never lift pays nothing for it.
    let mut flat = H::new(container().width(40.0).height(40.0).background(Color::RED));
    flat.fit(100.0, 100.0);
    assert_eq!(flat.tree.paint_overflow(flat.root), 0.0);
}

/// A spring goes past its target on purpose, so a reach measured from the target
/// leaves the shadow ring outside the damage rect exactly at the peak — the same
/// artefact the reach exists to prevent, moved to the moment it is most visible.
#[test]
fn the_damage_reach_allows_for_a_spring_overshooting() {
    let bouncy = |c: Container| {
        c.width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(0.0)
            .when_hovered(|s| s.elevation(8.0))
    };

    let mut eased = H::new(
        bouncy(container()).animate_elevation(Transition::new(80.0, TimingFunction::Linear)),
    );
    eased.fit(100.0, 100.0);
    let without_bounce = eased.tree.paint_overflow(eased.root);

    let mut sprung =
        H::new(bouncy(container()).animate_elevation(Transition::spring(SpringConfig::BOUNCY)));
    sprung.fit(100.0, 100.0);
    let with_bounce = sprung.tree.paint_overflow(sprung.root);

    assert!(
        with_bounce > without_bounce,
        "a bouncy spring has to reach further than an ease: {with_bounce} vs {without_bounce}"
    );
    assert!(
        with_bounce >= style::elevation_to_shadow(8.0 * 1.17).extent() - 0.01,
        "and far enough for BOUNCY's ~17%, got {with_bounce}"
    );
}

/// A declared elevation changing does need a new reach, so it must invalidate
/// the layout that recorded the old one.
#[test]
fn a_declared_elevation_change_invalidates_the_reach() {
    let level = create_signal(0.0f32);
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(move || level.get()),
    );
    h.fit(100.0, 100.0);
    h.paint();
    assert_eq!(h.tree.paint_overflow(h.root), 0.0);

    let queued = h.jobs_from(|| level.set(6.0));
    assert!(queued.contains(&JobType::Layout), "got {queued:?}");

    // `jobs_from` consumes the jobs without running them, so the reach is
    // recomputed by a write that gets pumped.
    level.set(9.0);
    pump(&mut h);
    h.fit(100.0, 100.0);
    assert!(h.tree.paint_overflow(h.root) > 0.0);
}

// ---------------------------------------------------------------------------
// Compositor blur regions — every one of these is about *when* in the frame
// ---------------------------------------------------------------------------
//
// The surface half of a backdrop blur is a draw command and shows up in the
// render tree. The compositor half is a region handed to `wl_region`, published
// once per frame, and it is where the interesting failure lives: a region is
// registered and withdrawn *during* paint, so anything that reads it must read
// it after. These drive `H::frame`, which calls the same `crate::paint_surface`
// the loop does, precisely so they cannot attest an order the loop lacks.

/// Turning a blur off has to reach the compositor on the frame that turns it
/// off. Collecting before the paint published the region the *previous* frame
/// asked for, and then nothing was dirty any more — so the withdrawal waited for
/// a frame that never came and the desktop stayed blurred for the life of the
/// surface.
#[test]
fn switching_a_compositor_blur_off_withdraws_it_on_that_frame() {
    let frosted = create_signal(true);
    let mut h = H::new(
        container()
            .width(40.0)
            .height(20.0)
            .backdrop_blur(move || if frosted.get() { 24.0 } else { 0.0 }),
    );
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "frosted");

    frosted.set(false);
    assert_eq!(
        h.frame(100.0, 100.0).blur,
        Vec::new(),
        "the frame that turns it off is the frame that stops publishing it"
    );

    // And a settled surface keeps it off: nothing is dirty, so this frame does
    // not repaint, and the region must not come back.
    assert_eq!(h.frame(100.0, 100.0).blur, Vec::new());

    frosted.set(true);
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "and back on");
}

/// Restricting the sources to the surface publishes no compositor region at all,
/// whatever the radius — while still drawing one.
#[test]
fn a_surface_only_blur_publishes_no_compositor_region() {
    let mut h = H::new(
        container()
            .width(40.0)
            .height(20.0)
            .backdrop_blur(BackdropBlur::new(24.0).sources(BackdropSources::SURFACE)),
    );
    let frame = h.frame(100.0, 100.0);
    assert_eq!(frame.blur, Vec::new());
    assert_eq!(blur_radii(&frame.node), vec![24.0], "but it draws one");
}

/// A hidden panel blurs nothing, and neither does anything inside it. The
/// parent's paint returns at the visibility gate before its children are
/// painted, so a blurred *descendant* never gets to withdraw its own region —
/// and layout returning zero for the parent leaves the child's own bounds
/// exactly where they were.
#[test]
fn hiding_a_panel_withdraws_the_blur_of_everything_inside_it() {
    let open = create_signal(true);
    let mut h = H::new(
        container()
            .width(60.0)
            .height(40.0)
            .visible(move || open.get())
            .child(container().width(40.0).height(20.0).backdrop_blur(24.0)),
    );
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "open");

    open.set(false);
    assert_eq!(
        h.frame(100.0, 100.0).blur,
        Vec::new(),
        "a hidden panel blurs nothing, descendants included"
    );
    assert_eq!(h.frame(100.0, 100.0).blur, Vec::new(), "and stays that way");

    open.set(true);
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "and back");
}

/// Hiding and showing a panel has to work in *both* directions, and the return
/// trip is the one that breaks: the panel repaints, but its child is clean and
/// is served from the paint cache, so the child's own paint never runs.
///
/// While the region was a registry written during paint, that meant the blur
/// went off and never came back — the withdrawal had a path and the
/// re-registration did not. Reading the region off the frame instead makes the
/// two directions one operation: a cached node carries its commands with it, so
/// it carries its blur.
#[test]
fn a_blur_served_from_the_paint_cache_is_still_published() {
    let open = create_signal(true);
    let mut h = H::new(
        container()
            .width(60.0)
            .height(40.0)
            .visible(move || open.get())
            .child(container().width(40.0).height(20.0).backdrop_blur(24.0)),
    );
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "open");

    open.set(false);
    assert_eq!(h.frame(100.0, 100.0).blur, Vec::new(), "hidden");

    // The panel is dirty and repaints; the child is not, and comes from the
    // cache without its paint running.
    open.set(true);
    assert!(
        !h.frame(100.0, 100.0).blur.is_empty(),
        "a blur that came back from the cache still has to be published"
    );

    // And it survives a frame in which nothing at all repaints.
    assert!(!h.frame(100.0, 100.0).blur.is_empty(), "and stays");
}

/// A child scrolled out of a viewport blurs nothing. It never paints — culled,
/// or outside the window the scroller even offers to `paint_children` — so it
/// cannot withdraw its own region, and its bounds stay where they were.
#[test]
fn scrolling_a_frosted_card_away_withdraws_its_blur() {
    // Enough rows that the scroller takes its windowing fast path, which is the
    // case the sweep exists for and the one a two-child list never reaches.
    let mut rows: Vec<crate::widgets::AnyWidget> = vec![
        container()
            .width(40.0)
            .height(20.0)
            .backdrop_blur(24.0)
            .into_any(),
    ];
    rows.extend((0..60).map(|_| box_of(40.0, 20.0).into_any()));

    let mut h = H::new(
        container()
            .width(40.0)
            .height(30.0)
            .scrollable(ScrollAxis::Vertical)
            .children(rows),
    );
    assert!(
        !h.frame(40.0, 30.0).blur.is_empty(),
        "visible at the top of the scroll"
    );

    h.send(Event::Scroll {
        x: 20.0,
        y: 15.0,
        delta_x: 0.0,
        delta_y: 600.0,
        source: ScrollSource::Wheel,
    });
    // Two frames: the first repaints the scroller and settles the flags, the
    // second is where a clean card is culled — or never offered at all.
    h.frame(40.0, 30.0);
    assert_eq!(
        h.frame(40.0, 30.0).blur,
        Vec::new(),
        "scrolled away, so it blurs nothing"
    );
}

/// A gradient needs a value that means "no gradient", or the rule the whole
/// branch is about — a property you can turn off with the signal that turned it
/// on — has a hole exactly where a header expands and collapses.
#[test]
fn a_gradient_can_be_switched_off_and_the_background_returns() {
    let expanded = create_signal(true);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .background(Color::GRAY)
            .gradient(move || {
                expanded
                    .get()
                    .then(|| LinearGradient::horizontal(Color::RED, Color::BLUE))
            }),
    );
    h.fit(100.0, 100.0);
    assert_eq!(gradient_ends(&h.paint()), Some((Color::RED, Color::BLUE)));

    expanded.set(false);
    let node = h.paint();
    assert_eq!(gradient_ends(&node), None, "no gradient");
    assert_eq!(
        rects(&node).first().map(|(_, color)| *color),
        Some(Color::GRAY),
        "and the background it was covering is drawn again"
    );
}
