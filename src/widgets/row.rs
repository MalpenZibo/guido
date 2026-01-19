use crate::layout::{Constraints, Size};
use crate::renderer::PaintContext;

use super::widget::{Event, EventResponse, Rect, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAxisAlignment {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossAxisAlignment {
    Start,
    Center,
    End,
    Stretch,
}

pub struct Row {
    children: Vec<Box<dyn Widget>>,
    spacing: f32,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    bounds: Rect,
    child_sizes: Vec<Size>,
}

impl Row {
    pub fn new() -> Self {
        Self {
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

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Row {
    fn layout(&mut self, constraints: Constraints) -> Size {
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

        for child in &mut self.children {
            let size = child.layout(child_constraints);
            total_width += size.width;
            max_height = max_height.max(size.height);
            self.child_sizes.push(size);
        }

        // Add spacing
        if !self.children.is_empty() {
            total_width += self.spacing * (self.children.len() - 1) as f32;
        }

        // For space-based alignments, expand to fill available width
        let width = match self.main_axis_alignment {
            MainAxisAlignment::SpaceBetween
            | MainAxisAlignment::SpaceAround
            | MainAxisAlignment::SpaceEvenly => constraints.max_width,
            _ => total_width
                .max(constraints.min_width)
                .min(constraints.max_width),
        };

        let size = Size::new(
            width,
            max_height
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
        let children_width: f32 = self.child_sizes.iter().map(|s| s.width).sum();
        let free_space = (size.width - children_width - total_spacing).max(0.0);

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

        let mut x = self.bounds.x + initial_offset;
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let y = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => self.bounds.y,
                CrossAxisAlignment::Center => {
                    self.bounds.y + (size.height - child_size.height) / 2.0
                }
                CrossAxisAlignment::End => self.bounds.y + size.height - child_size.height,
                CrossAxisAlignment::Stretch => self.bounds.y,
            };

            child.set_origin(x, y);
            x += child_size.width + between_spacing;
        }

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
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

        for child in &mut self.children {
            // Hacky but works for repositioning
            let dummy_constraints = Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: self.bounds.width,
                max_height: self.bounds.height,
            };
            child.layout(dummy_constraints);
        }

        // Reposition with the same layout logic
        let total_spacing = if self.children.len() > 1 {
            self.spacing * (self.children.len() - 1) as f32
        } else {
            0.0
        };
        let children_width: f32 = self.child_sizes.iter().map(|s| s.width).sum();
        let free_space = (self.bounds.width - children_width - total_spacing).max(0.0);

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

        let mut cx = self.bounds.x + initial_offset;
        for (i, child) in self.children.iter_mut().enumerate() {
            let child_size = self.child_sizes[i];
            let cy = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => self.bounds.y,
                CrossAxisAlignment::Center => {
                    self.bounds.y + (self.bounds.height - child_size.height) / 2.0
                }
                CrossAxisAlignment::End => self.bounds.y + self.bounds.height - child_size.height,
                CrossAxisAlignment::Stretch => self.bounds.y,
            };

            child.set_origin(cx, cy);
            cx += child_size.width + between_spacing;
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }
}

pub fn row() -> Row {
    Row::new()
}

#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        {
            let mut r = $crate::widgets::Row::new();
            $(
                r = r.child($child);
            )*
            r
        }
    };
}
