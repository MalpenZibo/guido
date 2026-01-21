pub mod flex;
pub mod flex_layout;

pub use flex::{Constraints, Size};
pub use flex_layout::Flex;

use crate::widgets::Widget;

/// Trait for layout strategies that position multiple children
pub trait Layout: Send + Sync {
    /// Perform layout on children and return the total size
    fn layout(
        &mut self,
        children: &mut [Box<dyn Widget>],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size;
}

/// Direction for flex layout
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Main axis alignment for flex layouts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAxisAlignment {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Cross axis alignment for flex layouts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossAxisAlignment {
    Start,
    Center,
    End,
    Stretch,
}

// IntoMaybeDyn implementations for reactive support
use crate::reactive::{IntoMaybeDyn, MaybeDyn};

impl IntoMaybeDyn<Axis> for Axis {
    fn into_maybe_dyn(self) -> MaybeDyn<Axis> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<MainAxisAlignment> for MainAxisAlignment {
    fn into_maybe_dyn(self) -> MaybeDyn<MainAxisAlignment> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<CrossAxisAlignment> for CrossAxisAlignment {
    fn into_maybe_dyn(self) -> MaybeDyn<CrossAxisAlignment> {
        MaybeDyn::Static(self)
    }
}
