use super::{Axis, Constraints, CrossAxisAlignment, Layout, MainAxisAlignment, Size};
use crate::reactive::{IntoMaybeDyn, MaybeDyn};
use crate::widgets::Widget;

/// Flex layout for rows and columns
pub struct Flex {
    direction: MaybeDyn<Axis>,
    spacing: MaybeDyn<f32>,
    main_axis_alignment: MaybeDyn<MainAxisAlignment>,
    cross_axis_alignment: MaybeDyn<CrossAxisAlignment>,

    // Cached values for change detection
    cached_direction: Axis,
    cached_spacing: f32,
    cached_main_align: MainAxisAlignment,
    cached_cross_align: CrossAxisAlignment,

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
            cached_direction: direction,
            cached_spacing: 0.0,
            cached_main_align: MainAxisAlignment::Start,
            cached_cross_align: CrossAxisAlignment::Center,
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

    /// Position children horizontally (row layout)
    fn layout_horizontal(
        &mut self,
        children: &mut [Box<dyn Widget>],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let spacing = self.spacing.get();
        let main_align = self.main_axis_alignment.get();
        let cross_align = self.cross_axis_alignment.get();

        self.cached_spacing = spacing;
        self.cached_main_align = main_align;
        self.cached_cross_align = cross_align;

        self.child_sizes.clear();

        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            max_width: constraints.max_width,
            max_height: constraints.max_height,
        };

        // First pass: measure all children
        let mut total_width = 0.0f32;
        let mut max_height = 0.0f32;

        for child in children.iter_mut() {
            let size = if child.needs_layout() {
                child.layout(child_constraints)
            } else {
                let bounds = child.bounds();
                Size::new(bounds.width, bounds.height)
            };
            total_width += size.width;
            max_height = max_height.max(size.height);
            self.child_sizes.push(size);
        }

        // Add spacing
        if !children.is_empty() {
            total_width += spacing * (children.len() - 1) as f32;
        }

        // For space-based alignments, expand to fill available width
        let width = match main_align {
            MainAxisAlignment::SpaceBetween
            | MainAxisAlignment::SpaceAround
            | MainAxisAlignment::SpaceEvenly => constraints.max_width,
            _ => total_width
                .max(constraints.min_width)
                .min(constraints.max_width),
        };

        let height = max_height
            .max(constraints.min_height)
            .min(constraints.max_height);

        let size = Size::new(width, height);

        // Position children
        let total_spacing = if children.len() > 1 {
            spacing * (children.len() - 1) as f32
        } else {
            0.0
        };
        let children_width: f32 = self.child_sizes.iter().map(|s| s.width).sum();
        let free_space = (size.width - children_width - total_spacing).max(0.0);

        let (initial_offset, between_spacing) = match main_align {
            MainAxisAlignment::Start => (0.0, spacing),
            MainAxisAlignment::Center => (free_space / 2.0, spacing),
            MainAxisAlignment::End => (free_space, spacing),
            MainAxisAlignment::SpaceBetween => {
                if children.len() > 1 {
                    (0.0, free_space / (children.len() - 1) as f32 + spacing)
                } else {
                    (0.0, spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / children.len() as f32;
                (space / 2.0, space + spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (children.len() + 1) as f32;
                (space, space + spacing)
            }
        };

        let mut x = origin.0 + initial_offset;
        for (i, child) in children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let y = match cross_align {
                CrossAxisAlignment::Start => origin.1,
                CrossAxisAlignment::Center => origin.1 + (height - child_size.height) / 2.0,
                CrossAxisAlignment::End => origin.1 + height - child_size.height,
                CrossAxisAlignment::Stretch => origin.1,
            };

            child.set_origin(x, y);
            x += child_size.width + between_spacing;
        }

        size
    }

    /// Position children vertically (column layout)
    fn layout_vertical(
        &mut self,
        children: &mut [Box<dyn Widget>],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let spacing = self.spacing.get();
        let main_align = self.main_axis_alignment.get();
        let cross_align = self.cross_axis_alignment.get();

        self.cached_spacing = spacing;
        self.cached_main_align = main_align;
        self.cached_cross_align = cross_align;

        self.child_sizes.clear();

        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            max_width: constraints.max_width,
            max_height: constraints.max_height,
        };

        // First pass: measure all children
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;

        for child in children.iter_mut() {
            let size = if child.needs_layout() {
                child.layout(child_constraints)
            } else {
                let bounds = child.bounds();
                Size::new(bounds.width, bounds.height)
            };
            max_width = max_width.max(size.width);
            total_height += size.height;
            self.child_sizes.push(size);
        }

        // Add spacing
        if !children.is_empty() {
            total_height += spacing * (children.len() - 1) as f32;
        }

        let width = max_width
            .max(constraints.min_width)
            .min(constraints.max_width);

        let height = total_height
            .max(constraints.min_height)
            .min(constraints.max_height);

        let size = Size::new(width, height);

        // Position children
        let total_spacing = if children.len() > 1 {
            spacing * (children.len() - 1) as f32
        } else {
            0.0
        };
        let children_height: f32 = self.child_sizes.iter().map(|s| s.height).sum();
        let free_space = (size.height - children_height - total_spacing).max(0.0);

        let (initial_offset, between_spacing) = match main_align {
            MainAxisAlignment::Start => (0.0, spacing),
            MainAxisAlignment::Center => (free_space / 2.0, spacing),
            MainAxisAlignment::End => (free_space, spacing),
            MainAxisAlignment::SpaceBetween => {
                if children.len() > 1 {
                    (0.0, free_space / (children.len() - 1) as f32 + spacing)
                } else {
                    (0.0, spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / children.len() as f32;
                (space / 2.0, space + spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (children.len() + 1) as f32;
                (space, space + spacing)
            }
        };

        let mut y = origin.1 + initial_offset;
        for (i, child) in children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let x = match cross_align {
                CrossAxisAlignment::Start => origin.0,
                CrossAxisAlignment::Center => origin.0 + (width - child_size.width) / 2.0,
                CrossAxisAlignment::End => origin.0 + width - child_size.width,
                CrossAxisAlignment::Stretch => origin.0,
            };

            child.set_origin(x, y);
            y += child_size.height + between_spacing;
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
        self.cached_direction = direction;

        match direction {
            Axis::Horizontal => self.layout_horizontal(children, constraints, origin),
            Axis::Vertical => self.layout_vertical(children, constraints, origin),
        }
    }
}
