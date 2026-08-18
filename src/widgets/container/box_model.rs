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
//! 2. **What do the children get?** — [`child_layout`] subtracts the padding
//!    and any scrollbar gutter, opens the scrolled axis to infinity so content
//!    can exceed the viewport, and reports the finite viewport separately —
//!    that second value is what scrolling measures its content against.
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
use crate::layout::Axis;

/// The container's own size declarations, resolved for one layout pass.
pub(super) struct BoxLengths {
    pub padding: Padding,
    pub width: Length,
    pub height: Length,
    /// Read here rather than at the point of use: `overflow` is reactive and
    /// decides whether a box may shrink below its content, so the read has to
    /// happen inside the layout tracking scope like every other length.
    pub overflow: Overflow,
}

/// What the children of one layout pass are laid out into.
pub(super) struct ChildLayout {
    /// What children are offered. A scrolled axis is unbounded here.
    pub constraints: Constraints,
    /// Where the first child starts, inside the padding.
    pub origin: (f32, f32),
    /// The extent children are actually *visible* within — finite on every
    /// axis, including a scrolled one. Scrolling compares its content against
    /// this, so it must never be the unbounded value from `constraints`: an
    /// infinite viewport is never exceeded, and a scrollbar that never has
    /// anything to scroll is a scrollbar that never appears.
    pub viewport: Size,
}

impl Container {
    /// Whether this container's size is independent of its children, so a
    /// dirty descendant can stop bubbling here instead of reaching the root.
    pub(super) fn is_relayout_boundary_for(&self, constraints: Constraints) -> bool {
        // A layout-affecting animation re-sizes the container every frame, so
        // the parent has to keep repositioning its siblings: not a boundary.
        let has_active_layout_anim = self.anims.as_ref().is_some_and(|a| {
            a.width.as_ref().is_some_and(|x| x.is_animating())
                || a.height.as_ref().is_some_and(|x| x.is_animating())
                || a.padding.as_ref().is_some_and(|x| x.is_animating())
        });
        if has_active_layout_anim {
            return false;
        }

        // Snapshots: this runs inside the container's own layout, which reads
        // the same signals under tracking right after, so the subscription
        // exists either way.
        let has_fixed_width = self
            .width
            .as_ref()
            .is_some_and(|w| w.get_untracked().exact.is_some());
        let has_fixed_height = self
            .height
            .as_ref()
            .is_some_and(|h| h.get_untracked().exact.is_some());
        let tight_width = constraints.min_width == constraints.max_width;
        let tight_height = constraints.min_height == constraints.max_height;
        (has_fixed_width || tight_width) && (has_fixed_height || tight_height)
    }

    /// Read the declared padding and lengths, tracked so a later write
    /// re-runs this layout, with fractions resolved against `constraints`.
    pub(super) fn read_box_lengths(&self, id: WidgetId, constraints: Constraints) -> BoxLengths {
        let (padding, mut width, mut height, overflow) =
            with_signal_tracking(id, JobType::Layout, || {
                (
                    self.animated_padding(),
                    self.width.as_ref().map(|w| w.get()).unwrap_or_default(),
                    self.height.as_ref().map(|h| h.get()).unwrap_or_default(),
                    self.overflow.get_or(Overflow::Visible),
                )
            });

        if let Some(f) = width.fraction
            && constraints.max_width.is_finite()
        {
            width.exact = Some((constraints.max_width * f).max(0.0));
        }
        if let Some(f) = height.fraction
            && constraints.max_height.is_finite()
        {
            height.exact = Some((constraints.max_height * f).max(0.0));
        }

        BoxLengths {
            padding,
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
    /// content.
    ///
    /// Everything outside the running-animation case falls through to the same
    /// answer, cap included: what children are offered must not depend on
    /// whether an animation happens to be attached.
    fn animated_extent(
        &self,
        anim: Option<&AnimationState<f32>>,
        length: &Length,
        available: f32,
    ) -> f32 {
        if let Some(anim) = anim
            && !anim.is_initial()
            && length.exact.is_some()
        {
            return anim.displayed();
        }
        match length.exact {
            Some(exact) => exact,
            None => match length.max {
                Some(max) => available.min(max),
                None => available,
            },
        }
    }

    /// What the children are offered, where the first one starts, and the
    /// viewport they are seen through.
    pub(super) fn child_layout(
        &self,
        lengths: &BoxLengths,
        constraints: Constraints,
    ) -> ChildLayout {
        let padding = lengths.padding;
        let layout_width = self.animated_extent(
            self.anims.as_ref().and_then(|a| a.width.as_ref()),
            &lengths.width,
            constraints.max_width,
        );
        let layout_height = self.animated_extent(
            self.anims.as_ref().and_then(|a| a.height.as_ref()),
            &lengths.height,
            constraints.max_height,
        );

        let mut max_width = (layout_width - padding.horizontal_total()).max(0.0);
        let mut max_height = (layout_height - padding.vertical_total()).max(0.0);

        // A reserved gutter is space the content never gets, scrollbar shown
        // or not — that is the point of reserving it.
        if let Some(ref sd) = self.scroll_data
            && sd.scrollbar_config.reserve_gutter
            && sd.scrollbar_visibility != ScrollbarVisibility::Hidden
        {
            let gutter = sd.scrollbar_config.width + sd.scrollbar_config.margin * 2.0;
            if self.scroll_axis.allows_vertical() {
                max_width = (max_width - gutter).max(0.0);
            }
            if self.scroll_axis.allows_horizontal() {
                max_height = (max_height - gutter).max(0.0);
            }
        }

        // Pass the effective minimum down so alignments like Center and End
        // know how much room they actually have to place children in.
        let min_width = if lengths.width.exact.is_some() || lengths.width.fill {
            max_width
        } else {
            let effective = lengths.width.min.unwrap_or(0.0).max(constraints.min_width);
            (effective - padding.horizontal_total())
                .max(0.0)
                .min(max_width)
        };
        let min_height = if lengths.height.exact.is_some() || lengths.height.fill {
            max_height
        } else {
            let effective = lengths
                .height
                .min
                .unwrap_or(0.0)
                .max(constraints.min_height);
            (effective - padding.vertical_total())
                .max(0.0)
                .min(max_height)
        };

        // The visible extent, captured before the scrolled axis is opened up.
        let viewport = Size::new(max_width, max_height);

        // A scrolled axis is unbounded: the content is free to exceed the
        // viewport, which is what there is to scroll through.
        let child = match self.scroll_axis {
            ScrollAxis::Vertical => Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width,
                max_height: f32::INFINITY,
            },
            ScrollAxis::Horizontal => Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: f32::INFINITY,
                max_height,
            },
            ScrollAxis::Both => Constraints {
                min_width: 0.0,
                min_height: 0.0,
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
            origin: (padding.left, padding.top),
            viewport,
        }
    }

    /// The container's final size, given what its content measured.
    pub(super) fn resolve_size(
        &self,
        lengths: &BoxLengths,
        constraints: Constraints,
        content: Size,
    ) -> Size {
        Size::new(
            self.resolve_axis(
                Axis::Horizontal,
                &lengths.width,
                lengths.overflow,
                content.width + lengths.padding.horizontal_total(),
                constraints.min_width,
                constraints.max_width,
            ),
            self.resolve_axis(
                Axis::Vertical,
                &lengths.height,
                lengths.overflow,
                content.height + lengths.padding.vertical_total(),
                constraints.min_height,
                constraints.max_height,
            ),
        )
    }

    fn resolve_axis(
        &self,
        axis: Axis,
        length: &Length,
        overflow: Overflow,
        content: f32,
        parent_min: f32,
        parent_max: f32,
    ) -> f32 {
        let anim = self.anims.as_ref().and_then(|a| match axis {
            Axis::Horizontal => a.width.as_ref(),
            Axis::Vertical => a.height.as_ref(),
        });
        let animating = anim.is_some_and(|a| a.is_animating());
        let has_exact = length.exact.is_some();

        // Growing back to fit the content is the default; these are the cases
        // where the author asked for a smaller box on purpose.
        let allow_shrink = overflow == Overflow::Hidden
            || animating
            || has_exact
            || match axis {
                Axis::Horizontal => self.scroll_axis.allows_horizontal(),
                Axis::Vertical => self.scroll_axis.allows_vertical(),
            };

        let mut size = if let Some(anim) = anim {
            let animated = anim.displayed();
            if allow_shrink {
                animated
            } else {
                content.max(animated)
            }
        } else if let Some(exact) = length.exact {
            exact
        } else if length.fill {
            parent_max
        } else {
            content
        };

        // min/max apply on top of everything above, fill included.
        if let Some(min) = length.min {
            size = size.max(min);
        }
        if let Some(max) = length.max {
            size = size.min(max);
        }
        if !allow_shrink && anim.is_none() && !has_exact {
            size = size.max(content);
        }

        // An explicit length answers to the parent's maximum but not to its
        // minimum: that is what keeps `.width(60)` at 60 inside a stretching
        // parent.
        if has_exact {
            size.min(parent_max)
        } else {
            size.max(parent_min).min(parent_max)
        }
    }
}
