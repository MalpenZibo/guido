//! The container's box model: what it offers its children, and how big it
//! ends up.
//!
//! A layout pass asks three questions in order, and this module answers all
//! three:
//!
//! 1. **What did the author declare?** — [`read_box_lengths`] reads padding,
//!    width and height under the container's Layout tracking scope and
//!    resolves fractional lengths against the incoming constraints. From that
//!    point on a fraction is an ordinary exact length that happened to become
//!    known at layout time, so nothing downstream has to know about it.
//! 2. **What do the children get?** — [`child_layout`] subtracts the border,
//!    the padding and any scrollbar gutter, opens the scrolled axis to
//!    infinity so content can exceed the viewport, and reports separately the
//!    finite box the content is seen through, inside the border, padding
//!    included but not the gutter — what scrolling measures its content and
//!    padding against.
//! 3. **How big are we?** — [`resolve_size`] combines the declared length, the
//!    measured content and the shrink policy, then clamps to what the parent
//!    allows.
//!
//! Sizes read through the animation state wherever one exists, which is why
//! several methods here consult `self.anims`: an animating width has to drive
//! both what children are offered and what the parent is told, or the content
//! would jump while the box glides.
//!
//! [`read_box_lengths`]: Container::read_box_lengths
//! [`child_layout`]: Container::child_layout
//! [`resolve_size`]: Container::resolve_size

use super::*;
use crate::finite::FiniteOr;
use crate::layout::{Axis, Extent};
use crate::tree::LayoutCtx;

/// The container's own size declarations, resolved for one layout pass.
#[derive(Clone, Copy)]
pub(super) struct BoxLengths {
    pub padding: Padding,
    /// The border's width: room the box keeps inside its edge, as CSS's
    /// `border-box` does, so the children start inside border and padding.
    pub border: f32,
    pub width: Length,
    pub height: Length,
    /// Read here rather than at the point of use: `overflow` is reactive and
    /// decides whether a box may shrink below its content, so the read has to
    /// happen inside the layout tracking scope like every other length.
    pub overflow: Overflow,
}

impl BoxLengths {
    /// What the box keeps between its edge and its content: the border, and
    /// the padding inside it.
    pub fn inset(&self) -> Padding {
        let (p, b) = (self.padding, self.border);
        Padding {
            top: p.top + b,
            right: p.right + b,
            bottom: p.bottom + b,
            left: p.left + b,
        }
    }
}

/// What the children of one layout pass are laid out into.
pub(super) struct ChildLayout {
    /// What children are offered. A scrolled axis is unbounded here.
    pub constraints: Constraints,
    /// Where the first child starts, inside the border and the padding.
    pub origin: (f32, f32),
    /// The box a scroller shows its content and padding through: the extent
    /// it was offered inside its border, less a reserved gutter, padding
    /// included. Scrolling
    /// compares its content, which counts the padding, against this; compared
    /// with the extent inside the padding instead, the padding counted twice
    /// and a padded scroller ran past its end by it (#721).
    pub seen_through: Size,
}

impl Container {
    /// The declared width and height, read untracked: for the two questions
    /// the layout asks before [`read_box_lengths`](Self::read_box_lengths)
    /// reads the same lengths under tracking, which is what subscribes this
    /// layout to them. Read once, as both questions take them.
    pub(super) fn declared_lengths(&self) -> (Length, Length) {
        (
            self.width.get_or_untracked(Length::default()),
            self.height.get_or_untracked(Length::default()),
        )
    }

    /// Whether this container's size is independent of its children, so a
    /// dirty descendant can stop bubbling here instead of reaching the root.
    ///
    /// Only a descendant: the size still follows this container's own
    /// signals, so its own changes go on to the parent, which places it.
    /// `Tree::mark_needs_layout` never stops at the widget that changed.
    pub(super) fn is_relayout_boundary_for(
        &self,
        (width, height): (Length, Length),
        constraints: Constraints,
    ) -> bool {
        // A layout-affecting animation re-sizes the container every frame, so
        // the parent has to keep repositioning its siblings: not a boundary.
        let has_active_layout_anim = self.anims.as_deref().is_some_and(|declared| {
            declared
                .slots()
                .any(|slot| slot.moves_the_box() && slot.is_animating())
        });
        if has_active_layout_anim {
            return false;
        }

        let has_fixed_width = width.exact_size().is_some();
        let has_fixed_height = height.exact_size().is_some();
        let tight_width = constraints.min_width == constraints.max_width;
        let tight_height = constraints.min_height == constraints.max_height;
        (has_fixed_width || tight_width) && (has_fixed_height || tight_height)
    }

    /// What this box is laid out under: its parent's constraints, except on an
    /// axis whose length is [`natural`](crate::layout::natural), which has
    /// neither a minimum nor a maximum there — so the content is measured
    /// unbounded, and the box is neither cut to the parent nor raised to it.
    pub(super) fn own_constraints(
        (width, height): (Length, Length),
        constraints: Constraints,
    ) -> Constraints {
        let mut own = constraints;
        if width.is_natural() {
            (own.min_width, own.max_width) = (0.0, f32::INFINITY);
        }
        if height.is_natural() {
            (own.min_height, own.max_height) = (0.0, f32::INFINITY);
        }
        own
    }

    /// Read the declared padding, border width and lengths, tracked so a later
    /// write re-runs this layout, with fractions resolved against
    /// `constraints`.
    pub(super) fn read_box_lengths(
        &self,
        ctx: &mut LayoutCtx,
        id: WidgetId,
        constraints: Constraints,
    ) -> BoxLengths {
        let (padding, border, mut width, mut height, overflow) = (
            self.animated_padding(ctx, id),
            self.laid_out_border_width(ctx, id),
            self.width.get_finite_or(Length::default(), id, "width"),
            self.height.get_finite_or(Length::default(), id, "height"),
            self.overflow.get_or(Overflow::Visible),
        );

        width.resolve_fraction(constraints.max_width);
        height.resolve_fraction(constraints.max_height);

        // Event dispatch reads the resolved value from here rather than from
        // the signal, so it costs nothing on the pointer path.
        self.overflow_resolved.set(overflow);

        BoxLengths {
            padding,
            border,
            width,
            height,
            overflow,
        }
    }

    /// The extent this container lays its children out inside, on one axis.
    ///
    /// While a size animation runs on an *exact* length, children follow the
    /// in-flight value so alignment tracks the box as it glides. A shrink-to-fit
    /// length keeps using the declared value instead: feeding the animated size
    /// back in would make the content define the target that defines the
    /// content. Its in-flight value reaches the children in a second layout,
    /// through [`lengths_in_flight`](Self::lengths_in_flight), once the first
    /// has set the target.
    ///
    /// Everything outside the running-animation case falls through to the same
    /// answer, cap included: what children are offered must not depend on
    /// whether an animation happens to be attached.
    ///
    /// An exact length — declared or in flight — is offered through
    /// [`exact_within`], which is what [`resolve_axis`](Self::resolve_axis)
    /// reports it as. Offering the uncut length centres children in a box the
    /// container is not, and measures a scroll viewport the parent refused.
    fn animated_extent(
        &self,
        ctx: &mut LayoutCtx,
        anim: Option<&AnimationState<f32>>,
        length: &Length,
        available: f32,
    ) -> f32 {
        match length.exact_size() {
            Some(exact) => {
                let exact = match anim {
                    Some(anim) if !anim.is_initial() => anim.displayed_in(ctx),
                    _ => exact,
                };
                exact_within(length, exact, available)
            }
            // No maximum is `INFINITY`, so the `min` is the whole of the
            // answer and an unbounded `available` stays unbounded.
            None => length.clamp_max(available),
        }
    }

    /// What the children are offered, where the first one starts, and the
    /// viewport they are seen through.
    pub(super) fn child_layout(
        &self,
        ctx: &mut LayoutCtx,
        lengths: &BoxLengths,
        constraints: Constraints,
    ) -> ChildLayout {
        let inset = lengths.inset();
        let layout_width = self.animated_extent(
            ctx,
            self.anims.as_ref().and_then(|a| a.width()),
            &lengths.width,
            constraints.max_width,
        );
        let layout_height = self.animated_extent(
            ctx,
            self.anims.as_ref().and_then(|a| a.height()),
            &lengths.height,
            constraints.max_height,
        );

        // A reserved gutter is space the content never gets, scrollbar shown
        // or not — that is the point of reserving it.
        let (gutter_width, gutter_height) = match self.scroll_data {
            Some(ref sd)
                if sd.metrics.reserve_gutter
                    && sd.scrollbar_visibility != ScrollbarVisibility::Hidden =>
            {
                let gutter = sd.metrics.width + sd.metrics.margin * 2.0;
                (
                    if self.scroll_axis.allows_vertical() {
                        gutter
                    } else {
                        0.0
                    },
                    if self.scroll_axis.allows_horizontal() {
                        gutter
                    } else {
                        0.0
                    },
                )
            }
            _ => (0.0, 0.0),
        };
        let max_width = (layout_width - inset.horizontal_total() - gutter_width).max(0.0);
        let max_height = (layout_height - inset.vertical_total() - gutter_height).max(0.0);

        // Pass the effective minimum down so alignments like Center and End
        // know how much room they actually have to place children in.
        let min_width = if lengths.width.settled_without_content(max_width) {
            max_width
        } else {
            let effective = lengths
                .width
                .min()
                .unwrap_or(0.0)
                .max(constraints.min_width);
            (effective - inset.horizontal_total())
                .max(0.0)
                .min(max_width)
        };
        let min_height = if lengths.height.settled_without_content(max_height) {
            max_height
        } else {
            let effective = lengths
                .height
                .min()
                .unwrap_or(0.0)
                .max(constraints.min_height);
            (effective - inset.vertical_total())
                .max(0.0)
                .min(max_height)
        };

        // The visible extents, captured before the scrolled axis is opened up.
        let viewport = Size::new(max_width, max_height);
        let border = lengths.border * 2.0;
        let seen_through = Size::new(
            (layout_width - border - gutter_width).max(0.0),
            (layout_height - border - gutter_height).max(0.0),
        );

        // A scrolled axis is unbounded: the content is free to exceed the
        // viewport, which is what there is to scroll through. Filling the
        // viewport makes the visible size its minimum, so a fill child has
        // the rest of it to take — where there is one: a scroller with no
        // maximum of its own has no visible size to make a minimum of.
        let fills = self.scroll_data.as_ref().is_some_and(|sd| sd.fill_viewport);
        let floor_of = |visible: f32| {
            if fills && visible.is_finite() {
                visible
            } else {
                0.0
            }
        };
        let floor = Size::new(floor_of(viewport.width), floor_of(viewport.height));
        let child = match self.scroll_axis {
            ScrollAxis::Vertical => Constraints {
                min_width: 0.0,
                min_height: floor.height,
                max_width,
                max_height: f32::INFINITY,
            },
            ScrollAxis::Horizontal => Constraints {
                min_width: floor.width,
                min_height: 0.0,
                max_width: f32::INFINITY,
                max_height,
            },
            ScrollAxis::Both => Constraints {
                min_width: floor.width,
                min_height: floor.height,
                max_width: f32::INFINITY,
                max_height: f32::INFINITY,
            },
            ScrollAxis::None => Constraints {
                min_width,
                min_height,
                max_width,
                max_height,
            },
        };

        ChildLayout {
            constraints: child,
            // Children sit in local coordinates; the parent places the container.
            origin: (inset.left, inset.top),
            seen_through,
        }
    }

    /// Lay the children out into `child`, and answer with the content's size.
    ///
    /// A layout reads its own reactive properties — Flex's spacing and
    /// alignment, whatever a user-written `Layout` declares — and those reads
    /// belong to this container, which is whose scope the call is already
    /// inside.
    pub(super) fn lay_out_children(&mut self, ctx: &mut LayoutCtx, child: &ChildLayout) -> Size {
        let children = self.children_source.get();
        if children.is_empty() {
            return Size::zero();
        }
        self.layout
            .layout(ctx, children, child.constraints, child.origin)
    }

    /// The lengths this box has on this frame, where a content-sized axis is
    /// in flight: that axis held where the animation has it, as an exact
    /// length is held. `None` where no such axis is in flight.
    ///
    /// What the children are laid out in a second time, once the first layout
    /// has measured the content and set the target from it — the CSS
    /// `interpolate-size` model, where the box animates between two content
    /// sizes and what is inside is laid out in the box as it is. A scrolled
    /// axis is left alone: its children are never laid out against the box.
    pub(super) fn lengths_in_flight(
        &self,
        ctx: &mut LayoutCtx,
        lengths: &BoxLengths,
    ) -> Option<BoxLengths> {
        // A measure reads where the size is going, which is what the content
        // was just laid out for.
        if ctx.measuring() {
            return None;
        }
        let mut in_flight = |axis: Axis, length: Length| {
            if length.exact_size().is_some() || self.scrolls(axis) {
                return None;
            }
            let anim = self.size_anim(axis).filter(|a| a.is_animating())?;
            Some(length.held_at(anim.displayed_in(ctx)))
        };
        let width = in_flight(Axis::Horizontal, lengths.width);
        let height = in_flight(Axis::Vertical, lengths.height);
        (width.is_some() || height.is_some()).then(|| BoxLengths {
            width: width.unwrap_or(lengths.width),
            height: height.unwrap_or(lengths.height),
            ..*lengths
        })
    }

    /// The container's final size, given what its content measured.
    pub(super) fn resolve_size(
        &self,
        ctx: &mut LayoutCtx,
        lengths: &BoxLengths,
        constraints: Constraints,
        content: Size,
    ) -> Size {
        let inset = lengths.inset();
        Size::new(
            self.resolve_axis(
                ctx,
                Axis::Horizontal,
                &lengths.width,
                lengths.overflow,
                content.width + inset.horizontal_total(),
                constraints.min_width,
                constraints.max_width,
            ),
            self.resolve_axis(
                ctx,
                Axis::Vertical,
                &lengths.height,
                lengths.overflow,
                content.height + inset.vertical_total(),
                constraints.min_height,
                constraints.max_height,
            ),
        )
    }

    /// The least this box can be, from the least its content can be: what a
    /// flex whose children do not fit takes it down to.
    ///
    /// An exact length is rigid, at any depth: an icon of a declared width
    /// inside a button keeps it however crowded the row the button is in, and
    /// a natural one is as rigid at the size its content gave it. A
    /// box whose content may run past it — cut off, or scrolled to — need not
    /// hold it, as CSS gives a scroll container no automatic minimum, and goes
    /// down to the minimum it declares. Any other box goes no lower than its
    /// content, padding and border, the rule [`resolve_axis`](Self::resolve_axis)
    /// holds it to. A size in flight is where the animation has it.
    pub(super) fn least_size(&self, lengths: &BoxLengths, content: Size, size: Size) -> Size {
        let least = |axis: Axis, length: &Length, padding: f32, content: f32, size: f32| {
            if self.size_anim(axis).is_some_and(|a| a.is_animating()) {
                return size;
            }
            if let Some(exact) = length.exact_size() {
                return length.clamp(exact);
            }
            if length.is_natural() {
                return size;
            }
            let declared = length.min().unwrap_or(0.0);
            if self.content_may_overflow(axis, lengths.overflow) {
                declared
            } else {
                (content + padding).max(declared)
            }
        };
        let inset = lengths.inset();
        Size::new(
            least(
                Axis::Horizontal,
                &lengths.width,
                inset.horizontal_total(),
                content.width,
                size.width,
            ),
            least(
                Axis::Vertical,
                &lengths.height,
                inset.vertical_total(),
                content.height,
                size.height,
            ),
        )
    }

    /// The animation declared on this box's size along `axis`, if any.
    fn size_anim(&self, axis: Axis) -> Option<&AnimationState<f32>> {
        self.anims.as_ref().and_then(|a| match axis {
            Axis::Horizontal => a.width(),
            Axis::Vertical => a.height(),
        })
    }

    /// Whether what this box holds may run past it along `axis` — cut off, or
    /// scrolled to — so that the box need not be as long as its content.
    fn content_may_overflow(&self, axis: Axis, overflow: Overflow) -> bool {
        overflow == Overflow::Hidden || self.scrolls(axis)
    }

    /// Whether this box scrolls along `axis`.
    fn scrolls(&self, axis: Axis) -> bool {
        match axis {
            Axis::Horizontal => self.scroll_axis.allows_horizontal(),
            Axis::Vertical => self.scroll_axis.allows_vertical(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_axis(
        &self,
        ctx: &mut LayoutCtx,
        axis: Axis,
        length: &Length,
        overflow: Overflow,
        content: f32,
        parent_min: f32,
        parent_max: f32,
    ) -> f32 {
        let anim = self.size_anim(axis);
        let animating = anim.is_some_and(|a| a.is_animating());
        let has_exact = length.exact_size().is_some();

        // Growing back to fit the content is the default; these are the cases
        // where the author asked for a smaller box on purpose.
        let allow_shrink = self.content_may_overflow(axis, overflow) || animating || has_exact;

        let mut size = if let Some(anim) = anim {
            let animated = anim.displayed_in(ctx);
            if allow_shrink {
                animated
            } else {
                content.max(animated)
            }
        } else {
            match length.extent() {
                Extent::Exact(exact) => exact,
                // An axis with no maximum has nothing to fill, so a fill
                // length sizes to its content there, as a fraction does.
                Extent::Fill if parent_max.is_finite() => parent_max,
                Extent::Fill => content,
                // A fraction was made exact by `resolve_fraction` before this
                // ran, so reaching here means the constraints were unbounded
                // and there was no share to take — which sizes to content, as
                // an undeclared length does.
                Extent::Auto | Extent::Fraction(_) | Extent::Natural => content,
            }
        };

        if has_exact {
            return exact_within(length, size, parent_max);
        }

        // min/max apply on top of everything above, fill included.
        size = length.clamp(size);
        if !allow_shrink && anim.is_none() {
            size = size.max(content);
        }
        size.max(parent_min).min(parent_max)
    }
}

/// What an exact length — declared or in flight — comes to inside its parent:
/// held within its own bounds, then cut to the parent's maximum.
///
/// Never raised to the parent's minimum: that is what keeps `.width(60)` at 60
/// inside a stretching parent. One definition, because the children are
/// offered this and the parent is told it, and the two must not disagree.
fn exact_within(length: &Length, size: f32, parent_max: f32) -> f32 {
    length.clamp(size).min(parent_max)
}
