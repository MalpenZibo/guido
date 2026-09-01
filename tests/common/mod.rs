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
//! Each test binary compiles its own copy — that is what `mod common;` does —
//! so a helper only one file uses is dead code in the others.
#![allow(dead_code)]

use guido::layout::{Constraints, Size};
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::{Tree, WidgetId};
use guido::widgets::widget::EventResponse;

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
