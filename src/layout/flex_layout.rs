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

        self.child_sizes.clear();

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

        // First pass: measure all children with full main-axis constraints.
        // Container children with fill() will expand to main_max and record
        // their fill intent in the tree via set_fills().
        let mut total_main = 0.0f32;
        let mut max_cross = 0.0f32;
        let mut children_main = 0.0f32;

        for &child_id in children.iter() {
            if let Some(size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                widget.layout(tree, id, child_constraints)
            }) {
                let main_size = size.main_axis(axis);
                let cross_size = size.cross_axis(axis);
                total_main += main_size;
                children_main += main_size;
                max_cross = max_cross.max(cross_size);
                self.child_sizes.push(size);
            }
        }

        // Second pass: re-measure fill children with remaining space.
        // After the first pass, Container has called tree.set_fills() for each
        // child, so we can now identify which children want fill behavior and
        // distribute the remaining main-axis space among them.
        let fill_indices: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|&(_, &id)| tree.fills(id, axis))
            .map(|(i, _)| i)
            .collect();

        if !fill_indices.is_empty() {
            let non_fill_main: f32 = self
                .child_sizes
                .iter()
                .enumerate()
                .filter(|(i, _)| !fill_indices.contains(i))
                .map(|(_, s)| s.main_axis(axis))
                .sum();

            let total_spacing = if children.len() > 1 {
                spacing * (children.len() - 1) as f32
            } else {
                0.0
            };
            let remaining = (main_max - non_fill_main - total_spacing).max(0.0);
            let per_fill = remaining / fill_indices.len() as f32;

            let fill_constraints = match axis {
                Axis::Horizontal => Constraints {
                    max_width: per_fill,
                    min_width: per_fill,
                    ..child_constraints
                },
                Axis::Vertical => Constraints {
                    max_height: per_fill,
                    min_height: per_fill,
                    ..child_constraints
                },
            };

            for &i in &fill_indices {
                if let Some(size) = tree.with_widget_mut(children[i], |widget, id, tree| {
                    widget.layout(tree, id, fill_constraints)
                }) {
                    let old_main = self.child_sizes[i].main_axis(axis);
                    total_main -= old_main;
                    children_main -= old_main;
                    total_main += size.main_axis(axis);
                    children_main += size.main_axis(axis);
                    max_cross = max_cross.max(size.cross_axis(axis));
                    self.child_sizes[i] = size;
                }
            }
        }

        // Add spacing
        if !children.is_empty() {
            total_main += spacing * (children.len() - 1) as f32;
        }

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
        if cross_align == CrossAxisAlignment::Stretch && stretch_cross.is_none() && cross_size > 0.0
        {
            self.child_sizes.clear();
            children_main = 0.0; // Reset for re-computation
            let stretch_constraints = match axis {
                Axis::Horizontal => Constraints {
                    min_width: 0.0,
                    min_height: cross_size,
                    max_width: main_max,
                    max_height: cross_size,
                },
                Axis::Vertical => Constraints {
                    min_width: cross_size,
                    min_height: 0.0,
                    max_width: cross_size,
                    max_height: main_max,
                },
            };
            for &child_id in children.iter() {
                if let Some(size) = tree.with_widget_mut(child_id, |widget, id, tree| {
                    widget.layout(tree, id, stretch_constraints)
                }) {
                    children_main += size.main_axis(axis);
                    self.child_sizes.push(size);
                }
            }
        }

        let (width, height) = match axis {
            Axis::Horizontal => (main_size, cross_size),
            Axis::Vertical => (cross_size, main_size),
        };
        let size = Size::new(width, height);

        // Position children
        let total_spacing = if children.len() > 1 {
            spacing * (children.len() - 1) as f32
        } else {
            0.0
        };
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
