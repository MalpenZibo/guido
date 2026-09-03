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
use crate::animation::{Animate, Keyframes, SpringConfig, TimingFunction, Transition};
use crate::backdrop::BackdropSources;
use crate::jobs::{self, JobType};
use crate::layout::{Constraints, Flex, at_least, at_most, fill, fraction};
use crate::reactive::create_signal;
use crate::renderer::{DrawCommand, PaintContext, RenderNode};
use crate::widgets::CornerRadii;
use crate::widgets::TextStyled;
use crate::widgets::widget::{Event, EventResponse, MouseButton};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct H {
    tree: Tree,
    root: WidgetId,
    /// The last frame's render tree and flattened commands, retained as the
    /// loop retains them — a frame that repaints nothing publishes from these.
    last: Option<(
        std::rc::Rc<RenderNode>,
        Vec<crate::renderer::FlattenedCommand>,
    )>,
}

impl H {
    fn new(widget: impl Widget + 'static) -> Self {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        Self {
            tree,
            root,
            last: None,
        }
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

    /// One frame, in the phases and the order `render_surface` runs them: queued
    /// jobs, layout, the skip-frame gate, paint, flatten, the compositor blur
    /// region read off the result, and `crate::cache_paint_results` to finish.
    ///
    /// Two of those are the loop's own code rather than a stand-in for it, and
    /// both were learned the hard way — and it is called the way the loop calls
    /// it, per child of the root, not on the root. `cache_paint_results` written out by hand
    /// left `cache_paint` undone, so `reuse_cached` never found a cache and every
    /// test repainted everything — making the whole class of bug about a widget
    /// that is *not* repainted structurally invisible here. And a `frame` that
    /// always painted could not reach the gate at all, where the loop publishes
    /// the region from the buffer it retained.
    ///
    /// Use this, not `paint()`, for anything whose correctness is about when in
    /// the frame it happens, or about which widgets painted at all.
    fn frame(&mut self, w: f32, h: f32) -> Frame {
        pump(self);
        self.fit(w, h);

        let root = self.root;
        if !self.tree.needs_paint(root)
            && let Some((node, commands)) = self.last.clone()
        {
            // The gate: nothing is dirty, so nothing repaints and the region
            // comes from the frame still on screen.
            let blur = crate::blur::regions_from_commands(&commands);
            return Frame { node, blur };
        }

        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });

        let node = std::rc::Rc::new(node);
        let mut commands = Vec::new();
        let mut layers = Vec::new();
        let _ = crate::renderer::flatten_root_into(&node, &mut commands, &mut layers);
        let blur = crate::blur::regions_from_commands(&commands);

        // Per child of the root and then `clear_needs_paint(root)`, which is
        // what the loop does: the surface root always repaints, so it is never
        // cache-reused, and handing it to `cache_paint_results` would leave the
        // harness with a cache entry the loop never has.
        for child in &node.children {
            crate::cache_paint_results(&mut self.tree, child);
        }
        self.tree.clear_needs_paint(root);
        self.last = Some((std::rc::Rc::clone(&node), commands));

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

/// A state layer that only scales must not make the container resolve a
/// translate and a rotate nothing declares. The fast path is per component,
/// and this is the layer that would have paid for it — it is on every button.
#[test]
fn a_scaling_state_layer_declares_only_a_scale() {
    let c = container()
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.scale(0.98));
    let declared = c
        .interaction
        .as_ref()
        .map(|ix| ix.declares_transform)
        .unwrap_or_default();

    assert!(declared.scale, "the pressed layer names a scale");
    assert!(!declared.translate, "and nothing names a translate");
    assert!(!declared.rotate, "or a rotate");
}

/// And the other two mirrors, because `moves_anything` answers for the three
/// components in a list of its own: one that named the wrong field would put a
/// container back on the slow path, or leave it on the fast one while a layer
/// moved it.
#[test]
fn a_translating_or_rotating_layer_declares_only_its_own_component() {
    let declares = |c: &Container| {
        c.interaction
            .as_ref()
            .map(|ix| ix.declares_transform)
            .unwrap_or_default()
    };

    let translating =
        declares(&container().when_hovered(|s| s.translate(Translate::new(0.0, -2.0))));
    assert!(translating.translate, "the hovered layer names a translate");
    assert!(
        !translating.rotate && !translating.scale,
        "and nothing else"
    );

    let rotating = declares(&container().when_pressed(|s| s.rotate(2.0)));
    assert!(rotating.rotate, "the pressed layer names a rotate");
    assert!(!rotating.translate && !rotating.scale, "and nothing else");
}

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
        let outer = if animated {
            container()
                .width(at_most(60.0).transition(Transition::new(200, TimingFunction::EaseOut)))
        } else {
            container().width(at_most(60.0))
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
            .width(at_most(100.0).transition(Transition::new(200, TimingFunction::EaseOut)))
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

/// The colour and corner radii of every rounded rect a paint produced.
fn painted_rects_with_style(node: &RenderNode) -> Vec<(Color, CornerRadii)> {
    fn walk(node: &RenderNode, out: &mut Vec<(Color, CornerRadii)>) {
        for cmd in &node.commands {
            if let DrawCommand::RoundedRect { color, radius, .. } = &**cmd {
                out.push((*color, *radius));
            }
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

/// A scroller with a handle the test can find by its colour.
fn scroller(scroll: Scroll) -> H {
    H::new(
        container()
            .height(100.0)
            .scroll(scroll)
            .child(box_of(40.0, 900.0)),
    )
}

/// The scrollbar is styled as what it is — two containers — so a signal reaches
/// it like it reaches anything else.
///
/// This is the whole point of the change. `ScrollbarConfig` held plain colours
/// captured at build time, so a theme switch restyled every surface except its
/// scrollbars.
#[test]
fn a_handle_colour_declared_with_a_signal_follows_it() {
    let hot = create_signal(Color::rgb(1.0, 0.0, 0.0));
    let mut h = scroller(Scroll::vertical().handle(move |c: Container| c.background(hot)));
    h.fit(500.0, 500.0);

    let before = painted_rects_with_style(&h.paint());
    assert!(
        before.iter().any(|(c, _)| c.r > 0.9 && c.g < 0.1),
        "the declared handle colour has to reach the paint: {before:?}"
    );

    hot.set(Color::rgb(0.0, 0.0, 1.0));
    pump(&mut h);
    h.fit(500.0, 500.0);

    let after = painted_rects_with_style(&h.paint());
    assert!(
        after.iter().any(|(c, _)| c.b > 0.9 && c.r < 0.1),
        "the handle kept the colour it was built with: {after:?}"
    );
}

/// And it takes corners the old pair could not spell.
///
/// `ScrollbarConfig` carried a radius and a K-value, which reaches
/// `Corners::superellipse` and nothing else. A bevel, a scoop or a per-corner
/// radius were all expressible downstream and unreachable from the API.
#[test]
fn a_handle_takes_corners_the_old_pair_could_not_spell() {
    let mut h = scroller(Scroll::vertical().handle(|c: Container| {
        c.background(Color::rgb(1.0, 0.0, 0.0))
            .corners([8.0, 0.0, 8.0, 0.0])
    }));
    h.fit(500.0, 500.0);

    let (_, radii) = painted_rects_with_style(&h.paint())
        .into_iter()
        .find(|(c, _)| c.r > 0.9 && c.g < 0.1)
        .expect("the handle, found by its declared colour");

    assert!(
        radii.top_left > 7.9 && radii.top_right < 0.1,
        "a per-corner radius has to survive to paint, got {radii:?}"
    );
}

/// A scrollbar that comes back after being hidden.
///
/// Starting Hidden is the half worth testing, and the half the agreed design
/// chose a property over a `.hidden()` flag for — "a flag can only be set, and
/// this has to be able to come back". A container laid out Hidden returns from
/// `ensure_scrollbar_containers` before registering the track and the handle,
/// so the flip back has to create them *and* lay them out in one pass; and it
/// only gets that pass because `resolve_scroll` reads the visibility under
/// layout tracking, which is what marks the container and keeps it off the
/// unchanged-constraints fast path.
///
/// That the content still scrolls while the bar is hidden is not re-asserted
/// here — `Hidden` gates only the drawing, and the scroll tests in
/// `tests/scroll_momentum.rs` and `tests/scrollbar_handle_tracking.rs` own that
/// question.
#[test]
fn visibility_answers_to_a_signal() {
    let shown = create_signal(ScrollbarVisibility::Hidden);
    let mut h = scroller(Scroll::vertical().visibility(shown));
    h.fit(500.0, 500.0);
    let hidden_children = h.paint().children.len();

    shown.set(ScrollbarVisibility::Always);
    pump(&mut h);
    h.fit(500.0, 500.0);
    let shown_children = h.paint().children.len();

    assert!(
        shown_children > hidden_children,
        "the scrollbar never came back: {hidden_children} children hidden, \
         {shown_children} shown"
    );

    shown.set(ScrollbarVisibility::Hidden);
    pump(&mut h);
    h.fit(500.0, 500.0);
    assert_eq!(
        h.paint().children.len(),
        hidden_children,
        "and it goes away again"
    );
}

/// Whether a scrollbar exists at all comes down to one comparison — content
/// against viewport — so the viewport a scroller records has to be its
/// *visible* extent, never the unbounded constraint the scrolled axis is laid
/// out with. An infinite viewport is never exceeded, and the scrollbar
/// silently stops being drawn. This asserts on the paint output because that
/// is exactly what "the scrollbars stopped appearing" means.
#[test]
fn an_overflowing_scroller_paints_its_scrollbar() {
    let mut h = scroller(Scroll::vertical());
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
            .scroll(Scroll::horizontal())
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
            .scroll(Scroll::vertical())
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
            .scroll(Scroll::vertical())
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
        Event::mouse_down(x, y, MouseButton::Left),
        Event::mouse_up(x, y, MouseButton::Left),
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

/// The other two buttons, which fire on the press rather than on a matched
/// release: there is no drag to complete, so there is nothing to wait for.
///
/// Nothing tested these at all until a mutant said so — killing the guard that
/// separates them from the left button left the whole suite green, which is the
/// same as saying the two handlers could stop firing and nobody would hear.
#[test]
fn the_right_and_middle_buttons_fire_the_handlers_that_name_them() {
    let right = std::rc::Rc::new(std::cell::Cell::new(0));
    let middle = std::rc::Rc::new(std::cell::Cell::new(0));
    let (r, m) = (right.clone(), middle.clone());
    let mut h = H::new(
        container()
            .width(50.0)
            .height(20.0)
            .on_right_click(move || r.set(r.get() + 1))
            .on_middle_click(move || m.set(m.get() + 1)),
    );
    h.fit(500.0, 500.0);

    h.send(Event::mouse_down(25.0, 10.0, MouseButton::Right));
    assert_eq!((right.get(), middle.get()), (1, 0), "a right press");

    h.send(Event::mouse_down(25.0, 10.0, MouseButton::Middle));
    assert_eq!((right.get(), middle.get()), (1, 1), "a middle press");

    // The left button is neither of them, and reaches neither handler.
    h.send(Event::mouse_down(25.0, 10.0, MouseButton::Left));
    assert_eq!(
        (right.get(), middle.get()),
        (1, 1),
        "a left press belongs to on_click, and must not reach either"
    );

    h.send(Event::mouse_down(200.0, 10.0, MouseButton::Right));
    assert_eq!(right.get(), 1, "a right press outside must not fire");
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
            .corners(20.0)
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

    h.send(Event::mouse_move(25.0, 10.0));
    assert!(state.get());

    h.send(Event::mouse_move(300.0, 10.0));
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

    h.send(Event::scroll(
        25.0,
        10.0,
        0.0,
        12.0,
        crate::widgets::widget::ScrollSource::Wheel,
    ));
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
            .border(
                (move || if wide.get() { 6.0 } else { 2.0 }).transition(t()),
                Color::BLUE,
            )
            .corners((move || if wide.get() { 20.0 } else { 4.0 }).transition(t())),
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

/// A state layer's border moves the *width* as well as the colour, because a
/// border is declared as a pair — so "can a state change move an animated
/// property" has to answer yes for a container that animates only the width.
///
/// It answered no: the list named `border_color` and not `border_width`, from
/// when the two were declared separately and a layer really could change one
/// alone. This PR added `elevation` to that list and left the hole next to it
/// that it had just made reachable.
///
/// Asked of the predicate rather than through a hover, because a hover reaches
/// the animation by a second route as well: the interaction flags are a signal,
/// and paint reads the animated width under `JobType::Animation` tracking, so
/// setting the flag queues the job anyway. That route is why nothing was visibly
/// broken — and it is not a reason to leave the direct answer wrong, since it
/// holds only while some paint has already subscribed.
#[test]
fn a_border_width_animation_counts_as_movable_by_a_state_layer() {
    let animated = container()
        .border(
            2.0.transition(Transition::spring(SpringConfig::BOUNCY)),
            Color::BLUE,
        )
        .when_hovered(|s| s.border(14.0, Color::BLUE));
    assert!(
        animated.has_animated_state_properties(),
        "hovering moves the width, so it needs an Animation job"
    );

    let plain = container()
        .border(2.0, Color::BLUE)
        .when_hovered(|s| s.border(14.0, Color::BLUE));
    assert!(
        !plain.has_animated_state_properties(),
        "with nothing animated, a plain repaint is the whole job"
    );
}

/// The same question for the three transform components, which answer it from
/// a list of their own.
#[test]
fn a_state_layer_moving_an_animated_transform_counts_as_movable() {
    let t = || Transition::new(80.0, TimingFunction::Linear);

    assert!(
        container()
            .translate(Translate::NONE.transition(t()))
            .when_hovered(|s| s.translate(Translate::new(0.0, -2.0)))
            .has_animated_state_properties(),
        "hovering moves the translate, so it needs an Animation job"
    );
    assert!(
        container()
            .rotate(0.0.transition(t()))
            .when_hovered(|s| s.rotate(2.0))
            .has_animated_state_properties(),
        "and a rotate"
    );
    assert!(
        container()
            .scale(Scale::NONE.transition(t()))
            .when_pressed(|s| s.scale(0.98))
            .has_animated_state_properties(),
        "and a scale — the layer on every button there is"
    );
    assert!(
        !container()
            .when_pressed(|s| s.scale(0.98))
            .has_animated_state_properties(),
        "and with nothing animated, a plain repaint is the whole job"
    );
}

/// Every animated transform component holds a copy of signal-derived state, so
/// every one of them needs the paint-time target re-sync. Width does not: its
/// target follows the measured content and is recomputed at each layout.
///
/// Asked of the predicate rather than through a moving container, for the
/// reason the border-width test above gives and one more of its own.
/// `seed_animations` subscribes to the *condition* of a branching closure at
/// the first layout, so flipping that condition queues an Animation job by that
/// route and the container converges even with the re-sync gone. The
/// behavioural route cannot see this omission; the predicate can.
#[test]
fn each_animated_transform_component_is_a_signal_animated_prop() {
    let t = || Transition::new(80.0, TimingFunction::Linear);

    assert!(
        container()
            .translate(Translate::NONE.transition(t()))
            .has_signal_animated_props(),
        "a translate animation mirrors a signal, so it re-syncs at paint"
    );
    assert!(
        container()
            .rotate(0.0.transition(t()))
            .has_signal_animated_props(),
        "and a rotate"
    );
    assert!(
        container()
            .scale(Scale::NONE.transition(t()))
            .has_signal_animated_props(),
        "and a scale"
    );
    assert!(
        !container()
            .width(0.0.transition(t()))
            .has_signal_animated_props(),
        "a width follows the content it measured, and is retargeted at layout"
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
            .corners(move || r.get()),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| r.set(6.0));
    assert!(queued.contains(&JobType::Paint), "got {queued:?}");
    assert!(!queued.contains(&JobType::Layout), "got {queued:?}");
}

/// A transform is a paint property and moving a widget must not reflow it —
/// not the widget, not its siblings, not the page. That has to hold even now
/// that a parent asks how far a transform can carry a child before deciding
/// whether to paint it: the answer is refreshed from the Paint job the same
/// write schedules, in `refresh_paint_bounds`, and never from layout.
///
/// Each of the three separately, because they are declared apart and a reach
/// read under layout tracking for one of them would be invisible in the others.
#[test]
fn a_transform_invalidates_paint_only() {
    for (name, build) in [
        (
            "translate",
            Box::new(|s: RwSignal<f32>| {
                container()
                    .width(10.0)
                    .height(10.0)
                    .translate(move || Translate::new(s.get(), 0.0))
            }) as Box<dyn Fn(RwSignal<f32>) -> Container>,
        ),
        (
            "rotate",
            Box::new(|s: RwSignal<f32>| {
                container().width(10.0).height(10.0).rotate(move || s.get())
            }),
        ),
        (
            "scale",
            Box::new(|s: RwSignal<f32>| {
                container()
                    .width(10.0)
                    .height(10.0)
                    .scale(move || Scale::uniform(1.0 + s.get()))
            }),
        ),
    ] {
        let value = create_signal(0.0f32);
        let mut h = H::new(build(value));
        h.fit(500.0, 500.0);
        h.paint();

        let queued = h.jobs_from(|| value.set(30.0));
        assert!(
            queued.contains(&JobType::Paint),
            "{name}: a transform must repaint, got {queued:?}"
        );
        assert!(
            !queued.contains(&JobType::Layout),
            "{name}: a transform must not reflow anything, got {queued:?}"
        );
    }
}

/// And the same, for a container that has a parent.
///
/// A child lays out inside its parent's tracking scope, so a read that does not
/// suspend tracking registers against *the parent* — the transformed container
/// stays quiet and its parent reflows instead. The test above cannot see it:
/// its container is the root, where there is no outer scope to catch the read.
#[test]
fn a_transform_does_not_reflow_the_parent_either() {
    let dx = create_signal(0.0f32);
    let mut h = H::new(
        container().layout(Flex::column()).child(
            container()
                .width(10.0)
                .height(10.0)
                .translate(move || Translate::new(dx.get(), 0.0)),
        ),
    );
    h.fit(500.0, 500.0);
    h.paint();

    let queued = h.jobs_from(|| dx.set(30.0));
    assert!(
        !queued.contains(&JobType::Layout),
        "a child's transform relaid out its parent, got {queued:?}"
    );
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

/// A whole frame that declares what time it is: jobs, then layout, as the loop
/// runs them.
///
/// `pump` asks the clock, so two frames are however far apart the machine
/// happened to make them; this one names the instant, so a curve can be
/// asserted at a value instead of inside a band.
fn frame_at(h: &mut H, now: std::time::Instant, width: f32, height: f32) {
    h.tree.set_frame_instant(Some(now));
    pump(h);
    h.fit(width, height);
    h.tree.set_frame_instant(None);
}

/// Halfway through a linear hundred milliseconds is halfway to the target —
/// exactly, on any machine, with nothing asleep.
///
/// The value of naming the instant is that this asserts the *curve*, which is
/// the half of an animation nothing outside `animations.rs`, `keyframes.rs`
/// and `timing.rs` can see. Every other test here watches where an animation
/// lands, and an easing that is wrong, a delay that is ignored, a reverse
/// transition used in place of the forward one and a property that skips the
/// animation entirely all land in the same place.
#[test]
fn a_linear_animation_is_halfway_at_half_its_duration() {
    let w = create_signal(0.0f32);
    let mut h = H::new(
        container()
            .height(10.0)
            .width(w.transition(Transition::new(100.0, TimingFunction::Linear))),
    );
    h.fit(400.0, 400.0);

    let t0 = std::time::Instant::now();
    w.set(100.0);
    frame_at(&mut h, t0, 400.0, 400.0);
    assert_eq!(
        h.tree.cached_size(h.root).unwrap().width,
        0.0,
        "the segment begins where it was, not where it is going"
    );

    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(50),
        400.0,
        400.0,
    );
    assert_eq!(
        h.tree.cached_size(h.root).unwrap().width,
        50.0,
        "half the duration of a linear curve is half the distance"
    );
}

/// The radius of the ripple drawn by this frame, if one is drawn.
fn painted_ripple_radius(h: &mut H) -> Option<f32> {
    h.paint()
        .overlay_commands
        .iter()
        .find_map(|cmd| match &**cmd {
            DrawCommand::Circle { radius, .. } => Some(*radius),
            _ => None,
        })
}

/// One frame is one instant: the ripple and the declared transition in the
/// same container are asked about the same moment.
///
/// They could not be before. The ripple already took an `Instant` and the
/// caller handed it `Instant::now()`, freshly read, some microseconds after
/// every animation beside it had read its own — so a frame was several
/// instants, and none of them was one a test could name.
#[test]
fn a_ripple_and_a_transition_are_asked_about_the_same_moment() {
    // Wide enough at rest to be clicked: the press has to land on the box.
    let w = create_signal(20.0f32);
    let mut h = H::new(
        container()
            .height(100.0)
            .width(w.transition(Transition::new(100.0, TimingFunction::Linear)))
            .when_pressed(|s| s.ripple())
            .on_click(|| {}),
    );
    h.fit(400.0, 400.0);

    h.send(Event::mouse_down(10.0, 50.0, MouseButton::Left));
    w.set(120.0);

    // After the press, because a ripple begins at the instant its event
    // arrived and that clock is the event's, not the frame's — a separate
    // question, and not this one.
    let t0 = std::time::Instant::now();

    frame_at(&mut h, t0, 400.0, 400.0);
    let started = painted_ripple_radius(&mut h).expect("the press starts a ripple");

    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(50),
        400.0,
        400.0,
    );
    assert_eq!(
        h.tree.cached_size(h.root).unwrap().width,
        70.0,
        "the transition is halfway: 20 plus half of the hundred it travels"
    );
    let grown = painted_ripple_radius(&mut h).expect("and the ripple is still drawn");
    assert!(
        grown > started,
        "the ripple grew over the same 50ms, from {started} to {grown}"
    );

    // The same instant twice: nothing moves, because nothing has.
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(50),
        400.0,
        400.0,
    );
    assert_eq!(
        painted_ripple_radius(&mut h),
        Some(grown),
        "asked about the same moment, a frame answers the same"
    );
}

/// An event delivered at a named moment, the way `dispatch_events` delivers
/// one.
fn send_at(h: &mut H, at: std::time::Instant, event: Event) -> EventResponse {
    h.tree.set_event_instant(Some(at));
    let response = h.send(event);
    h.tree.set_event_instant(None);
    response
}

/// A ripple held for a named length of time has grown by a named amount.
///
/// It could not be asked before. A ripple begins when the press arrives and
/// grows from there, and both ends of that were `Instant::now()` — one read in
/// the event handler and one in the frame — so a test could hold a press only
/// by sleeping, and assert only that something had happened.
#[test]
fn a_press_held_for_a_named_time_has_grown_by_a_named_amount() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .when_pressed(|s| s.ripple())
            .on_click(|| {}),
    );
    h.fit(400.0, 400.0);

    // A minute ago, and that is the point: if the press took its moment from the
    // clock instead of from the event, the disc would be a minute old by the
    // first frame and there would be nothing left to draw.
    let t0 = std::time::Instant::now() - std::time::Duration::from_secs(60);
    send_at(&mut h, t0, Event::mouse_down(50.0, 50.0, MouseButton::Left));

    // A frame later, because the press and the frame are now genuinely the same
    // instant: at t0 exactly nothing has elapsed and the disc has no opacity to
    // be drawn with.
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(8),
        400.0,
        400.0,
    );
    let started = painted_ripple_radius(&mut h).expect("the press starts a ripple");

    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(120),
        400.0,
        400.0,
    );
    let held = painted_ripple_radius(&mut h).expect("and holding it keeps one");

    // Asked about the same moment again, it answers the same: the disc is a
    // function of how long the press has lasted, and nothing else.
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(120),
        400.0,
        400.0,
    );
    assert_eq!(
        painted_ripple_radius(&mut h),
        Some(held),
        "the same instant twice is the same disc"
    );

    assert!(
        held > started,
        "a hundred and twenty milliseconds of press has to have grown it, \
         from {started} to {held}"
    );
}

/// One frame at a named instant, through the skip-frame gate.
///
/// `frame_at` pumps and lays out but leaves painting to the caller, so a test
/// that then calls `h.paint()` repaints unconditionally and cannot see a frame
/// the loop would have skipped. This one goes through `H::frame`, which serves
/// the retained node when nothing needs paint — which is the whole difference
/// between a value the loop noticed and one it did not.
fn gated_frame_at(h: &mut H, now: std::time::Instant, width: f32, height: f32) -> Frame {
    h.tree.set_frame_instant(Some(now));
    let frame = h.frame(width, height);
    h.tree.set_frame_instant(None);
    frame
}

/// The colour of the first overlay disc in a frame the loop actually produced.
fn ripple_color_of(frame: &Frame) -> Option<Color> {
    frame
        .node
        .overlay_commands
        .iter()
        .find_map(|cmd| match &**cmd {
            DrawCommand::Circle { color, .. } => Some(*color),
            _ => None,
        })
}

/// The colour of the first overlay disc, which is the ripple.
fn painted_ripple_color(h: &mut H) -> Option<Color> {
    h.paint()
        .overlay_commands
        .iter()
        .find_map(|cmd| match &**cmd {
            DrawCommand::Circle { color, .. } => Some(*color),
            _ => None,
        })
}

/// A ripple's colour is declared, so a theme switch reaches it.
///
/// The one colour that animates under a finger was the one frozen at build
/// time: `RippleConfig::color` was a `Color` where every other `StateStyle`
/// setter took a signal. Now it is a `Signal<Color>`, as `BorderOverride`'s
/// halves are, and the two speeds beside it stay plain.
///
/// Asserted on the drawn disc, and on its hue rather than its alpha, because
/// the alpha is multiplied by the ripple's own opacity and falls as the disc
/// fades — the hue is what the declaration decides.
///
/// The second half holds the press past its growth, which is the state the
/// declaration has to survive: a ripple that has finished growing stops asking
/// for frames so the loop can go quiet — see `Ripple::advance` — so the disc
/// under a still finger is not being repainted by an animation, and a colour
/// written then reaches the screen only because paint subscribed to it.
///
/// What this still does not pin is the subscription itself. Reading the colour
/// untracked passes here too, because `Tree::cache_layout` marks the widget for
/// paint on every layout pass — running layout *is* the invalidation signal —
/// so the skip-frame gate never comes up after a `fit` and each frame repaints
/// whatever the last write left. The read is tracked because the held case is
/// real, not because anything in `src/` would object if it stopped being.
#[test]
fn a_ripple_takes_the_colour_it_is_given_now() {
    let hot = create_signal(Color::rgba(1.0, 0.0, 0.0, 0.4));
    let mut h = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .when_pressed(move |s: StateStyle| s.ripple_with_color(hot))
            .on_click(|| {}),
    );
    h.fit(400.0, 400.0);

    let t0 = std::time::Instant::now() - std::time::Duration::from_secs(60);
    send_at(&mut h, t0, Event::mouse_down(50.0, 50.0, MouseButton::Left));
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(8),
        400.0,
        400.0,
    );

    let red = painted_ripple_color(&mut h).expect("the press starts a ripple");
    assert!(
        red.r > 0.9 && red.g < 0.1,
        "the declared colour is what the disc is drawn with, got {red:?}"
    );

    hot.set(Color::rgba(0.0, 0.0, 1.0, 0.4));
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(16),
        400.0,
        400.0,
    );

    let blue = painted_ripple_color(&mut h).expect("the ripple is still running");
    assert!(
        blue.b > 0.9 && blue.r < 0.1,
        "the ripple kept the colour it was built with, got {blue:?}"
    );

    // Now hold it past its growth, which is where the loop is allowed to go
    // quiet and where an unsubscribed read would strand the colour.
    let settled = t0 + std::time::Duration::from_secs(2);
    gated_frame_at(&mut h, settled, 400.0, 400.0);
    assert!(
        !pump(&mut h),
        "a ripple held past its growth has to stop asking for frames, or the \
         case this is about does not arise"
    );

    hot.set(Color::rgba(0.0, 1.0, 0.0, 0.4));
    eprintln!("needs_paint after write = {}", h.tree.needs_paint(h.root));
    let frame = gated_frame_at(
        &mut h,
        settled + std::time::Duration::from_millis(8),
        400.0,
        400.0,
    );

    eprintln!("frame colour = {:?}", ripple_color_of(&frame));
    let green = ripple_color_of(&frame).expect("the finger is still down");
    assert!(
        green.g > 0.9 && green.b < 0.1,
        "the disc under a still finger never saw the write, got {green:?}"
    );
    // The alpha is the declared one scaled by the disc's own opacity, which is
    // 1.0 for a ripple held at full growth — so here the two coincide and the
    // scaling is visible as itself rather than as some other arithmetic.
    assert!(
        (green.a - 0.4).abs() < 0.01,
        "the declared alpha is scaled by the disc's opacity, not combined with \
         it some other way: got {green:?}"
    );
}

/// The pressed layer's ripple is the one that paints, even when another layer
/// declares one too.
///
/// `ripple_config` asks for a layer that is *both* `Pressed` and has a ripple.
/// Loosen that to either and the search — which runs in reverse declaration
/// order — answers with whichever was declared last, so a hover ripple
/// declared after a pressed one would take over the press.
#[test]
fn a_hover_ripple_does_not_stand_in_for_the_pressed_one() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .when_pressed(|s: StateStyle| s.ripple_with_color(Color::rgba(1.0, 0.0, 0.0, 0.4)))
            .when_hovered(|s: StateStyle| s.ripple_with_color(Color::rgba(0.0, 0.0, 1.0, 0.4)))
            .on_click(|| {}),
    );
    h.fit(400.0, 400.0);

    let t0 = std::time::Instant::now() - std::time::Duration::from_secs(60);
    send_at(&mut h, t0, Event::mouse_down(50.0, 50.0, MouseButton::Left));
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(8),
        400.0,
        400.0,
    );

    let drawn = painted_ripple_color(&mut h).expect("the press starts a ripple");
    assert!(
        drawn.r > 0.9 && drawn.b < 0.1,
        "the hover layer's colour reached a press it does not describe: {drawn:?}"
    );
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
    h.send(Event::mouse_move(x, y));
}

/// The failure this guards against is cache-shaped and silent: the container
/// repaints with its new background while the text's cached render node is
/// reused with the old colour still inside its draw commands.
///
/// The hover belongs to the box and the colour to the glyphs, so each is
/// declared where it happens and `control()` joins them.
#[test]
fn a_hover_layer_reaches_the_text() {
    let mut h = H::new(
        container().width(100.0).height(40.0).control().child(
            crate::widgets::text("Label")
                .color(Color::rgb(0.5, 0.5, 0.5))
                .when_hovered(|s| s.color(Color::WHITE)),
        ),
    );

    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
    set_hover(&mut h, true);
    assert_eq!(painted_text_color(&mut h), Color::WHITE);
    set_hover(&mut h, false);
    assert_eq!(painted_text_color(&mut h), Color::rgb(0.5, 0.5, 0.5));
}

// ---------------------------------------------------------------------------
// The motion rides with the value
// ---------------------------------------------------------------------------

/// Two boxes differing in one thing only — whether the colour they are handed
/// carries a transition — and one write to the one signal both read.
///
/// This is the whole shape of the change in a single assertion: the timing is
/// not a second declaration naming a property, it is part of what the property
/// was set to, so the two containers can disagree about how they move while
/// agreeing about what they show.
#[test]
fn a_transition_on_the_value_is_what_makes_that_value_ease() {
    let color = create_signal(Color::BLACK);
    let mut h = H::new(
        container()
            .layout(Flex::row())
            .child(box_of(20.0, 20.0).background(color))
            .child(
                box_of(20.0, 20.0)
                    .background(color.transition(Transition::new(200.0, TimingFunction::Linear))),
            ),
    );
    h.fit(200.0, 200.0);
    h.paint();

    color.set(Color::WHITE);
    let t0 = std::time::Instant::now();
    frame_at(&mut h, t0, 200.0, 200.0);
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(100),
        200.0,
        200.0,
    );

    // The two boxes' backgrounds, in the order they were declared.
    let painted: Vec<Color> = h
        .paint()
        .children
        .iter()
        .map(|child| rects(child).first().expect("a background").1)
        .collect();
    assert_eq!(
        painted[0],
        Color::WHITE,
        "a value declared with no motion is at its new value on the next paint"
    );
    assert!(
        painted[1].r > 0.05 && painted[1].r < 0.95,
        "and one declared with a transition is still on its way, got {:?}",
        painted[1]
    );
}

/// A container that declares no motion carries no animation box.
///
/// `ContainerAnims` holds eleven `AnimationState`s and is boxed precisely so
/// that the overwhelming majority of containers — every one that only sets a
/// background — do not pay for it. Writing the absence of a motion is a real
/// write, so it has to skip the container that has nothing to write it into,
/// and nothing else can see the difference: the pixels are identical either
/// way, which is why this asks the field directly.
#[test]
fn declaring_no_motion_allocates_no_animation_box() {
    assert!(
        container().background(Color::RED).anims.is_none(),
        "a plain declaration must not allocate the animation box"
    );
    assert!(
        container()
            .background(Color::RED.transition(200.0))
            .anims
            .is_some(),
        "and one that declares a motion has somewhere to keep it"
    );
    assert!(
        container()
            .background(Color::RED.transition(200.0))
            .background(Color::BLUE)
            .anims
            .as_ref()
            .is_some_and(|a| a.background.is_none()),
        "restating it plainly empties the slot rather than the box"
    );
}

/// A declaration is the whole property, so restating one with a plain value
/// takes its motion away with it.
///
/// This is what makes `adopt_declarations_of` unnecessary — there are never
/// two declarations for one property to reconcile, only the one written last.
/// Left unwritten, the animation would outlive the declaration that asked for
/// it, which is the shape of the defect this whole change removes: a timing
/// still governing a property nobody declared it on.
#[test]
fn restating_a_property_plainly_takes_its_motion_with_it() {
    let color = create_signal(Color::BLACK);
    let mut h = H::new(
        container()
            .layout(Flex::row())
            .child(
                box_of(20.0, 20.0)
                    .background(color.transition(Transition::new(200.0, TimingFunction::Linear))),
            )
            // The same declaration, restated without one.
            .child(
                box_of(20.0, 20.0)
                    .background(color.transition(Transition::new(200.0, TimingFunction::Linear)))
                    .background(color),
            ),
    );
    h.fit(200.0, 200.0);
    h.paint();

    color.set(Color::WHITE);
    let t0 = std::time::Instant::now();
    frame_at(&mut h, t0, 200.0, 200.0);
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(100),
        200.0,
        200.0,
    );

    let painted: Vec<Color> = h
        .paint()
        .children
        .iter()
        .map(|child| rects(child).first().expect("a background").1)
        .collect();
    assert!(
        painted[0].r < 0.95,
        "the transition it was declared with still holds, got {:?}",
        painted[0]
    );
    assert_eq!(
        painted[1],
        Color::WHITE,
        "and the one restated plainly has no transition left to hold it back"
    );
}

/// And on a padding, which is the one newly-timeline-capable property on the
/// *layout* path rather than the paint path — so it is where "a sequence
/// speaks for its property while it plays" has to hold against
/// `update_size_targets` recomputing the declared value at every layout.
///
/// Asserted on the box the padding sizes rather than on a colour: a padding is
/// not drawn, it is the space a container takes around what it holds.
#[test]
fn a_timeline_plays_on_a_padding() {
    let plays = create_signal(0u32);
    let rest = Padding::all(0.0);
    let mut h = H::new(
        container()
            .padding(
                rest.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, rest)
                        .at(0.5, Padding::all(20.0))
                        .at(1.0, rest),
                    plays,
                ),
            )
            .child(box_of(20.0, 20.0)),
    );
    let at_rest = h.fit(200.0, 200.0);
    h.paint();
    assert_eq!(at_rest.width, 20.0, "nothing plays until the trigger moves");

    plays.set(1);
    let t0 = std::time::Instant::now();
    frame_at(&mut h, t0, 200.0, 200.0);
    h.tree
        .set_frame_instant(Some(t0 + std::time::Duration::from_millis(100)));
    pump(&mut h);
    let played = h.fit(200.0, 200.0);
    h.tree.set_frame_instant(None);

    assert!(
        played.width > at_rest.width + 5.0,
        "the sequence has to reach the layout the padding sizes, got {} \
         against {} at rest",
        played.width,
        at_rest.width
    );
}

/// A timeline is no longer transform-only: `Keyframes<T>` was always generic
/// and only the setters were not, so a background can flash where before it
/// could only be eased to.
#[test]
fn a_timeline_plays_on_a_background() {
    let plays = create_signal(0u32);
    let rest = Color::BLACK;
    let mut h = H::new(
        box_of(50.0, 50.0).background(
            rest.timeline(
                Keyframes::new(200.0)
                    .at(0.0, rest)
                    .at(0.5, Color::RED)
                    .at(1.0, rest),
                plays,
            ),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();
    assert_eq!(
        rects(&h.paint())[0].1,
        rest,
        "nothing plays until the trigger moves"
    );

    plays.set(1);
    let t0 = std::time::Instant::now();
    frame_at(&mut h, t0, 200.0, 200.0);
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(100),
        200.0,
        200.0,
    );

    let flashed = rects(&h.paint())[0].1;
    assert!(
        flashed.r > 0.5,
        "the sequence has to reach the pixels, got {flashed:?}"
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
    let plays = create_signal(0u32);
    let mut h = H::new(box_of(50.0, 50.0).rotate(0.0.timeline(
        Keyframes::new(200.0).at(0.0, 0.0).at(0.5, 2.0).at(1.0, 0.0),
        plays,
    )));
    h.fit(100.0, 100.0);
    h.paint();

    let queued = h.jobs_from(|| plays.set(1));
    assert!(
        queued.contains(&JobType::Animation),
        "a play has to wake the container that would show it, got {queued:?}"
    );
}

/// The transform a sequence has reached at the peak of its middle keyframe.
/// A rotation shows up as a matrix that is no longer the identity.
///
/// Two frames, because they are two different things: the first sees the
/// trigger move and starts the sequence, the second is 100ms later and is the
/// one with something to show. They used to be the same frame, and what made
/// that work was the few nanoseconds between `Instant::now()` in `play` and
/// `Instant::now()` inside `advance` — a gap this test was reading and nobody
/// had chosen.
fn played_transform(h: &mut H) -> Transform {
    let t0 = std::time::Instant::now();
    frame_at(h, t0, 200.0, 200.0);
    frame_at(h, t0 + std::time::Duration::from_millis(100), 200.0, 200.0);
    h.paint().children[0].local_transform
}

/// Waking is not playing. The test above passes whether or not the sequence
/// survived, because the job comes from the trigger — so this one asks the
/// only question that matters: did the property move?
#[test]
fn a_played_sequence_actually_moves_the_transform() {
    let plays = create_signal(0u32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0).rotate(
                0.0.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, 0.0)
                        .at(0.5, 20.0)
                        .at(1.0, 0.0),
                    plays,
                ),
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

/// A translate that animates has to arrive where its signal points.
///
/// The one component with no such test until now, and four separate lists name
/// it on the way: the target is only computed where `advance_animations` says
/// the component is animated, and resolved only where `animated_transform`
/// says something could move it.
#[test]
fn an_animated_translate_reaches_the_paint() {
    let offset = create_signal(Translate::NONE);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0)
                .translate(offset.transition(Transition::new(600.0, TimingFunction::Linear))),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();
    assert_eq!(
        h.paint().children[0].local_transform.tx(),
        0.0,
        "at rest it sits where it was laid out"
    );

    offset.set(Translate::new(40.0, 0.0));

    // Moving, and not arrived. Without this the test passes with the animation
    // deleted outright — `get_animated_value` falls back to the signal, and
    // paint lands on 40 the first time it is asked.
    //
    // A tenth of the way through a linear six hundred milliseconds is a tenth
    // of the distance, so this is a number rather than the band it used to be:
    // the band existed because the only "midway" available was however far the
    // clock had moved between two reads of it.
    let t0 = std::time::Instant::now();
    frame_at(&mut h, t0, 200.0, 200.0);
    frame_at(
        &mut h,
        t0 + std::time::Duration::from_millis(60),
        200.0,
        200.0,
    );
    let midway = h.paint().children[0].local_transform.tx();
    assert_eq!(
        midway, 4.0,
        "it has to animate towards the target rather than snap to it"
    );

    settle(&mut h, 400);
    let tx = h.paint().children[0].local_transform.tx();
    assert!(
        (tx - 40.0).abs() < 0.5,
        "the animation has to arrive at what the signal named, got {tx}"
    );
}

/// A target the widget could only have reached through a branch it was not
/// subscribed to when it first laid out.
///
/// `seed_animations` subscribes once, to whatever the closure read at the first
/// layout — here `armed` alone, because the other branch was not taken.
/// `advance_animations` reads in a `snapshot_zone` and subscribes to nothing.
/// So `resync_animation_targets` is the only pass that can ever pick up
/// `offset`, and the write below is the one nothing else can see: flipping
/// `armed` wakes the container by the subscription it already had, but the
/// write *after* that has only the re-sync between it and a stale value.
#[test]
fn a_translate_target_behind_an_untaken_branch_is_picked_up_by_the_resync() {
    let armed = create_signal(false);
    let offset = create_signal(0.0f32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0).translate(
                (move || {
                    if armed.get() {
                        Translate::new(offset.get(), 0.0)
                    } else {
                        Translate::NONE
                    }
                })
                .transition(Transition::new(80.0, TimingFunction::Linear)),
            ),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();

    // The branch flips. This much the first subscription can see, and the
    // paint it causes is the only chance anything has to notice `offset`.
    armed.set(true);
    settle(&mut h, 120);

    offset.set(40.0);
    settle(&mut h, 120);

    let tx = h.paint().children[0].local_transform.tx();
    assert!(
        (tx - 40.0).abs() < 0.5,
        "a target behind a branch nobody tracked still has to converge, got {tx}"
    );
}

/// The other two timelines, which `advance_animations` starts and
/// `resync_animation_targets` wakes from lists that name each component once.
/// Only a rotation was played by anything before this.
///
/// Neither sequence passes through the value that composes to the identity —
/// the displacement stays out at 10 and the pulse never comes back below 1.1 —
/// so neither assertion depends on how far the clock moved between `play` and
/// the advance that reads it. A scale is the one that needs this: it
/// interpolates *around* 1.0, and the first ~30ns of a linear ramp round to
/// exactly `Scale::NONE`. Where the timeline never ran at all, the animation
/// still holds the value it was built with, which is neutral, and that is what
/// each assertion is against.
#[test]
fn a_translate_sequence_and_a_scale_sequence_move_the_transform() {
    let plays = create_signal(0u32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0).translate(
                Translate::NONE.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, Translate::new(10.0, 0.0))
                        .at(0.5, Translate::new(30.0, 0.0))
                        .at(1.0, Translate::new(10.0, 0.0)),
                    plays,
                ),
            ),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();
    plays.set(1);
    let played = played_transform(&mut h).tx();
    assert!(
        played > 5.0,
        "a displacement sequence has to reach the paint, not only the job \
         queue, got {played}"
    );

    let plays = create_signal(0u32);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            box_of(50.0, 50.0).scale(
                Scale::NONE.timeline(
                    Keyframes::new(200.0)
                        .at(0.0, Scale::uniform(1.1))
                        .at(0.5, Scale::uniform(1.4))
                        .at(1.0, Scale::uniform(1.1)),
                    plays,
                ),
            ),
        ),
    );
    h.fit(200.0, 200.0);
    h.paint();
    plays.set(1);
    let played = played_transform(&mut h).a();
    assert!(played > 1.05, "and so does a pulse, got {played}");
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
            .when_pressed(|s| s.scale(0.5)),
    );
    h.fit(100.0, 100.0);

    set_hover(&mut h, true);
    h.send(Event::mouse_down(5.0, 5.0, MouseButton::Left));
    assert_eq!(rects(&h.paint())[0].1, Color::RED);
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

/// A gradient built in a closure follows the signals that built it.
#[test]
fn a_gradient_follows_the_signals_it_was_built_from() {
    let end = create_signal(Color::YELLOW);
    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .gradient(move || LinearGradient::vertical(Color::RED, end.get())),
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
            .elevation(0.0.transition(Transition::new(80.0, TimingFunction::Linear)))
            .when_hovered(|s| s.elevation(8.0)),
    );
    h.fit(100.0, 100.0);
    h.paint();

    assert_eq!(shadow_count(&h.paint()), 0, "flat on the surface at rest");

    set_hover(&mut h, true);
    // Two frames: the first starts the animation, the second is far enough up
    // the ramp for the shadow to be worth drawing — the very first values are
    // below the alpha floor by design, since a shadow nobody can see is a rect
    // drawn for nothing.
    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        pump(&mut h);
        h.fit(100.0, 100.0);
        h.paint();
    }
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

/// The four radii reach the clip, not just the paint: a row that rounds only
/// its top must cut its children there and leave them square at the bottom.
#[test]
fn the_clip_carries_every_corner() {
    let mut h = H::new(
        container()
            .width(100.0)
            .height(40.0)
            .corners([16.0, 0.0])
            .overflow(Overflow::Hidden)
            .child(container().width(100.0).height(40.0).background(Color::RED)),
    );
    let node = h.frame(200.0, 200.0).node;

    let clip = node.children[0]
        .clip
        .as_ref()
        .or(node.clip.as_ref())
        .expect("a hidden overflow clips");
    assert_eq!(clip.corner_radius.top_left, 16.0);
    assert_eq!(clip.corner_radius.top_right, 16.0);
    assert_eq!(clip.corner_radius.bottom_right, 0.0);
    assert_eq!(clip.corner_radius.bottom_left, 0.0);
}

/// A state override carries the whole shape, so a squircle that only asks to
/// grow on hover becomes an ordinary rounded box — the same trade the border
/// override makes, and the reason a bare size is spelled as one.
#[test]
fn a_state_override_replaces_the_whole_corner_shape() {
    let c = container()
        .corners(crate::widgets::Corners::squircle(12.0))
        .when_hovered(|s| s.corners(20.0));

    let hovered = c.interaction.as_ref().expect("a hover layer").states[0]
        .1
        .corners
        .expect("which declares a shape")
        .get_untracked();

    assert_eq!(hovered.curvature, 1.0, "rounded, not squircle");
    assert_eq!(hovered.radii.top_left, 20.0);
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
            .gradient(LinearGradient::horizontal(Color::RED, Color::BLUE)),
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
            .elevation(0.0.transition(Transition::new(80.0, TimingFunction::Linear)))
            .when_hovered(|s| s.elevation(8.0)),
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
    let bouncy = |transition: Transition| {
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(0.0.transition(transition))
            .when_hovered(|s| s.elevation(8.0))
    };

    let mut eased = H::new(bouncy(Transition::new(80.0, TimingFunction::Linear)));
    eased.fit(100.0, 100.0);
    let without_bounce = eased.tree.paint_overflow(eased.root);

    let mut sprung = H::new(bouncy(Transition::spring(SpringConfig::BOUNCY)));
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

/// The shadow's reach, as drawn — `None` where the box casts none.
fn shadow_extent(node: &RenderNode) -> Option<f32> {
    node.commands.iter().find_map(|c| match &**c {
        DrawCommand::RoundedRect {
            shadow: Some(shadow),
            ..
        } => Some(shadow.extent()),
        _ => None,
    })
}

/// A `.elevation(signal)` that falls has to *animate* down. It used to rise
/// animated and drop in one frame.
///
/// The reach the layout records is also the ceiling paint clamps to, and the
/// ceiling was recomputed at paint time from the declarations — which by then
/// say 0, while the shadow on screen is still 8 deep. Every frame of the descent
/// drew `min(interpolated, 0.0)`: no shadow at all, for an animation that went
/// on running and asking for frames.
///
/// The descent runs on a ramp no loaded runner can reach the end of, because the
/// claim is about what is drawn *while* it falls: with a short ramp, a slow
/// machine would finish the fall between two sleeps and the test would read the
/// correct absence of a shadow as the bug. Arrival is checked separately, by
/// settling rather than by a clock.
#[test]
fn a_falling_elevation_animates_down_instead_of_snapping() {
    let falling = |ramp_ms: f32| {
        let level = create_signal(8.0f32);
        let h = H::new(
            container()
                .width(40.0)
                .height(40.0)
                .background(Color::RED)
                .elevation(
                    (move || level.get())
                        .transition(Transition::new(ramp_ms, TimingFunction::Linear)),
                ),
        );
        (h, level)
    };

    let (mut h, level) = falling(60_000.0);
    h.fit(100.0, 100.0);
    assert!(
        shadow_extent(&h.paint()).is_some(),
        "a lifted card casts a shadow"
    );

    level.set(0.0);
    for step in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(4));
        pump(&mut h);
        h.fit(100.0, 100.0);
        assert!(
            shadow_extent(&h.paint()).is_some(),
            "frame {step} of the descent drew no shadow at all"
        );
    }

    // And it does arrive, on a ramp it can finish.
    let (mut h, level) = falling(60.0);
    h.fit(100.0, 100.0);
    h.paint();
    level.set(0.0);
    pump(&mut h);
    settle(&mut h, 200);
    assert_eq!(shadow_count(&h.paint()), 0, "and settles flat");
}

/// An elevation that goes flat damages the ring the shadow gave up, whichever
/// line gets there first.
///
/// A shrinking reach vacates a ring that a rect built from the *new* reach
/// stops short of. Two separate things cover it here, and the point of this
/// test is that it does not care which:
///
/// - `Container::layout` calls `cache_layout` before `publish_paint_reach`, so
///   the mark happens while the old reach is still standing.
/// - `Tree::set_own_paint_reach` damages the ring itself when the reach shrinks.
///
/// So this is not what proves that fix — an elevation never takes the path the
/// fix was written for, which is the Paint job (`refresh_paint_bounds` shrinks
/// the reach, `mark_needs_paint` follows), and that is watched at the `Tree`
/// level. What this says is that the *outcome* holds for a real elevation on a
/// real container, and keeps holding if either of the two ever goes away: swap
/// the two lines in `layout` and the setter still covers it, take the setter's
/// damage out and the order still does.
#[test]
fn an_elevation_going_flat_damages_the_ring_the_shadow_gave_up() {
    let level = create_signal(8.0f32);
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(move || level.get()),
    );
    h.fit(100.0, 100.0);
    h.paint();

    let lifted = h.tree.paint_overflow(h.root);
    assert!(lifted > 0.0, "a lifted card has to reach past its box");
    let ring = h.tree.get_bounds(h.root).expect("laid out").outset(lifted);
    let _ = h.tree.take_damage(h.root);

    level.set(0.0);
    pump(&mut h);
    h.fit(100.0, 100.0);
    assert_eq!(
        h.tree.paint_overflow(h.root),
        0.0,
        "the shadow is what this frame gives up"
    );

    match h.tree.take_damage(h.root) {
        crate::tree::DamageRegion::Partial(rect) => assert_eq!(
            ring.outset_beyond(rect),
            0.0,
            "the frame that flattens the shadow damages {rect:?}, so the ring \
             it cast at {ring:?} is left on screen"
        ),
        other => panic!("expected partial damage, got {other:?}"),
    }
}

/// The clamp the descent above must not trip is still doing its job.
///
/// A spring keeps its momentum across a retarget, so hover flicker pumps it:
/// driven near its damped natural frequency it settles at the resonant gain,
/// which for a lightly damped spring is several times the step response the
/// reach is measured from. Whatever it reaches, it is not allowed to draw
/// outside the rect the layout reserved — that ring would be composited nowhere
/// and left on screen.
#[test]
fn hover_flicker_cannot_push_a_shadow_outside_its_damage_rect() {
    // Lightly damped (zeta = 0.1, resonant gain ~3.7x) and fast, so the pumping
    // shows up within a test's worth of frames.
    let pumpable = SpringConfig {
        mass: 1.0,
        stiffness: 3600.0,
        damping: 12.0,
    };
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            // Small enough that the shadow is still growing with the number:
            // past ~12 the blur and the offset are both capped, so a clamp
            // there would have nothing to show for itself.
            .elevation(0.0.transition(Transition::spring(pumpable)))
            .when_hovered(|s| s.elevation(2.0)),
    );
    h.fit(100.0, 100.0);
    h.paint();

    let reach = h.tree.paint_overflow(h.root);
    let mut inside = false;
    let mut worst: f32 = 0.0;
    // Flipped by the clock, not by a frame count: pumping means reversing every
    // half period (~53ms here), and a frame is only worth ~8ms of it.
    let mut last_flip = std::time::Instant::now();
    for step in 0..120 {
        if last_flip.elapsed() >= std::time::Duration::from_millis(52) {
            inside = !inside;
            set_hover(&mut h, inside);
            last_flip = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
        pump(&mut h);
        h.fit(100.0, 100.0);
        let node = h.paint();
        if let Some(extent) = shadow_extent(&node) {
            worst = worst.max(extent);
            assert!(
                extent <= reach + 0.01,
                "step {step} drew a shadow reaching {extent} outside a damage rect of {reach}"
            );
        }
    }
    assert!(worst > 0.0, "the flicker has to actually raise a shadow");
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

/// A container that clips does not report the overhang it is clipping away.
///
/// A scroller's content runs far past its viewport by design. Counting that as
/// "how far this widget paints outside itself" would damage a rect the size of
/// the whole scrolled column on every frame the scroller repaints, and widen
/// every search above it by the same. What it paints is its own box.
#[test]
fn a_clipping_container_does_not_carry_its_content_s_overhang() {
    let rows: Vec<_> = (0..20)
        .map(|_| container().width(100.0).height(50.0).background(Color::RED))
        .collect();
    let mut clipped = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .scroll(Scroll::vertical())
            .layout(Flex::column())
            .children(rows),
    );
    clipped.fit(400.0, 400.0);

    assert_eq!(
        clipped.tree.paint_overflow(clipped.root),
        0.0,
        "a scroller reported the height of everything inside it as paint that \
         lands outside its own box"
    );
}

/// And it keeps not carrying it, after a descendant moves.
///
/// The clear at layout is one write; the gather that runs whenever any
/// descendant's reach changes writes the same field. A row with an animating
/// transform does that every frame, so a scroller that only forgot the overhang
/// once would have it back on the next one — with its damage rect and every
/// search above it sized to the whole scrolled column.
#[test]
fn a_clipping_container_keeps_not_carrying_it_when_a_child_moves() {
    let lift = create_signal(0.0f32);
    let mut rows: Vec<_> = (0..20)
        .map(|_| container().width(100.0).height(50.0).background(Color::RED))
        .collect();
    // The first row, so it is inside the viewport and therefore painted — a
    // row culled on the first frame holds no paint subscription and its own
    // write would wake nobody.
    rows[0] = container()
        .width(100.0)
        .height(50.0)
        .background(Color::BLUE)
        .translate(move || Translate::new(0.0, -lift.get()));

    let mut h = H::new(
        container()
            .width(100.0)
            .height(100.0)
            .scroll(Scroll::vertical())
            .layout(Flex::column())
            .children(rows),
    );
    h.fit(400.0, 400.0);
    // Painted, so the row holds the paint subscription its own write needs.
    h.paint();
    assert_eq!(h.tree.paint_overflow(h.root), 0.0);

    // A frame of an animating row, with no layout in between.
    lift.set(20.0);
    pump(&mut h);

    assert_eq!(
        h.tree.paint_overflow(h.root),
        0.0,
        "one row moved and the scroller went back to claiming it paints the \
         whole column outside its own box"
    );
}

/// A 40x40 box carried off its laid-out place by `lift` — the smallest thing
/// whose paint leaves its bounds without anything laying out.
fn lifted_box(lift: RwSignal<f32>) -> Container {
    container()
        .width(40.0)
        .height(40.0)
        .background(Color::RED)
        .translate(move || Translate::new(0.0, -lift.get()))
}

/// The transform's twin, and the one that proves `refresh_paint_bounds` is
/// wired at all: the reach has to grow on a Paint job, with no layout run
/// between the write and the answer.
///
/// A shadow's reach is republished by layout, so an elevation test passes
/// whether or not the Paint-job refresh exists. A transform's is not — layout
/// never subscribes to it — so this is the only thing standing between the hook
/// in `process_jobs` and being deleted with the suite still green.
#[test]
fn a_declared_transform_change_invalidates_the_reach_without_a_layout() {
    let lift = create_signal(0.0f32);
    let mut h = H::new(lifted_box(lift));
    h.fit(100.0, 100.0);
    h.paint();
    assert_eq!(h.tree.paint_overflow(h.root), 0.0);

    lift.set(30.0);
    let queued = jobs::queued_job_types(h.root);
    assert!(
        !queued.contains(&JobType::Layout),
        "a transform asked for a layout: {queued:?}"
    );

    // No `fit` between the write and the assertion: the reach has to be current
    // from the job alone, because the parent narrows to it in the same frame
    // and nothing lays out in between.
    pump(&mut h);
    assert_eq!(
        h.tree.paint_overflow(h.root),
        30.0,
        "the reach did not follow the transform through its Paint job"
    );
}

/// A child that is its own relayout boundary moves without its parent laying
/// out, and the reach still has to reach the parent.
///
/// This is what `gather_reach_upward` is for, and the only thing that asks for
/// it: `publish_paint_reach` gathers a container's children when *it* lays out,
/// which covers everything except the case where it does not lay out at all. A
/// fixed width and height make the row a boundary (`is_relayout_boundary_for`),
/// so `mark_needs_layout` stops there and the parent never re-runs.
#[test]
fn a_reach_that_grows_under_a_relayout_boundary_still_reaches_the_parent() {
    let lift = create_signal(0.0f32);
    let mut h = H::new(container().layout(Flex::column()).child(lifted_box(lift)));
    h.fit(200.0, 200.0);
    h.paint();
    assert_eq!(h.tree.children_reach(h.root), 0.0);

    lift.set(70.0);
    pump(&mut h);

    assert_eq!(
        h.tree.children_reach(h.root),
        70.0,
        "the row moved under a relayout boundary and its parent never heard, so \
         the search that narrows it would drop a row that is on screen"
    );
}

/// And one that shrinks lets go again.
///
/// Growing upward can be answered by comparison — wider than the parent knows,
/// so widen it — but shrinking cannot: this child may have been the widest and
/// the new maximum is whatever the others say. Answering it by comparison alone
/// would leave a scroller widened by a transform that has long since come to
/// rest, for as long as it goes without laying out, and a window widened for
/// nothing is virtualization spent for nothing.
#[test]
fn a_reach_that_shrinks_lets_the_parent_narrow_again() {
    let lift = create_signal(70.0f32);
    let mut h = H::new(container().layout(Flex::column()).child(lifted_box(lift)));
    h.fit(200.0, 200.0);
    h.paint();
    assert_eq!(h.tree.children_reach(h.root), 70.0);

    // No `fit`: the parent must let go without laying out, because a scroller
    // that is not being resized never does.
    lift.set(0.0);
    pump(&mut h);

    assert_eq!(
        h.tree.children_reach(h.root),
        0.0,
        "the parent stayed widened by a transform that has come to rest"
    );
}

// ---------------------------------------------------------------------------
// Compositor blur regions — every one of these is about which widgets painted
// ---------------------------------------------------------------------------
//
// The surface half of a backdrop blur is a draw command in the render tree. The
// compositor half is a `wl_region` derived from the same command after
// flattening, which is what makes these tests about *painting*: a container that
// did not paint has no command, one served from the cache carries its along, and
// a frame that repaints nothing publishes from the buffer it retained.
//
// They all go through `H::frame`, which runs those phases in the loop's order
// and with the loop's own `cache_paint_results` — the two things a test of this
// cannot model loosely and still be a test.

/// Turning a blur off has to reach the compositor on the frame that turns it
/// off. While the region was collected before the paint, the frame that switched
/// it off published the *previous* frame's answer, and nothing was dirty
/// afterwards — so the withdrawal waited for a frame a settled surface never
/// renders.
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
            .scroll(Scroll::vertical())
            .children(rows),
    );
    assert!(
        !h.frame(40.0, 30.0).blur.is_empty(),
        "visible at the top of the scroll"
    );

    h.send(Event::scroll(20.0, 15.0, 0.0, 600.0, ScrollSource::Wheel));
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

/// The border is gated on resolving to something visible; the shadow has to be
/// too. The opening frames of a lift are at ~0.001, where the shadow's alpha
/// rounds to nothing — and without a floor each one still pushed a rect
/// carrying it, every frame, for as long as the animation was leaving zero.
#[test]
fn a_shadow_too_faint_to_see_is_not_drawn() {
    let level = create_signal(0.001f32);
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .background(Color::RED)
            .elevation(move || level.get()),
    );
    h.fit(100.0, 100.0);
    assert_eq!(shadow_count(&h.paint()), 0, "invisible, so not drawn");

    level.set(1.0);
    h.fit(100.0, 100.0);
    assert_eq!(shadow_count(&h.paint()), 1, "and drawn once it can be seen");
}

/// The clip is part of the shape, for the region as much as for the renderer. A
/// blurred card cut off by its parent must publish only the part on show, or the
/// desktop is blurred beside a panel that is not there.
#[test]
fn a_clipped_blur_publishes_only_what_is_on_show() {
    // Content overflows by summing: each child answers to the parent's maximum,
    // their row does not. So the second card sits entirely past the clip.
    let card = || container().width(40.0).height(40.0).backdrop_blur(24.0);
    let mut h = H::new(
        container()
            .width(40.0)
            .height(40.0)
            .overflow(Overflow::Hidden)
            .layout(Flex::row())
            .child(card())
            .child(card()),
    );
    let frame = h.frame(200.0, 200.0);

    let widest = frame
        .blur
        .iter()
        .map(|r| r.x + r.width)
        .max()
        .expect("the visible card still blurs");
    assert!(
        widest <= 40,
        "the row is 80 wide inside a 40-wide clip, so no region may reach {widest}"
    );
}

/// The region is the shape *as drawn*, so a transform has to reach it: a scaled
/// card covers more of the desktop and a rotated one covers a diagonal.
///
/// End to end through the real paint and flatten, because the geometry is
/// derived there. The unit tests in `renderer::flatten` measure the arithmetic;
/// this is the wiring — a region built from a command's local rect instead of
/// its world one passes every one of those and still publishes a 40x40 upright
/// box for a card the compositor is showing at 80x80 turned on its corner.
#[test]
fn a_transformed_blur_publishes_the_shape_it_is_drawn_as() {
    let card = |c: Container| c.width(40.0).height(40.0).backdrop_blur(24.0);

    let plain = H::new(card(container())).frame(200.0, 200.0).blur;
    let scaled = H::new(card(container()).scale(2.0))
        .frame(200.0, 200.0)
        .blur;
    let turned = H::new(card(container()).rotate(45.0))
        .frame(200.0, 200.0)
        .blur;

    let span = |rects: &[crate::blur::BlurRect]| {
        let left = rects.iter().map(|r| r.x).min().expect("a region");
        let right = rects.iter().map(|r| r.x + r.width).max().expect("a region");
        right - left
    };

    let (plain, scaled, turned) = (span(&plain), span(&scaled), span(&turned));
    assert!(
        scaled >= plain * 2 - 2,
        "twice the card is twice the region: {scaled} against {plain}"
    );
    assert!(
        turned > plain && turned < scaled,
        "turned on its corner it covers its diagonal, {turned} against {plain}"
    );
}

/// A gradient between two fully transparent colours draws nothing, and must not
/// push a rect per frame to do it — the same gate the fill, the border and the
/// shadow have. With both endpoints reactive, one animating out through
/// transparent reaches this every frame.
#[test]
fn a_fully_transparent_gradient_draws_nothing() {
    let clear = Color::rgba(1.0, 0.0, 0.0, 0.0);

    let mut h = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .gradient(LinearGradient::horizontal(clear, clear)),
    );
    h.fit(100.0, 100.0);
    assert_eq!(rects(&h.paint()), Vec::new(), "nothing to draw");

    // One end still visible is still a gradient.
    let mut half = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .gradient(LinearGradient::horizontal(clear, Color::BLUE)),
    );
    half.fit(100.0, 100.0);
    assert_eq!(
        gradient_ends(&half.paint()),
        Some((clear, Color::BLUE)),
        "fading to transparent is a gradient"
    );

    // And an invisible gradient does not hide the background under it.
    let mut over = H::new(
        container()
            .width(20.0)
            .height(20.0)
            .background(Color::GRAY)
            .gradient(LinearGradient::horizontal(clear, clear)),
    );
    over.fit(100.0, 100.0);
    assert_eq!(
        rects(&over.paint()).first().map(|(_, c)| *c),
        Some(Color::GRAY)
    );
}
