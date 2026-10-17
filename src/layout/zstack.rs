//! ZStack layout that stacks children on top of each other.

use crate::reactive::{IntoSignal, Prop};
#[cfg(test)]
use crate::tree::Tree;
use crate::tree::{LayoutCtx, WidgetId};
use crate::widgets::LayoutHints;

use super::{Alignment, Constraints, Layout, Size};

/// Layout that places all children at the same position, stacking them along
/// the Z axis. Later children appear on top, and a pointer event is offered to
/// them first: one that draws or listens where the point is covers the ones
/// beneath it, and one that only lays out does not.
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
/// takes all the space it was offered on that axis — or, on an axis with no
/// maximum, the size of its largest child.
///
/// A hidden child takes no part in any of this, as it takes no space in a
/// [`Flex`](super::Flex): a stack whose only leader is hidden has nothing to
/// follow either.
///
/// # Positioning
///
/// Every child is placed by an [`Alignment`]: the stack's own, set with
/// [`align`](Self::align), or one the child declares for itself with
/// [`align_self`](crate::widgets::Container::align_self). The stack's is the
/// top start until it says otherwise, and a child that says nothing takes the
/// stack's — so a stack nobody aligns puts every child at its origin.
///
/// ```
/// use guido::prelude::*;
///
/// container()
///     .layout(ZStack::new().align(Alignment::CENTER))
///     // Leads: the stack is as big as this, and it fills the stack
///     .child(container().width(40).height(40).background(Color::BLUE))
///     // Centred over it, as the stack says
///     .child(text("3"))
///     // A badge on the top-right corner, as it says itself
///     .child(
///         container()
///             .width(8)
///             .height(8)
///             .corners(4)
///             .background(Color::RED)
///             .align_self(Alignment::TOP_END),
///     );
/// ```
///
/// A child narrower than the stack has free space to be placed in, and the
/// alignment says where; a child that follows the stack on an axis is laid out
/// at the stack's size there, so it has none and stays where the stack is. A
/// child larger than the stack on an axis is shared out the same way, so
/// `CENTER` overhangs both edges equally, as Flutter's `Stack` and SwiftUI's
/// `ZStack` do.
#[derive(Default)]
pub struct ZStack {
    align: Prop<Alignment>,
    /// Scratch buffer for per-child fill hints, reused across layouts.
    hints: Vec<LayoutHints>,
}

impl ZStack {
    /// Create a new z-stack layout
    pub fn new() -> Self {
        Self::default()
    }

    /// Where every child sits in the stack, unless it declares its own with
    /// [`align_self`](crate::widgets::Container::align_self). Top start by
    /// default.
    pub fn align<M>(mut self, alignment: impl IntoSignal<Alignment, M>) -> Self {
        self.align = alignment.into_prop();
        self
    }
}

impl Layout for ZStack {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        self.hints.clear();
        self.hints.extend(children.iter().map(|&child_id| {
            ctx.tree_ref()
                .with_widget(child_id, |w| w.layout_hints())
                .unwrap_or_default()
        }));

        // Pass 1: children that don't fill an axis establish the stack size on
        // that axis. A child that fills both can only follow, so it is left
        // for pass 2 — laying it out here would just be thrown away.
        let mut stack = Size::zero();
        let (mut led_width, mut led_height) = (false, false);
        // An axis with no maximum and nothing to lead it has no space to take
        // either, and the stack is as big as its children there — so a child
        // that fills both axes has to be measured after all. A hidden child
        // leads nothing.
        let unled = |max: f32, leads: fn(&LayoutHints) -> bool| {
            !max.is_finite() && !self.hints.iter().any(|h| !h.hidden && leads(h))
        };
        let measure_followers = unled(constraints.max_width, |h| !h.fill_width)
            || unled(constraints.max_height, |h| !h.fill_height);
        let mut largest = Size::zero();

        for (i, &child_id) in children.iter().enumerate() {
            let hints = self.hints[i];
            if hints.fill_width && hints.fill_height && !measure_followers {
                continue;
            }
            let Some(child_size) = ctx.layout_child(child_id, constraints) else {
                continue;
            };
            // Laid out all the same, so it hears its visibility change; but
            // hidden is `display: none`, and it leads nothing.
            if hints.hidden {
                continue;
            }
            largest.width = largest.width.max(child_size.width);
            largest.height = largest.height.max(child_size.height);
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
        // stack takes the space it was offered — or, with none to offer, the
        // size of its children.
        if !led_width {
            stack.width = if constraints.max_width.is_finite() {
                constraints.max_width
            } else {
                largest.width
            };
        }
        if !led_height {
            stack.height = if constraints.max_height.is_finite() {
                constraints.max_height
            } else {
                largest.height
            };
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
            ctx.layout_child(child_id, child_constraints);
        }

        // A follower was laid out at the stack's size on the axes it fills,
        // so there is no free space for its alignment to share there.
        //
        // The children are drawn over one another, so each needs all of its
        // own: the stack is as small as the largest of them can be.
        let stack_align = self.align.get_or(Alignment::default());
        let mut least = Size::zero();
        for (i, &child_id) in children.iter().enumerate() {
            let child_size = ctx.tree_ref().cached_size(child_id).unwrap_or_default();
            let (dx, dy) = self.hints[i]
                .align_self
                .unwrap_or(stack_align)
                .offset(size, child_size);
            ctx.tree()
                .set_origin(child_id, origin.0 + dx, origin.1 + dy);
            let child = ctx.tree_ref().minimum(child_id).unwrap_or_default();
            least.width = least.width.max(child.width);
            least.height = least.height.max(child.height);
        }
        ctx.set_minimum(least);

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Alignment, Flex, fill};
    use crate::widgets::container;

    /// Lay out `widget` and return the tree plus the root id.
    fn layout_root(
        widget: crate::widgets::Container,
        constraints: Constraints,
    ) -> (Tree, WidgetId) {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));
        tree.with_widget_mut(id, |w, id, tree| w.register_children(tree, id));
        tree.layout_widget(id, constraints);
        (tree, id)
    }

    fn child_size(tree: &Tree, root: WidgetId, index: usize) -> Size {
        let child = tree.get_children(root)[index];
        tree.cached_size(child).expect("child was laid out")
    }

    /// A stack is as small as the least of its children allows, and no
    /// smaller: they are drawn over one another, so each needs all of its own.
    #[test]
    fn a_stack_records_its_largest_minimum() {
        let offered = Constraints::new(0.0, 0.0, 400.0, 400.0);
        let (tree, root) = layout_root(
            container()
                .layout(ZStack::new())
                .child(container().width(30.0).height(50.0))
                .child(container().width(50.0).height(20.0)),
            offered,
        );
        assert_eq!(tree.minimum(root), Some(Size::new(50.0, 50.0)));
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

    /// A hidden child takes no space, as `display: none` takes none: it does
    /// not lead the stack, so the stack and its follower keep what was offered.
    #[test]
    fn a_hidden_child_takes_no_part_in_the_size() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(fill()).height(fill()))
            .child(container().visible(false).width(10).height(10));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, 200.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(400.0, 200.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(400.0, 200.0));
    }

    /// On an axis with no maximum a hidden child is no leader either, so the
    /// follower is measured and the stack is as tall as its content.
    #[test]
    fn a_hidden_child_does_not_lead_an_unbounded_axis() {
        let root = container()
            .layout(ZStack::new())
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .child(container().width(20).height(30)),
            )
            .child(container().visible(false).width(10).height(10));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, f32::INFINITY));

        assert_eq!(tree.cached_size(id), Some(Size::new(400.0, 30.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(400.0, 30.0));
    }

    /// The hidden child is still laid out, so showing it is heard: the stack
    /// follows the overlay while it shows, and the space offered while not.
    #[test]
    fn an_overlay_shown_and_hidden_again_leads_only_while_shown() {
        let shown = crate::reactive::create_signal(false);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .layout(ZStack::new())
                .child(container().width(fill()).height(fill()))
                .child(container().visible(shown).width(10).height(10)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let frame = |tree: &mut Tree| {
            crate::jobs::pump_and_layout(tree, root, Constraints::new(0.0, 0.0, 400.0, 200.0))
                .expect("the root is registered")
        };

        assert_eq!(frame(&mut tree), Size::new(400.0, 200.0));
        shown.set(true);
        assert_eq!(frame(&mut tree), Size::new(10.0, 10.0));
        shown.set(false);
        assert_eq!(frame(&mut tree), Size::new(400.0, 200.0));
    }

    /// A visible child that measures nothing still leads: being empty is not
    /// being hidden, and the stack follows it down to zero.
    #[test]
    fn a_visible_empty_child_still_leads() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(fill()).height(fill()))
            .child(container().width(0).height(0));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, 200.0));

        assert_eq!(tree.cached_size(id), Some(Size::new(0.0, 0.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(0.0, 0.0));
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

    /// With no leader on an axis that has no maximum there is no space to
    /// take either, so the stack is as big as its children on that axis — and
    /// every follower is then laid out at that size.
    #[test]
    fn all_followers_on_an_unbounded_axis_take_their_childrens_size() {
        let root = container()
            .layout(ZStack::new())
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .child(container().width(20).height(30)),
            )
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .child(container().width(10).height(40)),
            );

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, f32::INFINITY));

        assert_eq!(tree.cached_size(id), Some(Size::new(400.0, 40.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(400.0, 40.0));
        assert_eq!(child_size(&tree, id, 1), Size::new(400.0, 40.0));
    }

    /// A leader on an unbounded axis is still followed: a background behind a
    /// label in scrolled content is as tall as the label, not its own content.
    #[test]
    fn a_follower_on_an_unbounded_axis_still_follows_its_leader() {
        let root = container()
            .layout(ZStack::new())
            .child(container().width(fill()).height(fill()))
            .child(container().width(80).height(20));

        let (tree, id) = layout_root(root, Constraints::new(0.0, 0.0, 400.0, f32::INFINITY));

        assert_eq!(tree.cached_size(id), Some(Size::new(80.0, 20.0)));
        assert_eq!(child_size(&tree, id, 0), Size::new(80.0, 20.0));
    }

    /// Lay `root` out at 400×400 a frame at a time, pumping what a write
    /// scheduled, and answer where its first child is.
    fn first_child_per_frame(root: crate::widgets::Container) -> impl FnMut() -> (f32, f32) {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(root));
        tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
        move || {
            crate::jobs::pump_and_layout(&mut tree, id, Constraints::new(0.0, 0.0, 400.0, 400.0))
                .expect("the root is registered");
            tree.get_origin(tree.get_children(id)[0]).expect("laid out")
        }
    }

    /// The alignment a child declares is a property like any other: a write
    /// moves the child on the next frame.
    #[test]
    fn a_child_moves_when_its_own_alignment_changes() {
        let alignment = crate::reactive::create_signal(Alignment::TOP_START);
        let mut frame = first_child_per_frame(
            container()
                .width(100)
                .height(60)
                .layout(ZStack::new())
                .child(container().width(40).height(20).align_self(alignment)),
        );
        assert_eq!(frame(), (0.0, 0.0));
        alignment.set(Alignment::BOTTOM_END);
        assert_eq!(frame(), (60.0, 40.0));
    }

    /// And so is the stack's.
    #[test]
    fn a_stack_moves_its_children_when_its_alignment_changes() {
        let alignment = crate::reactive::create_signal(Alignment::TOP_START);
        let mut frame = first_child_per_frame(
            container()
                .width(100)
                .height(60)
                .layout(ZStack::new().align(alignment))
                .child(container().width(40).height(20)),
        );
        assert_eq!(frame(), (0.0, 0.0));
        alignment.set(Alignment::CENTER);
        assert_eq!(frame(), (30.0, 20.0));
    }
}
