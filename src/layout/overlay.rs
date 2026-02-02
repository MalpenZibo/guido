//! Overlay layout that stacks children on top of each other.

use super::{Constraints, Layout, Size};
use crate::reactive::{LayoutArena, WidgetId};

/// Overlay layout that places all children at the same position,
/// stacking them on top of each other. Later children appear on top.
///
/// The size of the overlay is determined by the largest child.
pub struct Overlay;

impl Overlay {
    /// Create a new overlay layout
    pub fn new() -> Self {
        Self
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout for Overlay {
    fn layout(
        &mut self,
        arena: &LayoutArena,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        // Layout all children at the same origin, giving them the full constraints
        for &child_id in children.iter() {
            if let Some(child_size) = arena.with_widget_mut(child_id, |child| {
                let size = child.layout(arena, constraints);
                child.set_origin(origin.0, origin.1);
                size
            }) {
                max_width = max_width.max(child_size.width);
                max_height = max_height.max(child_size.height);
            }
        }

        // Return the size of the largest child, constrained
        constraints.constrain(Size::new(max_width, max_height))
    }
}
