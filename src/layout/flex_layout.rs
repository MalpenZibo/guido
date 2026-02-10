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
//! **Main axis** ([`MainAxisAlignment`]):
//! - `Start`, `Center`, `End` - Position children at start/center/end
//! - `SpaceBetween` - Equal space between children, none at edges
//! - `SpaceAround` - Equal space around children (half at edges)
//! - `SpaceEvenly` - Equal space including edges
//!
//! **Cross axis** ([`CrossAxisAlignment`]):
//! - `Start`, `Center`, `End` - Align children along cross axis
//! - `Stretch` - Stretch children to fill cross axis (default)
//!
//! ## Usage
//!
//! ```ignore
//! container()
//!     .layout(Flex::row().spacing(8.0).main_axis_alignment(MainAxisAlignment::Center))
//!     .children([button_a, button_b, button_c])
//! ```

use super::{Axis, Constraints, CrossAxisAlignment, Layout, MainAxisAlignment, Size};
use crate::{
    reactive::{IntoMaybeDyn, MaybeDyn},
    tree::{Tree, WidgetId},
};

/// Flex layout for rows and columns
pub struct Flex {
    direction: MaybeDyn<Axis>,
    spacing: MaybeDyn<f32>,
    main_axis_alignment: MaybeDyn<MainAxisAlignment>,
    cross_axis_alignment: MaybeDyn<CrossAxisAlignment>,

    child_sizes: Vec<Size>,
}

impl Flex {
    /// Create a new flex layout with the given direction
    ///
    /// Default alignments match CSS Flexbox:
    /// - `main_axis_alignment`: `Start` (CSS `justify-content: flex-start`)
    /// - `cross_axis_alignment`: `Stretch` (CSS `align-items: stretch`)
    pub fn new(direction: Axis) -> Self {
        Self {
            direction: MaybeDyn::Static(direction),
            spacing: MaybeDyn::Static(0.0),
            main_axis_alignment: MaybeDyn::Static(MainAxisAlignment::Start),
            cross_axis_alignment: MaybeDyn::Static(CrossAxisAlignment::Stretch),
            child_sizes: Vec::with_capacity(8),
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
    pub fn spacing(mut self, spacing: impl IntoMaybeDyn<f32>) -> Self {
        self.spacing = spacing.into_maybe_dyn();
        self
    }

    /// Set the main axis alignment
    pub fn main_axis_alignment(mut self, alignment: impl IntoMaybeDyn<MainAxisAlignment>) -> Self {
        self.main_axis_alignment = alignment.into_maybe_dyn();
        self
    }

    /// Set the cross axis alignment
    pub fn cross_axis_alignment(
        mut self,
        alignment: impl IntoMaybeDyn<CrossAxisAlignment>,
    ) -> Self {
        self.cross_axis_alignment = alignment.into_maybe_dyn();
        self
    }

    /// Calculate initial offset and spacing between children based on main axis alignment
    fn calc_main_axis_spacing(
        &self,
        main_align: MainAxisAlignment,
        spacing: f32,
        free_space: f32,
        child_count: usize,
    ) -> (f32, f32) {
        match main_align {
            MainAxisAlignment::Start => (0.0, spacing),
            MainAxisAlignment::Center => (free_space / 2.0, spacing),
            MainAxisAlignment::End => (free_space, spacing),
            MainAxisAlignment::SpaceBetween => {
                if child_count > 1 {
                    (0.0, free_space / (child_count - 1) as f32 + spacing)
                } else {
                    (0.0, spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / child_count as f32;
                (space / 2.0, space + spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (child_count + 1) as f32;
                (space, space + spacing)
            }
        }
    }

    /// Layout children along the given axis
    fn layout_axis(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
        axis: Axis,
    ) -> Size {
        let spacing = self.spacing.get();
        let main_align = self.main_axis_alignment.get();
        let cross_align = self.cross_axis_alignment.get();

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

        // Pre-scan fill flags via layout_hints() before any layout
        let is_fill: Vec<bool> = children
            .iter()
            .map(|&id| {
                tree.with_widget(id, |w| match axis {
                    Axis::Horizontal => w.layout_hints().fill_width,
                    Axis::Vertical => w.layout_hints().fill_height,
                })
                .unwrap_or(false)
            })
            .collect();
        let fill_count = is_fill.iter().filter(|&&f| f).count();

        // For Stretch alignment, use min constraint if set
        let stretch_cross = if cross_align == CrossAxisAlignment::Stretch && cross_min > 0.0 {
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

        // Pass 1: layout non-fill children to measure their main-axis contribution
        let mut non_fill_main = 0.0f32;
        let mut max_cross = 0.0f32;

        for (i, &child_id) in children.iter().enumerate() {
            if !is_fill[i]
                && let Some(size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                    widget.layout(tree, id, child_constraints)
                })
            {
                non_fill_main += size.main_axis(axis);
                max_cross = max_cross.max(size.cross_axis(axis));
                self.child_sizes[i] = size;
            }
        }

        // Compute fill distribution
        let total_spacing = if children.len() > 1 {
            spacing * (children.len() - 1) as f32
        } else {
            0.0
        };
        let per_fill = if fill_count > 0 {
            let remaining = (main_max - non_fill_main - total_spacing).max(0.0);
            remaining / fill_count as f32
        } else {
            0.0
        };

        // Pass 2: layout fill children with tight main-axis constraints
        if fill_count > 0 {
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

            for (i, &child_id) in children.iter().enumerate() {
                if is_fill[i]
                    && let Some(size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                        widget.layout(tree, id, fill_constraints)
                    })
                {
                    max_cross = max_cross.max(size.cross_axis(axis));
                    self.child_sizes[i] = size;
                }
            }
        }

        // Compute total main-axis usage
        let mut children_main: f32 = self.child_sizes.iter().map(|s| s.main_axis(axis)).sum();

        let total_main = children_main + total_spacing;

        // Calculate final dimensions
        let (main_min, cross_constraint_min) = match axis {
            Axis::Horizontal => (constraints.min_width, constraints.min_height),
            Axis::Vertical => (constraints.min_height, constraints.min_width),
        };

        // For space-based and centered alignments, expand to fill available main axis
        let main_size = match main_align {
            MainAxisAlignment::SpaceBetween
            | MainAxisAlignment::SpaceAround
            | MainAxisAlignment::SpaceEvenly
            | MainAxisAlignment::Center
            | MainAxisAlignment::End => main_max,
            MainAxisAlignment::Start => total_main.max(main_min).min(main_max),
        };

        let cross_size = max_cross.max(cross_constraint_min).min(cross_max);

        // For Stretch: if we didn't have a known cross size before, re-layout children
        // with the computed cross size. Fill children get tight main-axis constraints.
        if cross_align == CrossAxisAlignment::Stretch && stretch_cross.is_none() && cross_size > 0.0
        {
            self.child_sizes.clear();
            self.child_sizes.resize(children.len(), Size::zero());
            children_main = 0.0;
            for (i, &child_id) in children.iter().enumerate() {
                let main_constraint = if is_fill[i] { per_fill } else { main_max };
                let stretch_constraints = match axis {
                    Axis::Horizontal => Constraints {
                        min_width: if is_fill[i] { per_fill } else { 0.0 },
                        min_height: cross_size,
                        max_width: main_constraint,
                        max_height: cross_size,
                    },
                    Axis::Vertical => Constraints {
                        min_width: cross_size,
                        min_height: if is_fill[i] { per_fill } else { 0.0 },
                        max_width: cross_size,
                        max_height: main_constraint,
                    },
                };
                if let Some(size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                    widget.layout(tree, id, stretch_constraints)
                }) {
                    children_main += size.main_axis(axis);
                    self.child_sizes[i] = size;
                }
            }
        }

        let (width, height) = match axis {
            Axis::Horizontal => (main_size, cross_size),
            Axis::Vertical => (cross_size, main_size),
        };
        let size = Size::new(width, height);

        // Position children
        let free_space = (main_size - children_main - total_spacing).max(0.0);

        let (initial_offset, between_spacing) =
            self.calc_main_axis_spacing(main_align, spacing, free_space, children.len());

        let mut main_pos = match axis {
            Axis::Horizontal => origin.0,
            Axis::Vertical => origin.1,
        } + initial_offset;

        for (i, &child_id) in children.iter().enumerate() {
            let child_size = self.child_sizes[i];
            let child_main = child_size.main_axis(axis);
            let child_cross = child_size.cross_axis(axis);

            let cross_pos = match cross_align {
                CrossAxisAlignment::Start => match axis {
                    Axis::Horizontal => origin.1,
                    Axis::Vertical => origin.0,
                },
                CrossAxisAlignment::Center => match axis {
                    Axis::Horizontal => origin.1 + (cross_size - child_cross) / 2.0,
                    Axis::Vertical => origin.0 + (cross_size - child_cross) / 2.0,
                },
                CrossAxisAlignment::End => match axis {
                    Axis::Horizontal => origin.1 + cross_size - child_cross,
                    Axis::Vertical => origin.0 + cross_size - child_cross,
                },
                CrossAxisAlignment::Stretch => match axis {
                    Axis::Horizontal => origin.1,
                    Axis::Vertical => origin.0,
                },
            };

            let (x, y) = match axis {
                Axis::Horizontal => (main_pos, cross_pos),
                Axis::Vertical => (cross_pos, main_pos),
            };

            tree.set_origin(child_id, x, y);
            main_pos += child_main + between_spacing;
        }

        size
    }
}

impl Layout for Flex {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let direction = self.direction.get();
        self.layout_axis(tree, children, constraints, origin, direction)
    }
}
