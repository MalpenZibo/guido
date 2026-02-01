pub mod flex;
pub mod flex_layout;
pub mod overlay;

pub use flex::{Constraints, Size};
pub use flex_layout::Flex;
pub use overlay::Overlay;

use crate::reactive::{IntoMaybeDyn, MaybeDyn, WidgetId};

/// A unified sizing type that can specify exact, min, max, or range constraints.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// // Exact size (most common)
/// container().width(200.0);
///
/// // Minimum only
/// container().width(at_least(100.0));
///
/// // Maximum only
/// container().width(at_most(400.0));
///
/// // Range (both work)
/// container().width(at_least(50.0).at_most(400.0));
/// container().width(at_most(400.0).at_least(50.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Length {
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub exact: Option<f32>,
    /// When true, expand to fill all available space
    pub fill: bool,
}

impl Length {
    /// Add a minimum constraint to this length.
    pub fn at_least(mut self, min: f32) -> Self {
        self.min = Some(min);
        self
    }

    /// Add a maximum constraint to this length.
    pub fn at_most(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }
}

/// Create a length with a minimum constraint.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// container().width(at_least(100.0));
/// container().width(at_least(50.0).at_most(400.0));
/// ```
pub fn at_least(min: f32) -> Length {
    Length {
        min: Some(min),
        max: None,
        exact: None,
        fill: false,
    }
}

/// Create a length with a maximum constraint.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// container().width(at_most(400.0));
/// container().width(at_most(400.0).at_least(50.0));
/// ```
pub fn at_most(max: f32) -> Length {
    Length {
        min: None,
        max: Some(max),
        exact: None,
        fill: false,
    }
}

/// Create a length that fills all available space.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// // Fill available width
/// container().width(fill());
///
/// // Fill available height
/// container().height(fill());
/// ```
pub fn fill() -> Length {
    Length {
        min: None,
        max: None,
        exact: None,
        fill: true,
    }
}

/// f32 converts to exact sizing
impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Length {
            min: None,
            max: None,
            exact: Some(value),
            fill: false,
        }
    }
}

impl IntoMaybeDyn<Length> for Length {
    fn into_maybe_dyn(self) -> MaybeDyn<Length> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<Length> for f32 {
    fn into_maybe_dyn(self) -> MaybeDyn<Length> {
        MaybeDyn::Static(Length::from(self))
    }
}

/// Trait for layout strategies that position multiple children
pub trait Layout: Send + Sync {
    /// Perform layout on children and return the total size.
    ///
    /// Children are identified by WidgetId and accessed via the global LayoutArena.
    fn layout(
        &mut self,
        children: &[WidgetId],
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
