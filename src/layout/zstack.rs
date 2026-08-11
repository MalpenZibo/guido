//! ZStack layout that stacks children on top of each other.

use crate::tree::{Tree, WidgetId};
use crate::widgets::LayoutHints;

use super::{Constraints, Layout, Size};

/// Layout that places all children at the same position, stacking them along
/// the Z axis. Later children appear on top.
///
/// # Sizing
///
/// The stack takes the size of its largest child — but a child that declares
/// [`fill()`](super::fill) on an axis **follows** the stack instead of leading
/// it: it never contributes to that axis, and it is laid out against the size
/// the other children established.
///
/// That is how a decoration is sized to its sibling without measuring
/// anything:
///
/// ```
/// use guido::prelude::*;
///
/// container()
///     .layout(ZStack::new())
///     // Background: exactly as wide and tall as the label below
///     .child(container().width(fill()).height(fill()).background(Color::RED))
///     // Content: leads, so the stack is as big as this text
///     .child(text("hello"));
/// ```
///
/// Both axes are decided independently, which is what makes the common bar
/// case work: a child with only `.height(fill())` still leads the width.
///
/// When *every* child fills an axis there is nothing to follow, and the stack
/// takes all the space it was offered on that axis.
///
/// # Positioning
///
/// Every child is placed at the stack's origin. To align a follower inside
/// the stack, give it `fill()` on both axes and let its own layout do the
/// positioning:
///
/// ```
/// use guido::prelude::*;
///
/// container()
///     .layout(ZStack::new())
///     .child(text("content"))
///     // Badge pinned to the top-right corner of the content
///     .child(
///         container()
///             .width(fill())
///             .height(fill())
///             .layout(Flex::row().main_alignment(MainAlignment::End))
///             .child(container().width(4).height(4).corner_radius(2)),
///     );
/// ```
#[derive(Default)]
pub struct ZStack {
    /// Scratch buffer for per-child fill hints, reused across layouts.
    hints: Vec<LayoutHints>,
}

impl ZStack {
    /// Create a new z-stack layout
    pub fn new() -> Self {
        Self::default()
    }
}

impl Layout for ZStack {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        self.hints.clear();
        self.hints.extend(children.iter().map(|&child_id| {
            tree.with_widget(child_id, |w| w.layout_hints())
                .unwrap_or_default()
        }));

        // Pass 1: children that don't fill an axis establish the stack size on
        // that axis. A child that fills both can only follow, so it is left
        // for pass 2 — laying it out here would just be thrown away.
        let mut stack = Size::zero();
        let (mut led_width, mut led_height) = (false, false);

        for (i, &child_id) in children.iter().enumerate() {
            let hints = self.hints[i];
            if hints.fill_width && hints.fill_height {
                continue;
            }
            let Some(child_size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                widget.layout(tree, id, constraints)
            }) else {
                continue;
            };
            if !hints.fill_width {
                stack.width = stack.width.max(child_size.width);
                led_width = true;
            }
            if !hints.fill_height {
                stack.height = stack.height.max(child_size.height);
                led_height = true;
            }
        }

        // Nothing leads this axis: there is no sibling size to follow, so the
        // stack takes the space it was offered.
        if !led_width {
            stack.width = constraints.max_width;
        }
        if !led_height {
            stack.height = constraints.max_height;
        }
        let size = constraints.constrain(stack);

        // Pass 2: followers resolve against the stack size — tight on the axes
        // they fill, unchanged on the axes they lead.
        for (i, &child_id) in children.iter().enumerate() {
            let hints = self.hints[i];
            if !hints.fill_width && !hints.fill_height {
                continue;
            }
            // A child that leads one axis was already laid out in pass 1.
            // Only a change on the axis it follows can alter its layout: on
            // the axis it leads, the max shrinks at most down to the size it
            // reported, which cannot change a layout that already fit.
            let leads_an_axis = !hints.fill_width || !hints.fill_height;
            let followed_size_changed = (hints.fill_width && size.width != constraints.max_width)
                || (hints.fill_height && size.height != constraints.max_height);
            if leads_an_axis && !followed_size_changed {
                continue;
            }
            let child_constraints = Constraints {
                min_width: if hints.fill_width {
                    size.width
                } else {
                    constraints.min_width.min(size.width)
                },
                min_height: if hints.fill_height {
                    size.height
                } else {
                    constraints.min_height.min(size.height)
                },
                max_width: size.width,
                max_height: size.height,
            };
            tree.with_widget_mut(child_id, |widget, id, tree| {
                widget.layout(tree, id, child_constraints)
            });
        }

        for &child_id in children.iter() {
            tree.set_origin(child_id, origin.0, origin.1);
        }

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Flex, fill};
    use crate::widgets::container;

    /// Lay out `widget` and return the tree plus the root id.
    fn layout_root(
        widget: crate::widgets::Container,
        constraints: Constraints,
    ) -> (Tree, WidgetId) {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));
        tree.with_widget_mut(id, |w, id, tree| {
            w.register_children(tree, id);
            w.layout(tree, id, constraints);
        });
        (tree, id)
    }

    fn child_size(tree: &Tree, root: WidgetId, index: usize) -> Size {
        let child = tree.get_children(root)[index];
        tree.cached_size(child).expect("child was laid out")
    }

    /// A filling child follows the size its sibling established instead of
    /// expanding to all the available space — the whole point of the layout:
    /// decorations sized to their sibling without a measure round-trip.
    #[test]
    fn filling_child_follows_its_sibling() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(fill()).height(fill()))
            .child(container().width(80).height(20));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, 400.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(80.0, 20.0)));
        assert_eq!(
            child_size(&tree, id, 0),
            Size::new(80.0, 20.0),
            "the follower must take the leader's size, not the 400x400 offered"
        );
        assert_eq!(tree.get_origin(tree.get_children(id)[0]), Some((0.0, 0.0)));
        assert_eq!(tree.get_origin(tree.get_children(id)[1]), Some((0.0, 0.0)));
    }

    /// Axes are decided independently: the bar case, where the content fills
    /// the bar height yet still dictates the width of the visualizer behind
    /// it.
    #[test]
    fn axes_are_decided_independently() {
        let root = container()
            .layout(ZStack::new())
            // Background follows both axes
            .child(container().width(fill()).height(fill()))
            // Content fills the bar height but leads the width
            .child(container().width(60).height(fill()));

        // Tight height (a bar row), loose width
        let (tree, id) = layout_root(root, Constraints::new(0.0, 34.0, 400.0, 34.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(60.0, 34.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(60.0, 34.0));
        assert_eq!(child_size(&tree, id, 1), Size::new(60.0, 34.0));
    }

    /// With no leader on an axis there is no sibling size to follow, so the
    /// stack keeps the pre-follow behaviour: it takes what it was offered.
    #[test]
    fn all_followers_fall_back_to_the_available_space() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(fill()).height(fill()))
            .child(container().width(fill()).height(fill()));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, 200.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(400.0, 200.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(400.0, 200.0));
        assert_eq!(child_size(&tree, id, 1), Size::new(400.0, 200.0));
    }

    /// A follower that fills both axes can position content inside the stack
    /// with its own layout — a badge pinned to the leader's right edge.
    #[test]
    fn follower_can_align_inside_the_stack() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(50).height(50))
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .layout(Flex::row().main_alignment(crate::layout::MainAlignment::End))
                    .child(container().width(4).height(4)),
            );

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, 400.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(50.0, 50.0)));
        let follower = tree.get_children(id)[1];
        let badge = tree.get_children(follower)[0];
        assert_eq!(
            tree.get_origin(badge),
            Some((46.0, 0.0)),
            "the badge should sit at the right edge of the 50px leader"
        );
    }
}
