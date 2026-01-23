use super::{Axis, Constraints, CrossAxisAlignment, Layout, MainAxisAlignment, Size};
use crate::reactive::{IntoMaybeDyn, MaybeDyn};
use crate::widgets::Widget;

/// Flex layout for rows and columns
pub struct Flex {
    direction: MaybeDyn<Axis>,
    spacing: MaybeDyn<f32>,
    main_axis_alignment: MaybeDyn<MainAxisAlignment>,
    cross_axis_alignment: MaybeDyn<CrossAxisAlignment>,

    // Cached child sizes
    child_sizes: Vec<Size>,
}

impl Flex {
    /// Create a new flex layout with the given direction
    pub fn new(direction: Axis) -> Self {
        Self {
            direction: MaybeDyn::Static(direction),
            spacing: MaybeDyn::Static(0.0),
            main_axis_alignment: MaybeDyn::Static(MainAxisAlignment::Start),
            cross_axis_alignment: MaybeDyn::Static(CrossAxisAlignment::Center),
            child_sizes: Vec::new(),
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
        children: &mut [Box<dyn Widget>],
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

        // First pass: measure all children
        let mut total_main = 0.0f32;
        let mut max_cross = 0.0f32;
        let mut children_main = 0.0f32;

        for child in children.iter_mut() {
            let size = if child.needs_layout() {
                child.layout(child_constraints)
            } else {
                let bounds = child.bounds();
                Size::new(bounds.width, bounds.height)
            };
            let main_size = size.main_axis(axis);
            let cross_size = size.cross_axis(axis);
            total_main += main_size;
            children_main += main_size;
            max_cross = max_cross.max(cross_size);
            self.child_sizes.push(size);
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
            for child in children.iter_mut() {
                child.mark_dirty(crate::reactive::ChangeFlags::NEEDS_LAYOUT);
                let size = child.layout(stretch_constraints);
                children_main += size.main_axis(axis);
                self.child_sizes.push(size);
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

        for (i, child) in children.iter_mut().enumerate() {
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

            child.set_origin(x, y);
            main_pos += child_main + between_spacing;
        }

        size
    }
}

impl Layout for Flex {
    fn layout(
        &mut self,
        children: &mut [Box<dyn Widget>],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let direction = self.direction.get();
        self.layout_axis(children, constraints, origin, direction)
    }
}
