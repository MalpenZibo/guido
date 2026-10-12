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
        let (main_max, cross_min, cross_max) = match axis {
            Axis::Horizontal => (
                constraints.max_width,
                constraints.min_height,
                constraints.max_height,
            ),
            Axis::Vertical => (
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
        let mut non_fill_main = 0.0f32;
        let mut non_fill_kept = 0usize;
        let mut max_cross = 0.0f32;
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
                non_fill_main += size.main_axis(axis);
                non_fill_kept += usize::from(takes_a_gap(size.main_axis(axis)));
                max_cross = max_cross.max(size.cross_axis(axis));
                self.child_sizes[i] = size;
            }
        }

        // Compute fill distribution. The gaps counted are the ones placement
        // keeps: none beside a child that measured nothing, such as a hidden
        // one. Every fill child is counted as keeping its gap. Placement drops
        // that gap only for a fill child given half a pixel or less, which
        // happens only when the row has next to no room left.
        let total_spacing = gaps(spacing, self.fill_indices.len() + non_fill_kept);
        let per_fill = if !self.fill_indices.is_empty() {
            let remaining = (main_max - non_fill_main - total_spacing).max(0.0);
            remaining / self.fill_indices.len() as f32
        } else {
            0.0
        };

        // Pass 2: layout fill children with tight main-axis constraints
        if !self.fill_indices.is_empty() {
            let fill_constraints = match axis {
                Axis::Horizontal => Constraints {
                    min_width: per_fill,
                    max_width: per_fill,
                    ..child_constraints
                },
                Axis::Vertical => Constraints {
                    min_height: per_fill,
                    max_height: per_fill,
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

        // Calculate final dimensions
        let (main_min, cross_constraint_min) = match axis {
            Axis::Horizontal => (constraints.min_width, constraints.min_height),
            Axis::Vertical => (constraints.min_height, constraints.min_width),
        };

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

        let cross_size = max_cross.max(cross_constraint_min).min(cross_max);

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
                let main_constraint = if child_is_fill { per_fill } else { main_max };
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

        size
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
            .map(|align| {
                let id = tree.register(Box::new(root(align)));
                tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
                frame(tree, id);
                (align, id)
            })
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
}
