pub mod flex;
pub mod flex_layout;
pub mod zstack;

pub use flex::{Constraints, Size};
pub use flex_layout::Flex;
pub use zstack::ZStack;

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

/// Accepting f64 keeps bare float literals (which default to f64) working
/// without relying on the deprecated f32 inference fallback
/// (rust-lang/rust#154024).
impl IntoF32 for f64 {
    fn into_f32(self) -> f32 {
        self as f32
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
    /// Fraction (0.0..=1.0) of the available space, resolved against the
    /// incoming constraints at layout time. Behaves like an `exact` length
    /// whose value is only known during layout — a slider fill bar at 55%
    /// is `fraction(0.55)` with no measure round-trip.
    pub fraction: Option<f32>,
}

impl Length {
    /// Create a length with an exact value.
    pub fn exact(value: impl IntoF32) -> Self {
        Length {
            min: None,
            max: None,
            exact: Some(value.into_f32()),
            fill: false,
            fraction: None,
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
        fraction: None,
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
        fraction: None,
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
        fraction: None,
    }
}

/// Create a length that takes a fraction (0.0..=1.0) of the available
/// space, resolved at layout time.
///
/// The natural tool for value-proportional bars (sliders, gauges,
/// progress): the width follows the value on the very first frame, with
/// no measured-rect round-trip.
///
/// # Examples
/// ```
/// use guido::prelude::*;
///
/// // A fill bar at 55% of the track width
/// container().width(fraction(0.55));
///
/// // Reactive: follows the volume signal
/// let volume = create_signal(40);
/// container().width(move || fraction(volume.get() as f32 / 100.0));
/// ```
pub fn fraction(f: impl IntoF32) -> Length {
    Length {
        min: None,
        max: None,
        exact: None,
        fill: false,
        fraction: Some(f.into_f32()),
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
            fraction: None,
        }
    }
}

/// f64 converts to exact sizing — bare float literals default to f64, and
/// accepting them avoids the deprecated f32 inference fallback.
impl From<f64> for Length {
    fn from(value: f64) -> Self {
        Length::from(value as f32)
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

impl IntoVal<Length> for f64 {
    fn into_val(self) -> Length {
        Length::from(self as f32)
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
    /// Line the children up on the baseline of their first line of text, so
    /// labels of different sizes read as one line instead of floating at
    /// unrelated heights.
    ///
    /// Only meaningful across a row; in a column it behaves as `Start`.
    /// A child that reports no baseline — a box, an image — is aligned by its
    /// bottom edge, which is what CSS does with it.
    Baseline,
}
