use crate::layout::{Constraints, Size};
use crate::reactive::{ChangeFlags, WidgetId};
use crate::renderer::PaintContext;

use super::row::{CrossAxisAlignment, MainAxisAlignment};
use super::widget::{Event, EventResponse, Rect, Widget};

pub struct Column {
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,
    children: Vec<Box<dyn Widget>>,
    spacing: f32,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    bounds: Rect,
    child_sizes: Vec<Size>,
}

impl Column {
    /// Check if any child widget needs layout
    fn any_child_needs_layout(&self) -> bool {
        self.children.iter().any(|child| child.needs_layout())
    }

    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            children: Vec::new(),
            spacing: 0.0,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            child_sizes: Vec::new(),
        }
    }

    pub fn with_children(children: Vec<Box<dyn Widget>>) -> Self {
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            children,
            spacing: 0.0,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            child_sizes: Vec::new(),
        }
    }

    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.children.push(Box::new(widget));
        self
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Column {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Only do layout if this widget or any child needs it
        let needs_layout = self.needs_layout() || self.any_child_needs_layout();
        if !needs_layout {
            // Return cached size
            return Size::new(self.bounds.width, self.bounds.height);
        }

        self.child_sizes.clear();

        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            max_width: constraints.max_width,
            max_height: constraints.max_height,
        };

        // First pass: measure all children (only layout dirty children)
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;

        for child in &mut self.children {
            let size = if child.needs_layout() {
                child.layout(child_constraints)
            } else {
                // Use cached bounds
                let bounds = child.bounds();
                Size::new(bounds.width, bounds.height)
            };
            max_width = max_width.max(size.width);
            total_height += size.height;
            self.child_sizes.push(size);
        }

        // Add spacing
        if !self.children.is_empty() {
            total_height += self.spacing * (self.children.len() - 1) as f32;
        }

        let size = Size::new(
            max_width
                .max(constraints.min_width)
                .min(constraints.max_width),
            total_height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        // Position children
        let total_spacing = if self.children.len() > 1 {
            self.spacing * (self.children.len() - 1) as f32
        } else {
            0.0
        };
        let children_height: f32 = self.child_sizes.iter().map(|s| s.height).sum();
        let free_space = (size.height - children_height - total_spacing).max(0.0);

        let (initial_offset, between_spacing) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, self.spacing),
            MainAxisAlignment::Center => (free_space / 2.0, self.spacing),
            MainAxisAlignment::End => (free_space, self.spacing),
            MainAxisAlignment::SpaceBetween => {
                if self.children.len() > 1 {
                    (
                        0.0,
                        free_space / (self.children.len() - 1) as f32 + self.spacing,
                    )
                } else {
                    (0.0, self.spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / self.children.len() as f32;
                (space / 2.0, space + self.spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (self.children.len() + 1) as f32;
                (space, space + self.spacing)
            }
        };

        let mut y = self.bounds.y + initial_offset;
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let x = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => self.bounds.x,
                CrossAxisAlignment::Center => self.bounds.x + (size.width - child_size.width) / 2.0,
                CrossAxisAlignment::End => self.bounds.x + size.width - child_size.width,
                CrossAxisAlignment::Stretch => self.bounds.x,
            };

            child.set_origin(x, y);
            y += child_size.height + between_spacing;
        }

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Paint all children - selective rendering is handled at main loop level
        for child in &self.children {
            child.paint(ctx);
        }
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        for child in &mut self.children {
            if child.event(event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }
        EventResponse::Ignored
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.bounds.x = x;
        self.bounds.y = y;

        // Reposition children
        let total_spacing = if self.children.len() > 1 {
            self.spacing * (self.children.len() - 1) as f32
        } else {
            0.0
        };
        let children_height: f32 = self.child_sizes.iter().map(|s| s.height).sum();
        let free_space = (self.bounds.height - children_height - total_spacing).max(0.0);

        let (initial_offset, between_spacing) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, self.spacing),
            MainAxisAlignment::Center => (free_space / 2.0, self.spacing),
            MainAxisAlignment::End => (free_space, self.spacing),
            MainAxisAlignment::SpaceBetween => {
                if self.children.len() > 1 {
                    (
                        0.0,
                        free_space / (self.children.len() - 1) as f32 + self.spacing,
                    )
                } else {
                    (0.0, self.spacing)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / self.children.len() as f32;
                (space / 2.0, space + self.spacing)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (self.children.len() + 1) as f32;
                (space, space + self.spacing)
            }
        };

        let mut cy = self.bounds.y + initial_offset;
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let cx = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => self.bounds.x,
                CrossAxisAlignment::Center => {
                    self.bounds.x + (self.bounds.width - child_size.width) / 2.0
                }
                CrossAxisAlignment::End => self.bounds.x + self.bounds.width - child_size.width,
                CrossAxisAlignment::Stretch => self.bounds.x,
            };

            child.set_origin(cx, cy);
            cy += child_size.height + between_spacing;
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn id(&self) -> WidgetId {
        self.widget_id
    }

    fn mark_dirty(&mut self, flags: ChangeFlags) {
        self.dirty_flags |= flags;
    }

    fn needs_layout(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_LAYOUT)
    }

    fn needs_paint(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_PAINT)
    }

    fn clear_dirty(&mut self) {
        self.dirty_flags = ChangeFlags::empty();
        // Also clear child dirty flags
        for child in &mut self.children {
            child.clear_dirty();
        }
    }
}

pub fn column() -> Column {
    Column::new()
}

#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        {
            let mut c = $crate::widgets::Column::new();
            $(
                c = c.child($child);
            )*
            c
        }
    };
}
