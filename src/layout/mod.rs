pub mod flex;
pub mod flex_layout;
pub mod overlay;

pub use flex::{Constraints, Size};
pub use flex_layout::Flex;
pub use overlay::Overlay;

use crate::tree::{Tree, WidgetId};

/// Trait for types that can be converted to f32 for use in layout dimensions.
///
/// This extends beyond `Into<f32>` to include `i32` and `u32` which don't have
/// lossless `From<T>` impls for `f32` but are commonly used for pixel values.
pub trait IntoF32 {
    fn into_f32(self) -> f32;
}

impl IntoF32 for f32 {
    fn into_f32(self) -> f32 {
        self
    }
}

impl IntoF32 for i32 {
    fn into_f32(self) -> f32 {
        self as f32
    }
}

impl IntoF32 for u16 {
    fn into_f32(self) -> f32 {
        self as f32
    }
}

impl IntoF32 for u32 {
    fn into_f32(self) -> f32 {
        self as f32
    }
}

/// A unified sizing type that can specify exact, min, max, or range constraints.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// // Exact size (most common)
/// container().width(200.0);
///
/// // Integers also work
/// container().width(200).height(100);
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
    /// Create a length with an exact value.
    pub fn exact(value: impl IntoF32) -> Self {
        Length {
            min: None,
            max: None,
            exact: Some(value.into_f32()),
            fill: false,
        }
    }

    /// Add a minimum constraint to this length.
    pub fn at_least(mut self, min: impl IntoF32) -> Self {
        self.min = Some(min.into_f32());
        self
    }

    /// Add a maximum constraint to this length.
    pub fn at_most(mut self, max: impl IntoF32) -> Self {
        self.max = Some(max.into_f32());
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
/// container().width(at_least(100));
/// container().width(at_least(50.0).at_most(400.0));
/// ```
pub fn at_least(min: impl IntoF32) -> Length {
    Length {
        min: Some(min.into_f32()),
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
/// container().width(at_most(400));
/// container().width(at_most(400.0).at_least(50.0));
/// ```
pub fn at_most(max: impl IntoF32) -> Length {
    Length {
        min: None,
        max: Some(max.into_f32()),
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

impl From<i32> for Length {
    fn from(value: i32) -> Self {
        Length::from(value as f32)
    }
}

impl From<u16> for Length {
    fn from(value: u16) -> Self {
        Length::from(value as f32)
    }
}

impl From<u32> for Length {
    fn from(value: u32) -> Self {
        Length::from(value as f32)
    }
}

// IntoVal<Length> impls for closures returning numeric types
use crate::reactive::IntoVal;

impl IntoVal<Length> for i32 {
    fn into_val(self) -> Length {
        Length::from(self)
    }
}

impl IntoVal<Length> for u32 {
    fn into_val(self) -> Length {
        Length::from(self)
    }
}

impl IntoVal<Length> for u16 {
    fn into_val(self) -> Length {
        Length::from(self)
    }
}

impl IntoVal<Length> for f32 {
    fn into_val(self) -> Length {
        Length::from(self)
    }
}

/// Trait for layout strategies that position multiple children
pub trait Layout {
    /// Perform layout on children and return the total size.
    ///
    /// Children are identified by WidgetId and accessed via the passed Tree.
    fn layout(
        &mut self,
        tree: &mut Tree,
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
pub enum MainAlignment {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Cross axis alignment for flex layouts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossAlignment {
    Start,
    Center,
    End,
    Stretch,
}
