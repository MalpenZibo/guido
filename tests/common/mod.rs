//! The four lines every integration test writes before it can ask anything.
//!
//! A widget cannot be laid out, pointed at or painted without a [`Tree`] to
//! hold its bounds, and registering one takes the same shape every time:
//! register, register its children, lay it out under some constraints. Five
//! test files had written that out privately before this existed, and each had
//! drifted — one dispatched inside a snapshot zone and the others did not,
//! which decides whether the non-reactive-read diagnostic fires on a hit test.
//!
//! What is *not* here is anything a single file needs: a scrollbar's handle
//! node, a caption's painted top, the colour of a named text. Those stay where
//! they are asked about. This is the surface underneath them.
//!
//! Below `Harness`, and for the same reason, is the rule that decides whether
//! a run may rewrite a reference file. It is not about widgets at all; it is
//! here because both reference harnesses have to read the same copy of it, and
//! the last time it lived in two places it lived in neither.
//!
//! Each test binary compiles its own copy — that is what `mod common;` does —
//! so a helper only one file uses is dead code in the others.
#![allow(dead_code)]

use guido::layout::{Constraints, Size};
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::{Tree, WidgetId};
use guido::widgets::widget::EventResponse;

/// The Material elevation ladder, written out as the six `Shadow` values the
/// library's own table used to produce for levels 0 to 5.
///
/// It lives here, in a test file, because that is where it belongs now: guido
/// ships `shadow` and no ladder, so a design system's steps are values the
/// application writes down. Two reference harnesses draw this one, and the
/// numbers are exact rather than approximate so the pixels and the snapshot
/// text did not move when the table was deleted.
pub const LADDER: [Shadow; 6] = [
    Shadow::none(),
    Shadow::new((0.0, 1.0), 3.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.12)),
    Shadow::new((0.0, 2.0), 4.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.16)),
    Shadow::new((0.0, 3.0), 6.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.19)),
    Shadow::new((0.0, 4.0), 8.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.20)),
    Shadow::new((0.0, 6.0), 10.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.22)),
];

/// A registered widget and the tree that holds its bounds.
pub struct Harness {
    pub tree: Tree,
    pub root: WidgetId,
}

impl Harness {
    /// Register `widget` as the root, and its children with it.
    pub fn new(widget: impl Widget + 'static) -> Self {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        Self { tree, root }
    }

    /// Register a widget and lay it out at once, which is what a test that
    /// then asks a question about it wants.
    pub fn laid_out(widget: impl Widget + 'static, width: f32, height: f32) -> Self {
        let mut surface = Self::new(widget);
        surface.lay_out(width, height);
        surface
    }

    /// Lay out loose within `width` x `height`.
    pub fn lay_out(&mut self, width: f32, height: f32) -> Size {
        let root = self.root;
        self.tree
            .with_widget_mut(root, |w, id, t| {
                w.layout(t, id, Constraints::new(0.0, 0.0, width, height))
            })
            .expect("the root is registered")
    }

    /// Deliver one event.
    ///
    /// Inside a snapshot zone, because that is where the real loop dispatches
    /// (`render_surface`): hit testing reads animated values for *this* event
    /// and must not subscribe to them. Outside one those reads trip the
    /// non-reactive-read diagnostic, which would be the harness reporting on
    /// itself.
    pub fn send(&mut self, event: Event) -> EventResponse {
        let root = self.root;
        guido::reactive::diagnostics::snapshot_zone(|| {
            self.tree
                .with_widget_mut(root, |w, id, t| w.event(t, id, &event))
                .expect("the root is registered")
        })
    }

    /// Deliver one event, at a named instant.
    pub fn send_at(&mut self, event: Event, at: std::time::Instant) -> EventResponse {
        self.tree.set_event_instant(Some(at));
        let response = self.send(event);
        self.tree.set_event_instant(None);
        response
    }

    /// A press and the release that follows it, at the same place.
    pub fn click(&mut self, x: f32, y: f32) {
        self.send(Event::mouse_down(x, y, MouseButton::Left));
        self.send(Event::mouse_up(x, y, MouseButton::Left));
    }

    /// Every rectangle the tree painted, in the order it drew them.
    ///
    /// What a caret or a selection highlight is made of: `TextInput` draws
    /// both as rounded rects with no radius, and two files ask about them.
    pub fn painted_rects(&mut self) -> Vec<Rect> {
        fn collect(node: &RenderNode, out: &mut Vec<Rect>) {
            for cmd in &node.commands {
                if let DrawCommand::RoundedRect { rect, .. } = &**cmd {
                    out.push(*rect);
                }
            }
            for child in &node.children {
                collect(child, out);
            }
        }

        let mut out = Vec::new();
        collect(&self.paint(), &mut out);
        out
    }

    /// Paint the whole tree and hand back what it drew.
    pub fn paint(&mut self) -> RenderNode {
        let root = self.root;
        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
        node
    }

    /// One animation pass, at a named instant, post-order as the loop runs it.
    pub fn advance(&mut self, at: std::time::Instant) {
        self.tree.set_frame_instant(Some(at));
        for id in self.tree.collect_subtree_post_order(self.root) {
            self.tree
                .with_widget_mut(id, |w, id, t| w.advance_animations(t, id));
        }
        self.tree.set_frame_instant(None);
    }
}

/// What to do with a reference file this run disagrees with.
///
/// Two variables, and the difference between them is the whole rule.
/// `UPDATE_*` makes a reference that is not there yet: a new scenario needs a
/// first picture and that is ordinary work. `REBLESS_*` rewrites one that is,
/// which turns a failing test green without changing anything back — so it is
/// a decision, taken with the diff in front of somebody, and the pull request
/// carries the `golden-update` label to say it was taken.
///
/// One copy, read by both `assert_snapshot` and `assert_golden`, because the
/// last time the rule lived in two places it lived in neither: both harnesses
/// wrote unconditionally on `UPDATE_*` while their own module headers said
/// they declined, and `REBLESS_*` was read nowhere at all.
///
/// A pure function of three booleans, so the table below is the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blessing {
    /// Write the reference, and skip the comparison.
    Write,
    /// Compare against what is on disk, whatever was asked for.
    Compare,
}

/// What to do, given what the environment asked for and whether the
/// reference is already on disk.
pub fn blessing(update: bool, rebless: bool, exists: bool) -> Blessing {
    match (update, rebless, exists) {
        // Rewriting is what REBLESS is, and the only thing it is.
        (_, true, _) => Blessing::Write,
        // A reference that is not there yet is not a rewrite.
        (true, false, false) => Blessing::Write,
        // Asked to create one that already exists: decline, and let the
        // comparison report what actually moved. This is the line that was
        // missing.
        (true, false, true) => Blessing::Compare,
        (false, false, _) => Blessing::Compare,
    }
}

/// Read the pair for one kind of reference — `"SNAPSHOTS"` or `"GOLDEN"`.
///
/// Says so when it declines. Somebody who asked to bless and got silence would
/// read the pass that follows as the blessing having happened.
pub fn blessing_from_env(kind: &str, exists: bool) -> Blessing {
    let asked = |prefix: &str| std::env::var_os(format!("{prefix}_{kind}")).is_some();
    let update = asked("UPDATE");
    let decision = blessing(update, asked("REBLESS"), exists);
    if update && decision == Blessing::Compare {
        eprintln!(
            "UPDATE_{kind} declined: this reference already exists, and rewriting one \
             makes a failing test pass without changing anything back. Read the diff \
             the comparison is about to print; if the change is intended, that is \
             REBLESS_{kind}=1 and the `golden-update` label on the pull request."
        );
    }
    decision
}

/// Write the reference at `path`, if the rule allows it. Returns whether it
/// wrote, so the caller can fall through to the comparison when it did not.
///
/// The writing is here rather than at the two call sites so that "decided it
/// may write" and "wrote" cannot come apart: a harness that consults the rule
/// and then writes anyway is the defect this all exists to stop, and it would
/// read as compliant at both call sites.
pub fn write_if_blessed(path: &std::path::Path, blessing: Blessing, write: impl FnOnce()) -> bool {
    match blessing {
        Blessing::Write => {
            write();
            assert!(
                path.exists(),
                "blessed {} and it is still not there",
                path.display()
            );
            true
        }
        Blessing::Compare => false,
    }
}
