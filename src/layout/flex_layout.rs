//! Flexbox-style layout algorithm for rows and columns.
//!
//! This module implements a flexbox-inspired layout system that arranges
//! children along a main axis with configurable alignment and spacing.
//!
//! ## Main Axis vs Cross Axis
//!
//! - **Row**: Main axis is horizontal, cross axis is vertical
//! - **Column**: Main axis is vertical, cross axis is horizontal
//!
//! ## Alignment Options
//!
//! **Main axis** ([`MainAlignment`]):
//! - `Start`, `Center`, `End` - Position children at start/center/end
//! - `SpaceBetween` - Equal space between children, none at edges
//! - `SpaceAround` - Equal space around children (half at edges)
//! - `SpaceEvenly` - Equal space including edges
//!
//! **Cross axis** ([`CrossAlignment`]):
//! - `Start`, `Center`, `End` - Align children along cross axis
//! - `Stretch` - Stretch children to fill cross axis (default)
//!
//! [`Flex::center`] sets both to `Center`.
//!
//! ## Children That Do Not Fit
//!
//! When the children that do not `fill()` are longer than the main axis, those
//! that can be shorter give the overflow up in proportion to their natural
//! sizes, none below the minimum its layout recorded
//! ([`LayoutCtx::set_minimum`](crate::tree::LayoutCtx::set_minimum)): an exact
//! length at any depth keeps it, a text that ellipsizes goes down to its mark.
//! A child held at its minimum leaves the rest to the others, as CSS Flexbox
//! §9.7 resolves flexible lengths. Children that fit are laid out at their
//! natural sizes, once.
//!
//! ## Usage
//!
//! ```no_run
//! # use guido::prelude::*;
//! # let (button_a, button_b, button_c) = (text("a"), text("b"), text("c"));
//! container()
//!     .layout(Flex::row().spacing(8.0).main_alignment(MainAlignment::Center))
//!     .children([button_a, button_b, button_c]);
//! ```

use super::{Axis, Constraints, CrossAlignment, Layout, MainAlignment, Size};
use crate::{
    reactive::{IntoSignal, Prop},
    tree::{LayoutCtx, Tree, WidgetId},
};

/// Children with main-axis size at or below this threshold are treated as
/// invisible for spacing purposes (no gap is added on either side).
pub(crate) const MIN_VISIBLE_SIZE: f32 = 0.5;

/// Whether a child this long on the main axis keeps a gap beside it. One
/// predicate for the fill share and for placement, which have to agree on it.
fn takes_a_gap(main: f32) -> bool {
    main > MIN_VISIBLE_SIZE
}

/// The spacing between `count` children that each keep a gap.
fn gaps(spacing: f32, count: usize) -> f32 {
    spacing * count.saturating_sub(1) as f32
}

/// Flex layout for rows and columns
pub struct Flex {
    direction: Prop<Axis>,
    spacing: Prop<f32>,
    main_alignment: Prop<MainAlignment>,
    cross_alignment: Prop<CrossAlignment>,

    child_sizes: Vec<Size>,
    fill_indices: Vec<usize>,
}

/// One child an overflow can take room from.
#[derive(Clone, Copy)]
struct Share {
    index: usize,
    natural: f32,
    /// Its minimum, never above its natural size.
    floor: f32,
    target: f32,
    frozen: bool,
}

impl Flex {
    /// Create a new flex layout with the given direction
    ///
    /// Default alignments match CSS Flexbox:
    /// - `main_alignment`: `Start` (CSS `justify-content: flex-start`)
    /// - `cross_alignment`: `Stretch` (CSS `align-items: stretch`)
    pub fn new<M>(direction: impl IntoSignal<Axis, M>) -> Self {
        Self {
            direction: direction.into_prop(),
            spacing: Prop::Unset,
            main_alignment: Prop::Unset,
            cross_alignment: Prop::Unset,
            child_sizes: Vec::with_capacity(8),
            fill_indices: Vec::new(),
        }
    }

    /// Create a row layout
    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Create a column layout
    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Set the spacing between children
    pub fn spacing<M>(mut self, spacing: impl IntoSignal<f32, M>) -> Self {
        self.spacing = spacing.into_prop();
        self
    }

    /// Set the main axis alignment
    pub fn main_alignment<M>(mut self, alignment: impl IntoSignal<MainAlignment, M>) -> Self {
        self.main_alignment = alignment.into_prop();
        self
    }

    /// Set the cross axis alignment
    pub fn cross_alignment<M>(mut self, alignment: impl IntoSignal<CrossAlignment, M>) -> Self {
        self.cross_alignment = alignment.into_prop();
        self
    }

    /// Centre the children on both axes: shorthand for
    /// [`main_alignment(MainAlignment::Center)`](Self::main_alignment) and
    /// [`cross_alignment(CrossAlignment::Center)`](Self::cross_alignment).
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// container()
    ///     .layout(Flex::column().spacing(4.0).center())
    ///     .children([text("Locked"), text("Type your password")]);
    /// ```
    pub fn center(self) -> Self {
        self.main_alignment(MainAlignment::Center)
            .cross_alignment(CrossAlignment::Center)
    }

    /// Calculate initial offset and spacing between children based on main axis alignment
    fn calc_main_axis_spacing(
        &self,
        main_align: MainAlignment,
        spacing: f32,
        free_space: f32,
        child_count: usize,
    ) -> (f32, f32) {
        match main_align {
            MainAlignment::Start => (0.0, spacing),
            MainAlignment::Center => (free_space / 2.0, spacing),
            MainAlignment::End => (free_space, spacing),
            MainAlignment::SpaceBetween => {
                if child_count > 1 {
                    (0.0, free_space / (child_count - 1) as f32 + spacing)
                } else {
                    (0.0, spacing)
                }
            }
            // With no child that keeps a gap — no child at all, or every child
            // hidden or half a pixel or less along the axis — `SpaceAround`
            // and `SpaceEvenly` divide as if there were one, so what they hold
            // is centred in the free space, as CSS centres a lone item (#697).
            MainAlignment::SpaceAround => {
                let space = free_space / child_count.max(1) as f32;
                (space / 2.0, space + spacing)
            }
            MainAlignment::SpaceEvenly => {
                let space = free_space / (child_count.max(1) + 1) as f32;
                (space, space + spacing)
            }
        }
    }

    /// Take the overflow of the children that are not filling out of those
    /// that can give room up, in proportion to their natural sizes, and lay
    /// each one that gave some up out again at what it kept.
    ///
    /// CSS Flexbox §9.7: a child whose share would take it below its minimum
    /// is held at the minimum, and the rest share what is left, until no
    /// share goes below one. When even the minimums do not fit, every child
    /// ends at its minimum and the row overflows, as it did before.
    ///
    /// Answers the most each child may now be, by index, for the stretch that
    /// lays the children out again. Not kept on the flex, which every
    /// container holds: one whose children fit would carry it for nothing.
    fn share_overflow(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        offered: Constraints,
        axis: Axis,
        room: f32,
    ) -> Vec<f32> {
        let mut shares = Vec::new();
        let mut held = 0.0f32;
        for (index, &child) in children.iter().enumerate() {
            let natural = self.child_sizes[index].main_axis(axis);
            if self.fill_indices.binary_search(&index).is_ok() || natural <= 0.0 {
                continue;
            }
            let floor = ctx
                .tree_ref()
                .minimum(child)
                .map_or(natural, |least| least.main_axis(axis));
            if floor >= natural {
                held += natural;
                continue;
            }
            shares.push(Share {
                index,
                natural,
                floor,
                target: natural,
                frozen: false,
            });
        }

        let mut free_natural: f32 = shares.iter().map(|share| share.natural).sum();
        loop {
            let overflow = held + free_natural - room;
            if free_natural <= 0.0 || overflow <= 0.0 {
                break;
            }
            // Every share of a round is taken from the same overflow, and only
            // then are the ones under their minimums held: holding one part
            // way through must not make the shares after it larger.
            let (mut newly_held, mut released) = (0.0f32, 0.0f32);
            for share in shares.iter_mut().filter(|share| !share.frozen) {
                share.target = share.natural - overflow * share.natural / free_natural;
                if share.target < share.floor {
                    share.target = share.floor;
                    share.frozen = true;
                    newly_held += share.floor;
                    released += share.natural;
                }
            }
            if released == 0.0 {
                break;
            }
            held += newly_held;
            free_natural -= released;
        }

        let whole = match axis {
            Axis::Horizontal => offered.max_width,
            Axis::Vertical => offered.max_height,
        };
        let mut kept = vec![whole; children.len()];
        for share in &shares {
            if share.target >= share.natural {
                continue;
            }
            kept[share.index] = share.target;
            let constraints = match axis {
                Axis::Horizontal => Constraints {
                    max_width: share.target,
                    ..offered
                },
                Axis::Vertical => Constraints {
                    max_height: share.target,
                    ..offered
                },
            };
            if let Some(size) = ctx.layout_child(children[share.index], constraints) {
                self.child_sizes[share.index] = size;
            }
        }
        kept
    }

    /// The main-axis length of the children laid out so far, how many of them
    /// keep a gap, and the longest across.
    fn tally(&self, axis: Axis) -> (f32, usize, f32) {
        self.child_sizes
            .iter()
            .fold((0.0, 0, 0.0), |(main, kept, cross), size| {
                (
                    main + size.main_axis(axis),
                    kept + usize::from(takes_a_gap(size.main_axis(axis))),
                    f32::max(cross, size.cross_axis(axis)),
                )
            })
    }

    /// Layout children along the given axis
    fn layout_axis(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
        axis: Axis,
    ) -> Size {
        let spacing = self.spacing.get_or(0.0);
        let main_align = self.main_alignment.get_or(MainAlignment::Start);
        let cross_align = self.cross_alignment.get_or(CrossAlignment::Stretch);

        // Get main/cross axis constraints based on direction
        let (main_min, main_max, cross_min, cross_max) = match axis {
            Axis::Horizontal => (
                constraints.min_width,
                constraints.max_width,
                constraints.min_height,
                constraints.max_height,
            ),
            Axis::Vertical => (
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            ),
        };

        // For Stretch alignment, use min constraint if set
        let stretch_cross = if cross_align == CrossAlignment::Stretch && cross_min > 0.0 {
            Some(cross_min)
        } else {
            None
        };

        let child_constraints = match axis {
            Axis::Horizontal => Constraints {
                min_width: 0.0,
                min_height: stretch_cross.unwrap_or(0.0),
                max_width: main_max,
                max_height: stretch_cross.unwrap_or(cross_max),
            },
            Axis::Vertical => Constraints {
                min_width: stretch_cross.unwrap_or(0.0),
                min_height: 0.0,
                max_width: stretch_cross.unwrap_or(cross_max),
                max_height: main_max,
            },
        };

        // Pre-allocate child_sizes
        self.child_sizes.clear();
        self.child_sizes.resize(children.len(), Size::zero());

        // Pass 1: layout non-fill children and collect fill child indices
        self.fill_indices.clear();

        for (i, &child_id) in children.iter().enumerate() {
            let is_fill = ctx
                .tree_ref()
                .with_widget(child_id, |w| match axis {
                    Axis::Horizontal => w.layout_hints().fill_width,
                    Axis::Vertical => w.layout_hints().fill_height,
                })
                .unwrap_or(false);

            if is_fill {
                self.fill_indices.push(i);
            } else if let Some(size) = ctx.layout_child(child_id, child_constraints) {
                self.child_sizes[i] = size;
            }
        }
        let (mut non_fill_main, mut non_fill_kept, mut max_cross) = self.tally(axis);

        // Compute fill distribution. The gaps counted are the ones placement
        // keeps: none beside a child that measured nothing, such as a hidden
        // one. Every fill child is counted as keeping its gap. Placement drops
        // that gap only for a fill child given half a pixel or less, which
        // happens only when the row has next to no room left.
        let mut total_spacing = gaps(spacing, self.fill_indices.len() + non_fill_kept);
        // An axis with no maximum — a scrolled one — has nothing to fill: a
        // fill child is offered its share of the minimum, which is nothing
        // unless the scroller fills its viewport, and grows past it to its
        // content's size, as CSS sizes `flex-grow` in a container of
        // indefinite size.
        let bounded = main_max.is_finite();

        // Children that do not fit give room up, down to their minimums. Only
        // here is any child laid out again, so a row that fits costs what it
        // always did.
        let mut kept = Vec::new();
        if bounded && non_fill_main + total_spacing > main_max {
            kept = self.share_overflow(
                ctx,
                children,
                child_constraints,
                axis,
                main_max - total_spacing,
            );
            (non_fill_main, non_fill_kept, max_cross) = self.tally(axis);
            total_spacing = gaps(spacing, self.fill_indices.len() + non_fill_kept);
        }
        let room = if bounded { main_max } else { main_min };
        let per_fill = if !self.fill_indices.is_empty() {
            let remaining = (room - non_fill_main - total_spacing).max(0.0);
            remaining / self.fill_indices.len() as f32
        } else {
            0.0
        };
        let fill_max = if bounded { per_fill } else { f32::INFINITY };

        // Pass 2: layout fill children with tight main-axis constraints
        if !self.fill_indices.is_empty() {
            let fill_constraints = match axis {
                Axis::Horizontal => Constraints {
                    min_width: per_fill,
                    max_width: fill_max,
                    ..child_constraints
                },
                Axis::Vertical => Constraints {
                    min_height: per_fill,
                    max_height: fill_max,
                    ..child_constraints
                },
            };

            for &i in &self.fill_indices {
                let child_id = children[i];
                if let Some(size) = ctx.layout_child(child_id, fill_constraints) {
                    max_cross = max_cross.max(size.cross_axis(axis));
                    self.child_sizes[i] = size;
                }
            }
        }

        // Compute total main-axis usage
        let mut children_main: f32 = self.child_sizes.iter().map(|s| s.main_axis(axis)).sum();

        // Baseline alignment needs the deepest baseline in the row before it
        // can place anything: every child shifts down by the difference
        // between it and its own. A child shifted down takes its depth below
        // its baseline with it, so the row is as deep as the deepest baseline
        // plus the greatest of those depths, which is how CSS sizes a line box.
        let max_baseline = if cross_align == CrossAlignment::Baseline && axis == Axis::Horizontal {
            let mut max_depth = 0.0f32;
            let mut max_baseline = 0.0f32;
            for (i, &id) in children.iter().enumerate() {
                let child_cross = self.child_sizes[i].cross_axis(axis);
                let baseline = baseline_of(ctx.tree_ref(), id, child_cross);
                max_baseline = max_baseline.max(baseline);
                max_depth = max_depth.max(child_cross - baseline);
            }
            max_cross = max_cross.max(max_baseline + max_depth);
            max_baseline
        } else {
            0.0
        };

        let cross_size = max_cross.max(cross_min).min(cross_max);

        // For Stretch: if we didn't have a known cross size before, re-layout children
        // with the computed cross size. Fill children get tight main-axis constraints.
        //
        // Skip children that already match cross_size — re-constraining them to their
        // own size is a no-op for sizing but creates tight constraints that turn them
        // into relayout boundaries, preventing dynamic content changes from propagating
        // up to recompute cross_size.
        if cross_align == CrossAlignment::Stretch && stretch_cross.is_none() && cross_size > 0.0 {
            children_main = 0.0;
            let mut fill_cursor = 0;
            for (i, &child_id) in children.iter().enumerate() {
                if (self.child_sizes[i].cross_axis(axis) - cross_size).abs() < 0.5 {
                    children_main += self.child_sizes[i].main_axis(axis);
                    if fill_cursor < self.fill_indices.len() && self.fill_indices[fill_cursor] == i
                    {
                        fill_cursor += 1;
                    }
                    continue;
                }
                let child_is_fill =
                    fill_cursor < self.fill_indices.len() && self.fill_indices[fill_cursor] == i;
                if child_is_fill {
                    fill_cursor += 1;
                }
                // A child that gave room up is offered what it kept, or the
                // stretch would undo the share.
                let main_constraint = if child_is_fill {
                    fill_max
                } else {
                    kept.get(i).copied().unwrap_or(main_max)
                };
                let stretch_constraints = match axis {
                    Axis::Horizontal => Constraints {
                        min_width: if child_is_fill { per_fill } else { 0.0 },
                        min_height: cross_size,
                        max_width: main_constraint,
                        max_height: cross_size,
                    },
                    Axis::Vertical => Constraints {
                        min_width: cross_size,
                        min_height: if child_is_fill { per_fill } else { 0.0 },
                        max_width: cross_size,
                        max_height: main_constraint,
                    },
                };
                if let Some(size) = ctx.layout_child(child_id, stretch_constraints) {
                    children_main += size.main_axis(axis);
                    self.child_sizes[i] = size;
                }
            }
        }

        // Collapse spacing around zero-sized children: a child with zero
        // main-axis size should not contribute a gap on either side.
        let visible_count = self
            .child_sizes
            .iter()
            .filter(|s| takes_a_gap(s.main_axis(axis)))
            .count();
        let visible_spacing = gaps(spacing, visible_count);
        let visible_total_main = children_main + visible_spacing;
        // The spacing alignments fill the main axis, unless it has no maximum
        // — a scrolled one — and then there is no free space to share: they
        // take their content's size, as `Start` does.
        let main_size = match main_align {
            MainAlignment::SpaceBetween
            | MainAlignment::SpaceAround
            | MainAlignment::SpaceEvenly
                if main_max.is_finite() =>
            {
                main_max
            }
            _ => visible_total_main.max(main_min).min(main_max),
        };

        let (width, height) = match axis {
            Axis::Horizontal => (main_size, cross_size),
            Axis::Vertical => (cross_size, main_size),
        };
        let size = Size::new(width, height);

        // Position children
        let free_space = (main_size - children_main - visible_spacing).max(0.0);

        let (initial_offset, between_spacing) =
            self.calc_main_axis_spacing(main_align, spacing, free_space, visible_count);

        let mut main_pos = match axis {
            Axis::Horizontal => origin.0,
            Axis::Vertical => origin.1,
        } + initial_offset;

        let mut prev_nonzero = false;
        for (i, &child_id) in children.iter().enumerate() {
            let child_size = self.child_sizes[i];
            let child_main = child_size.main_axis(axis);
            let child_cross = child_size.cross_axis(axis);

            // Add spacing only between consecutive non-zero-sized children
            if prev_nonzero && takes_a_gap(child_main) {
                main_pos += between_spacing;
            }

            let cross_pos = match cross_align {
                CrossAlignment::Start => match axis {
                    Axis::Horizontal => origin.1,
                    Axis::Vertical => origin.0,
                },
                CrossAlignment::Center => match axis {
                    Axis::Horizontal => origin.1 + (cross_size - child_cross) / 2.0,
                    Axis::Vertical => origin.0 + (cross_size - child_cross) / 2.0,
                },
                CrossAlignment::End => match axis {
                    Axis::Horizontal => origin.1 + cross_size - child_cross,
                    Axis::Vertical => origin.0 + cross_size - child_cross,
                },
                CrossAlignment::Stretch => match axis {
                    Axis::Horizontal => origin.1,
                    Axis::Vertical => origin.0,
                },
                CrossAlignment::Baseline => match axis {
                    Axis::Horizontal => {
                        origin.1 + max_baseline - baseline_of(ctx.tree_ref(), child_id, child_cross)
                    }
                    // A column has no shared line to sit on.
                    Axis::Vertical => origin.0,
                },
            };

            let (x, y) = match axis {
                Axis::Horizontal => (main_pos, cross_pos),
                Axis::Vertical => (cross_pos, main_pos),
            };

            ctx.tree().set_origin(child_id, x, y);
            main_pos += child_main;

            if takes_a_gap(child_main) {
                prev_nonzero = true;
            }
        }

        let least = Self::least(ctx.tree_ref(), children, axis, spacing);
        ctx.set_minimum(least);
        size
    }

    /// The least the children can be arranged in: along the axis, each one's
    /// minimum and the gaps those that keep a length keep between them;
    /// across it, the largest one's. A child's minimum is never more than
    /// the size it was laid out at, so this is never more than the content.
    fn least(tree: &Tree, children: &[WidgetId], axis: Axis, spacing: f32) -> Size {
        let mut along = 0.0f32;
        let mut across = 0.0f32;
        let mut kept = 0usize;
        for &child in children {
            let least = tree.minimum(child).unwrap_or_default();
            along += least.main_axis(axis);
            kept += usize::from(takes_a_gap(least.main_axis(axis)));
            across = across.max(least.cross_axis(axis));
        }
        along += gaps(spacing, kept);
        match axis {
            Axis::Horizontal => Size::new(along, across),
            Axis::Vertical => Size::new(across, along),
        }
    }
}

impl Layout for Flex {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        // Never unset — `new` always gives one — so the default is a formality.
        let direction = self.direction.get_or(Axis::Vertical);
        self.layout_axis(ctx, children, constraints, origin, direction)
    }
}

/// Where a child sits on its baseline, for the purpose of lining a row up.
///
/// Leaves that draw text report one during layout. Anything else — a box, an
/// image — is treated as sitting on its own bottom edge, which is how CSS
/// aligns a baseline-less box among text.
fn baseline_of(tree: &Tree, id: WidgetId, cross_size: f32) -> f32 {
    tree.baseline(id).unwrap_or(cross_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs;
    use crate::layout::fill;
    use crate::reactive::create_signal;
    use crate::widgets::{Container, Scroll, container};

    fn frame(tree: &mut Tree, root: WidgetId) -> Size {
        jobs::pump_and_layout(tree, root, Constraints::new(0.0, 0.0, 400.0, 400.0))
            .expect("the root is registered")
    }

    /// A label that is hidden takes its baseline with it.
    ///
    /// A big label beside a box 10 tall sets the row's baseline well below the
    /// box's top. Once the label is hidden the box is all the row holds, so the
    /// row is the box's height and the box sits at its top. A baseline kept
    /// from the label's last layout would hold both where the label put them.
    #[test]
    fn a_hidden_label_leaves_no_baseline_behind() {
        crate::load_font(include_bytes!("../../tests/assets/DejaVuSansMono.ttf").to_vec());
        let shown = create_signal(true);

        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .layout(Flex::row().cross_alignment(CrossAlignment::Baseline))
                .child(
                    container().visible(shown).child(
                        crate::widgets::text("Ag")
                            .font_size(24.0)
                            .font_family(crate::widgets::FontFamily::name("DejaVu Sans Mono")),
                    ),
                )
                .child(container().width(20.0).height(10.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        frame(&mut tree, root);
        let swatch = tree.get_children(root)[1];
        assert!(
            tree.get_origin(swatch).expect("laid out").1 > 0.0,
            "while the label shows, the box is pushed down to its baseline"
        );

        shown.set(false);
        let row = frame(&mut tree, root);
        assert_eq!(row.height, 10.0, "the box is all the row holds");
        assert_eq!(
            tree.get_origin(swatch).expect("laid out").1,
            0.0,
            "with nothing else on the line, the box sits at the top"
        );
    }

    /// A row records the least its children can be arranged in, with the
    /// gaps those that keep a length keep between them, and the tallest of
    /// them across.
    ///
    /// The button is padding round an exact box, so it cannot be less than
    /// both; the clipping panel can be nothing, and keeps no gap; the swatch
    /// has an exact width.
    #[test]
    fn a_row_records_the_least_its_children_can_be() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .layout(Flex::row().spacing(4.0))
                .child(
                    container()
                        .padding(4.0)
                        .child(container().width(16.0).height(16.0)),
                )
                .child(
                    container()
                        .overflow(crate::widgets::Overflow::Hidden)
                        .child(container().width(100.0).height(40.0)),
                )
                .child(container().width(10.0).height(10.0)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);

        assert_eq!(tree.minimum(root), Some(Size::new(24.0 + 4.0 + 10.0, 24.0)));
    }

    /// A label stretched to the height of the row it is in is as short as its
    /// lines, not as the row: a column around the row could otherwise never
    /// shrink it.
    #[test]
    fn a_stretched_label_records_the_height_of_its_lines() {
        crate::load_font(include_bytes!("../../tests/assets/DejaVuSansMono.ttf").to_vec());
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .layout(Flex::row())
                .child(container().width(10.0).height(100.0))
                .child(
                    crate::widgets::text("label")
                        .font_size(14.0)
                        .font_family(crate::widgets::FontFamily::name("DejaVu Sans Mono")),
                ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        frame(&mut tree, root);

        let label = tree.get_children(root)[1];
        assert_eq!(tree.get_bounds(label).expect("laid out").height, 100.0);
        let least = tree.minimum(label).expect("laid out").height;
        assert!(least < 20.0, "one line is the least it can be, got {least}");
    }

    /// A minimum is recorded with the size it belongs to: a layout skipped as
    /// clean answers the one it recorded, and a layout that runs again
    /// records a new one.
    #[test]
    fn a_recorded_minimum_survives_a_skip_and_follows_a_change() {
        crate::load_font(include_bytes!("../../tests/assets/DejaVuSansMono.ttf").to_vec());
        let label = create_signal(String::from("abc defgh"));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container().layout(Flex::row()).child(
                crate::widgets::text(label)
                    .font_size(14.0)
                    .font_family(crate::widgets::FontFamily::name("DejaVu Sans Mono")),
            ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let glyph = 168.574_22 / 20.0;
        let least = |tree: &Tree| tree.minimum(root).expect("laid out").width;

        frame(&mut tree, root);
        assert!((least(&tree) - 5.0 * glyph).abs() < 0.01, "the widest word");

        frame(&mut tree, root);
        assert!(
            (least(&tree) - 5.0 * glyph).abs() < 0.01,
            "a skipped layout still answers it"
        );

        label.set(String::from("abc defghijk"));
        frame(&mut tree, root);
        assert!(
            (least(&tree) - 8.0 * glyph).abs() < 0.01,
            "the content changed, and so did the widest word"
        );
    }

    /// A minimum is recorded again once what it was worked out from changes.
    ///
    /// A container that does not clip, holding a label that neither wraps nor
    /// ellipsizes, cannot be narrower than the label, so in a row that
    /// overflows it keeps the label's width and the title beside it gives the
    /// room up. When the label grows, the container's minimum has to grow
    /// with it: a minimum kept from before would let the row squash the
    /// container below the label it now holds.
    #[test]
    fn a_minimum_follows_the_content_it_was_measured_from() {
        crate::load_font(include_bytes!("../../tests/assets/DejaVuSansMono.ttf").to_vec());
        let label = create_signal(String::from("abcdefgh"));
        let style = |t: crate::widgets::Text| {
            t.font_size(14.0)
                .font_family(crate::widgets::FontFamily::name("DejaVu Sans Mono"))
                .nowrap()
        };

        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container()
                .width(200.0)
                .layout(Flex::row())
                .child(container().child(style(crate::widgets::text(label))))
                .child(
                    style(crate::widgets::text("abcdefghijklmnopqrst"))
                        .overflow(crate::widgets::TextOverflow::Ellipsis),
                ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let holder = tree.get_children(root)[0];
        let glyph = 168.574_22 / 20.0;

        frame(&mut tree, root);
        let before = tree.get_bounds(holder).expect("laid out").width;
        assert!((before - 8.0 * glyph).abs() < 0.01, "got {before}");

        label.set(String::from("abcdefghij"));
        frame(&mut tree, root);
        let after = tree.get_bounds(holder).expect("laid out").width;
        assert!(
            (after - 10.0 * glyph).abs() < 0.01,
            "the container was held at a minimum it has outgrown: {after}"
        );
    }

    /// A row becomes a column on a write, with the same widgets in the tree.
    ///
    /// The direction is read on every pass, and a closure lands as a signal —
    /// only a constant is held as a value, which keeps `container()` from
    /// claiming a slot. Asserted on the children's origins rather
    /// than on the container's size, because two boxes of the same size in a
    /// square make the row and the column the same size and only the placement
    /// tells them apart.
    #[test]
    fn a_row_becomes_a_column_when_its_direction_says_so() {
        let horizontal = create_signal(true);
        let axis = move || {
            if horizontal.get() {
                Axis::Horizontal
            } else {
                Axis::Vertical
            }
        };

        let mut tree = Tree::new();
        let root = tree.register(Box::new(container().layout(Flex::new(axis)).children(
            vec![
                container().width(20.0).height(20.0),
                container().width(20.0).height(20.0),
            ],
        )));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        frame(&mut tree, root);
        let kids = tree.get_children(root).to_vec();
        assert_eq!(kids.len(), 2, "two children, laid out");
        let second = tree.get_origin(kids[1]).expect("laid out");
        assert!(
            second.0 > 0.0 && second.1 == 0.0,
            "a row puts the second child to the right, got {second:?}"
        );

        horizontal.set(false);
        frame(&mut tree, root);

        let same_kids = tree.get_children(root).to_vec();
        assert_eq!(same_kids, kids, "the children were rebuilt, not relaid out");
        let second = tree.get_origin(kids[1]).expect("laid out");
        assert!(
            second.1 > 0.0 && second.0 == 0.0,
            "a column puts the second child below, got {second:?}"
        );
    }

    const SPACINGS: [MainAlignment; 3] = [
        MainAlignment::SpaceBetween,
        MainAlignment::SpaceAround,
        MainAlignment::SpaceEvenly,
    ];

    fn two_boxes() -> Vec<Container> {
        vec![
            container().width(10.0).height(10.0),
            container().width(10.0).height(10.0),
        ]
    }

    /// Lays out one root per alignment, side by side in one tree, and returns
    /// each root. One tree, because a tree that is dropped leaves jobs behind
    /// for ids the next one would reuse.
    fn laid_out(
        tree: &mut Tree,
        root: impl Fn(MainAlignment) -> Container,
    ) -> Vec<(MainAlignment, WidgetId)> {
        SPACINGS
            .into_iter()
            .map(|align| (align, laid_out_once(tree, root(align))))
            .collect()
    }

    /// Where the two boxes under `id` landed. A scroller's own scrollbar parts
    /// come after its content, so they are past the first two.
    fn two_origins(tree: &Tree, id: WidgetId) -> Vec<(f32, f32)> {
        tree.get_children(id)[..2]
            .iter()
            .map(|&child| tree.get_origin(child).expect("laid out"))
            .collect()
    }

    /// A scrolled axis has no maximum, so there is no free space to share:
    /// the spacing alignments place their children as `Start` would.
    #[test]
    fn a_scrolled_column_spaces_its_children_as_start_would() {
        let mut tree = Tree::new();
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .scroll(Scroll::vertical())
                .layout(Flex::column().main_alignment(align))
                .children(two_boxes())
        });
        for (align, root) in laid {
            assert_eq!(
                two_origins(&tree, root),
                [(0.0, 0.0), (0.0, 10.0)],
                "{align:?} in a scrolled column"
            );
        }
    }

    /// With no child that keeps a gap there is nothing to space (#697).
    /// `SpaceAround` divided its free space by those no children, so it
    /// placed what it held at infinity, or at NaN where there was no free
    /// space, as on a scrolled axis; `SpaceEvenly` placed it at the far end.
    /// Both now centre it in the free space, as CSS centres a lone item:
    /// hidden children in a column and in a row, hairlines in a column.
    /// `SpaceBetween` starts it, as it did.
    #[test]
    fn a_spacing_alignment_with_nothing_to_space_places_in_reach() {
        let mut tree = Tree::new();
        let hidden = || container().visible(false).width(10.0).height(10.0);

        // Scrolled: no free space, so at the start.
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .scroll(Scroll::vertical())
                .layout(Flex::column().main_alignment(align))
                .child(hidden())
        });
        for (align, root) in laid {
            let origin = tree.get_origin(tree.get_children(root)[0]);
            assert_eq!(origin, Some((0.0, 0.0)), "{align:?} scrolled");
        }

        // Bounded, all hidden: 100 free, nothing to put it around.
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .layout(Flex::column().main_alignment(align))
                .child(hidden())
        });
        for (align, root) in laid {
            let origin = tree.get_origin(tree.get_children(root)[0]);
            let centred = align != MainAlignment::SpaceBetween;
            let expected = if centred { 50.0 } else { 0.0 };
            assert_eq!(origin, Some((0.0, expected)), "{align:?} bounded");
        }

        // In a row the same.
        let laid = laid_out(&mut tree, |align| {
            container()
                .width(100.0)
                .layout(Flex::row().main_alignment(align))
                .child(hidden())
        });
        for (align, root) in laid {
            let origin = tree.get_origin(tree.get_children(root)[0]);
            let centred = align != MainAlignment::SpaceBetween;
            let expected = if centred { 50.0 } else { 0.0 };
            assert_eq!(origin, Some((expected, 0.0)), "{align:?} row");
        }

        // A hairline in a scrolled column: no free space, so at the start.
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .scroll(Scroll::vertical())
                .layout(Flex::column().main_alignment(align))
                .child(container().width(10.0).height(MIN_VISIBLE_SIZE))
        });
        for (align, root) in laid {
            let origin = tree.get_origin(tree.get_children(root)[0]);
            assert_eq!(origin, Some((0.0, 0.0)), "{align:?} scrolled hairline");
        }

        // Two hairlines keep no gap between them, and are centred together.
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .layout(Flex::column().main_alignment(align))
                .child(container().width(10.0).height(MIN_VISIBLE_SIZE))
                .child(container().width(10.0).height(MIN_VISIBLE_SIZE))
        });
        for (align, root) in laid {
            let centred = align != MainAlignment::SpaceBetween;
            let first = if centred {
                (100.0 - 2.0 * MIN_VISIBLE_SIZE) / 2.0
            } else {
                0.0
            };
            assert_eq!(
                two_origins(&tree, root),
                [(0.0, first), (0.0, first + MIN_VISIBLE_SIZE)],
                "{align:?} two hairlines"
            );
        }

        // A hairline keeps no gap either, and is centred like any lone item.
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .layout(Flex::column().main_alignment(align))
                .child(container().width(10.0).height(MIN_VISIBLE_SIZE))
        });
        for (align, root) in laid {
            let origin = tree.get_origin(tree.get_children(root)[0]);
            let centred = align != MainAlignment::SpaceBetween;
            let expected = if centred {
                (100.0 - MIN_VISIBLE_SIZE) / 2.0
            } else {
                0.0
            };
            assert_eq!(origin, Some((0.0, expected)), "{align:?} hairline");
        }
    }

    /// What a scrolled container measures as its content is the column's own
    /// size: its children, not the infinity it was offered.
    #[test]
    fn a_column_on_an_unbounded_axis_is_as_tall_as_its_children() {
        let mut tree = Tree::new();
        let laid = laid_out(&mut tree, |align| {
            container().height(100.0).scroll(Scroll::vertical()).child(
                container()
                    .layout(Flex::column().main_alignment(align))
                    .children(two_boxes()),
            )
        });
        for (align, root) in laid {
            let column = tree.get_children(root)[0];
            let height = tree.get_bounds(column).expect("laid out").height;
            assert_eq!(height, 20.0, "{align:?} column on an unbounded axis");
        }
    }

    /// A column with a height still shares out what its children leave free.
    #[test]
    fn a_bounded_column_shares_its_free_space() {
        let mut tree = Tree::new();
        let laid = laid_out(&mut tree, |align| {
            container()
                .height(100.0)
                .layout(Flex::column().main_alignment(align))
                .children(two_boxes())
        });
        // SpaceEvenly's share, added up in the order layout adds it.
        let even = 80.0 / 3.0;
        let expected = [[0.0, 90.0], [20.0, 70.0], [even, even + 10.0 + even]];
        for ((align, root), [first, second]) in laid.into_iter().zip(expected) {
            assert_eq!(
                two_origins(&tree, root),
                [(0.0, first), (0.0, second)],
                "{align:?} in a column 100 tall"
            );
        }
    }

    /// A box 10 tall and a child that fills the rest, in a column.
    fn box_and_fill() -> Vec<Container> {
        vec![
            container().width(10.0).height(10.0),
            container().width(10.0).height(fill()),
        ]
    }

    fn laid_out_once(tree: &mut Tree, root: Container) -> WidgetId {
        let id = tree.register(Box::new(root));
        tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
        frame(tree, id);
        id
    }

    fn bounds_of(tree: &Tree, id: WidgetId) -> crate::widgets::widget::Rect {
        tree.get_bounds(id).expect("laid out")
    }

    /// A scrolled axis has no maximum, so there is nothing for a fill child to
    /// fill: it is laid out at its content size, which is nothing for an empty
    /// container, and it sits after the box.
    #[test]
    fn a_fill_child_in_a_scrolled_column_is_its_content_size() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical())
                .layout(Flex::column())
                .children(box_and_fill()),
        );
        let fill_child = tree.get_children(root)[1];
        assert_eq!(tree.get_origin(fill_child), Some((0.0, 10.0)));
        assert_eq!(bounds_of(&tree, fill_child).height, 0.0);
    }

    /// A fill child with something in it on an unbounded axis is as tall as
    /// what it holds.
    #[test]
    fn a_fill_child_on_an_unbounded_axis_is_as_tall_as_what_it_holds() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container().height(100.0).scroll(Scroll::vertical()).child(
                container()
                    .height(fill())
                    .child(container().width(10.0).height(30.0)),
            ),
        );
        let fill_child = tree.get_children(root)[0];
        assert_eq!(bounds_of(&tree, fill_child).height, 30.0);
    }

    /// What a scroller measures as its content is the column's own size, and
    /// a fill child in it adds nothing it does not hold.
    #[test]
    fn a_column_holding_a_fill_child_on_an_unbounded_axis_is_finite() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical())
                .child(container().layout(Flex::column()).children(box_and_fill())),
        );
        let column = tree.get_children(root)[0];
        assert_eq!(bounds_of(&tree, column).height, 10.0);
    }

    /// A column with a height still gives its fill child what is left of it.
    #[test]
    fn a_fill_child_in_a_bounded_column_takes_the_remainder() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .layout(Flex::column())
                .children(box_and_fill()),
        );
        let fill_child = tree.get_children(root)[1];
        assert_eq!(tree.get_origin(fill_child), Some((0.0, 10.0)));
        assert_eq!(bounds_of(&tree, fill_child).height, 90.0);
    }

    /// A scroller that fills its viewport gives its content the visible
    /// height as a minimum, and a fill child takes what the box leaves of it.
    #[test]
    fn a_fill_child_in_a_scroller_that_fills_its_viewport_takes_the_rest_of_it() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical().fill_viewport(true))
                .layout(Flex::column())
                .children(box_and_fill()),
        );
        let fill_child = tree.get_children(root)[1];
        assert_eq!(tree.get_origin(fill_child), Some((0.0, 10.0)));
        assert_eq!(bounds_of(&tree, fill_child).height, 90.0);
    }

    /// The same through a column nested in the scroller: a column that fills
    /// is as tall as the viewport, and its fill child takes the rest of it.
    #[test]
    fn a_filling_column_in_a_scroller_that_fills_its_viewport_is_as_tall_as_it() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical().fill_viewport(true))
                .child(
                    container()
                        .height(fill())
                        .layout(Flex::column())
                        .children(box_and_fill()),
                ),
        );
        let column = tree.get_children(root)[0];
        assert_eq!(bounds_of(&tree, column).height, 100.0);
        assert_eq!(bounds_of(&tree, tree.get_children(column)[1]).height, 90.0);
    }

    /// Content taller than the viewport is what scrolls: the minimum is met
    /// already, the fill child is its content size, and the column its own.
    #[test]
    fn content_taller_than_a_filled_viewport_still_scrolls() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical().fill_viewport(true))
                .child(container().height(fill()).layout(Flex::column()).children([
                    container().width(10.0).height(150.0),
                    container().width(10.0).height(fill()),
                ])),
        );
        let column = tree.get_children(root)[0];
        assert_eq!(bounds_of(&tree, column).height, 150.0);
        assert_eq!(bounds_of(&tree, tree.get_children(column)[1]).height, 0.0);
    }

    /// A scroller with no height inside another scroller's content has no
    /// viewport to fill on that axis, and asking it to adds no minimum.
    #[test]
    fn filling_an_unbounded_viewport_adds_no_minimum() {
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container().height(100.0).scroll(Scroll::vertical()).child(
                container()
                    .scroll(Scroll::vertical().fill_viewport(true))
                    .layout(Flex::column())
                    .children(box_and_fill()),
            ),
        );
        let inner = tree.get_children(root)[0];
        assert_eq!(bounds_of(&tree, inner).height, 10.0);
        assert_eq!(bounds_of(&tree, tree.get_children(inner)[1]).height, 0.0);
    }

    /// `fill_viewport` is read under layout tracking, so writing it lays the
    /// content out again.
    #[test]
    fn writing_fill_viewport_lays_the_content_out_again() {
        let fills = create_signal(false);
        let mut tree = Tree::new();
        let root = laid_out_once(
            &mut tree,
            container()
                .height(100.0)
                .scroll(Scroll::vertical().fill_viewport(fills))
                .layout(Flex::column())
                .children(box_and_fill()),
        );
        let fill_child = tree.get_children(root)[1];
        assert_eq!(bounds_of(&tree, fill_child).height, 0.0);

        fills.set(true);
        frame(&mut tree, root);
        assert_eq!(bounds_of(&tree, fill_child).height, 90.0);
    }
}
