use guido::layout::{Constraints, Flex, Layout, Size};
use guido::tree::{LayoutCtx, WidgetId};

// Clipping needs oversized content even when its container obeys parent limits.
pub struct OverflowLayout {
    pub width: f32,
    pub height: Option<f32>,
    pub flex: Flex,
}

impl Layout for OverflowLayout {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        mut constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        assert!(constraints.max_width <= self.width);
        constraints.min_width = self.width;
        constraints.max_width = self.width;
        if let Some(height) = self.height {
            assert!(constraints.max_height <= height);
            constraints.min_height = height;
            constraints.max_height = height;
        }
        let size = self.flex.layout(ctx, children, constraints, origin);
        for child in children {
            let bounds = ctx.tree_ref().get_bounds(*child).unwrap();
            assert_eq!(bounds.width, self.width);
            if let Some(height) = self.height {
                assert_eq!(bounds.height, height);
            }
        }
        size
    }
}
