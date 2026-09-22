pub mod flex;
pub mod flex_layout;
pub mod zstack;

pub use flex::{Constraints, Size};
pub use flex_layout::Flex;
pub use zstack::ZStack;

use crate::tree::{LayoutCtx, WidgetId};

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    extent: Extent,
    /// [`NO_MIN`] where none was declared.
    min: f32,
    /// [`NO_MAX`] where none was declared.
    max: f32,
}

/// How big the box is, before its bounds are applied.
///
/// Four cases, and exactly one holds — which is the point of it being one
/// field. A box cannot be both an exact size and a share of its parent, and a
/// representation that can say so is one the layout has to pick a winner from.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Extent {
    /// Size to the content, which is what a length that says nothing means.
    #[default]
    Auto,
    /// A size in logical pixels.
    Exact(f32),
    /// Every pixel the parent offers.
    Fill,
    /// A share (0.0..=1.0) of the available space, resolved against the
    /// incoming constraints at layout time. Behaves like an [`Exact`] length
    /// whose value is only known during layout — a slider fill bar at 55% is
    /// `fraction(0.55)` with no measure round-trip.
    ///
    /// [`Exact`]: Extent::Exact
    Fraction(f32),
}

impl Extent {
    /// The number this extent carries, where it carries one.
    ///
    /// `Auto` and `Fill` are not numbers at all, which is what makes them the
    /// two cases a bad one cannot hide in.
    pub fn declared_size(&self) -> Option<f32> {
        match self {
            Extent::Exact(size) | Extent::Fraction(size) => Some(*size),
            Extent::Auto | Extent::Fill => None,
        }
    }
}

/// The bounds nobody declared, as values rather than as `Option`s.
///
/// Each is the identity of the operation that applies it, over *every* input,
/// so [`Length::clamp`] needs no branch. That "every" is the whole of the
/// choice: `f32::MAX` looks like an identity for `min` and is one only over
/// finite inputs — `INFINITY.min(f32::MAX)` is `f32::MAX` — and an unbounded
/// axis is exactly the infinite input. A scrolled axis measures with
/// `f32::INFINITY`, so a sentinel that swallows it hands every reader below a
/// finite `max_width`, and a `fill` or a `fraction` underneath lays out at
/// 3.4e38. `x.min(INFINITY)` is `x` for all x, including `INFINITY` itself.
///
/// They are not finite, and do not need to be: `AllFinite` reads the bounds
/// through [`Length::min`] and [`Length::max`], which answer `None` for a
/// sentinel, so a declaration nobody made is never mistaken for a bad number.
/// `at_most(f32::INFINITY)` therefore means "no maximum" rather than being
/// reported as a value that is not a number — which is what it means, and what
/// Flutter's `BoxConstraints` spells the same way.
const NO_MIN: f32 = f32::NEG_INFINITY;
const NO_MAX: f32 = f32::INFINITY;

/// Written out rather than derived, because a derived one would give `max` the
/// `f32` default of `0.0` — a maximum of nothing rather than no maximum, and
/// every undeclared length would measure zero.
impl Default for Length {
    fn default() -> Self {
        Self {
            extent: Extent::default(),
            min: NO_MIN,
            max: NO_MAX,
        }
    }
}

impl Length {
    /// How big the box is, before [`min`](Self::min) and [`max`](Self::max).
    pub fn extent(&self) -> Extent {
        self.extent
    }

    /// The exact size, where one was declared.
    ///
    /// Named for what it answers rather than `exact`, which is the
    /// constructor's name — `Length::exact(200.0)` builds one and this reads
    /// it back.
    pub fn exact_size(&self) -> Option<f32> {
        match self.extent {
            Extent::Exact(size) => Some(size),
            _ => None,
        }
    }

    /// Whether this length takes every pixel the parent offers.
    pub fn is_fill(&self) -> bool {
        matches!(self.extent, Extent::Fill)
    }

    /// Whether the size is settled without measuring the content.
    ///
    /// What the layout asks before passing a minimum down: an exact length and
    /// a fill are both as big as they are going to get, so the children can be
    /// told the whole of the box. The two other extents depend on what is
    /// inside.
    pub(crate) fn settled_without_content(&self) -> bool {
        matches!(self.extent, Extent::Exact(_) | Extent::Fill)
    }

    /// The minimum, where one was declared.
    pub fn min(&self) -> Option<f32> {
        (self.min != NO_MIN).then_some(self.min)
    }

    /// The maximum, where one was declared.
    pub fn max(&self) -> Option<f32> {
        (self.max != NO_MAX).then_some(self.max)
    }

    /// `size` held within the declared bounds.
    ///
    /// Two unconditional operations rather than two branches, which is what
    /// the sentinels buy. `f32::clamp` is not used because it panics when
    /// `min > max`, and `at_least(400.0).at_most(50.0)` is constructible
    /// through the public API and arrives here; `.max().min()` answers 50.
    pub(crate) fn clamp(&self, size: f32) -> f32 {
        size.max(self.min).min(self.max)
    }

    /// `size` held within the declared maximum alone.
    ///
    /// For the reader that has a floor of its own and only wants the ceiling.
    pub(crate) fn clamp_max(&self, size: f32) -> f32 {
        size.min(self.max)
    }

    /// Resolve a fraction against the space now known, making it exact.
    ///
    /// The one mutation the layout performs on a declared length: a fraction is
    /// an exact size whose number arrives late, so as soon as the constraints
    /// are known it becomes one and every reader downstream sees an ordinary
    /// exact length.
    ///
    /// An unbounded `available` leaves it alone — there is no share to take of
    /// a space nobody has measured — and it stays a `Fraction`, which sizes to
    /// content like the `Auto` it then behaves as.
    pub(crate) fn resolve_fraction(&mut self, available: f32) {
        if let Extent::Fraction(share) = self.extent
            && available.is_finite()
        {
            self.extent = Extent::Exact((available * share).max(0.0));
        }
    }

    /// The length this one describes, with its extent replaced.
    fn with(self, extent: Extent) -> Self {
        Self { extent, ..self }
    }

    /// Create a length with an exact value.
    pub fn exact(value: impl IntoF32) -> Self {
        Length::default().with(Extent::Exact(value.into_f32()))
    }

    /// Add a minimum constraint to this length.
    pub fn at_least(mut self, min: impl IntoF32) -> Self {
        self.min = min.into_f32();
        self
    }

    /// Add a maximum constraint to this length.
    pub fn at_most(mut self, max: impl IntoF32) -> Self {
        self.max = max.into_f32();
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
    Length::default().at_least(min)
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
    Length::default().at_most(max)
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
    Length::default().with(Extent::Fill)
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
    Length::default().with(Extent::Fraction(f.into_f32()))
}

/// f32 converts to exact sizing
impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Length::exact(value)
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

/// Trait for layout strategies that position multiple children
pub trait Layout {
    /// Perform layout on children and return the total size.
    ///
    /// Children are identified by WidgetId and accessed via the passed Tree.
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
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

// Every conversion a length accepts, declared once: the closure form and the
// three signal forms. The value form is the `From` above. See
// `reactive::into_signal` for why the signal forms name their source type, and
// the `widgets` skill for the rule that a property takes all five.
crate::reactive::converts!(
    f32 => Length,
    f64 => Length,
    i32 => Length,
    u32 => Length,
    u16 => Length,
);

#[cfg(test)]
mod a_length_is_what_it_carries {
    use super::*;
    use crate::reactive::Prop;
    use std::mem::size_of;

    /// A `Length` weighs what it says, and a property field weighs the same.
    ///
    /// Both halves matter. The first is the struct; the second is what makes
    /// it worth doing, because since #450 a property field stores its value
    /// inline — `width: Prop<Length>` and `height: Prop<Length>` sit on every
    /// `Container` in every tree, declared or not.
    ///
    /// `Prop<Length>` matching `Length` exactly is the niche at work: the
    /// discriminant fits in a bit pattern `Extent` does not use, so the three
    /// cases cost nothing over the value. Lose that and this says so.
    #[test]
    fn and_a_property_holding_one_weighs_the_same() {
        assert_eq!(
            size_of::<Length>(),
            16,
            "a length is an extent and two bounds"
        );
        assert_eq!(
            size_of::<Prop<Length>>(),
            16,
            "and the property tag rides in a niche rather than a word of its own"
        );
    }

    /// An extent carries a number only when it names one.
    ///
    /// Its own test because its only readers cannot speak for it: `AllFinite`
    /// checks the number it returns, and the characterization enumeration
    /// reads a length's numbers back through the very same call — so a
    /// `declared_size` that answered `None` to everything would leave both the
    /// checker and the checked agreeing that a `width(NaN)` has no number in
    /// it, and nothing would object.
    #[test]
    fn an_extent_carries_a_number_only_when_it_names_one() {
        assert_eq!(
            Extent::Auto.declared_size(),
            None,
            "sizing to content is not a number"
        );
        assert_eq!(
            Extent::Fill.declared_size(),
            None,
            "nor is taking what is offered"
        );
        assert_eq!(Extent::Exact(12.0).declared_size(), Some(12.0));
        assert_eq!(Extent::Fraction(0.25).declared_size(), Some(0.25));

        // Which is what lets a bad number be found wherever it was declared.
        assert!(
            Length::exact(f32::NAN)
                .extent()
                .declared_size()
                .is_some_and(f32::is_nan)
        );
        assert!(
            fraction(f32::NAN)
                .extent()
                .declared_size()
                .is_some_and(f32::is_nan)
        );
    }

    /// The four sizing cases are four, and each one says only itself.
    ///
    /// They were three independent fields, so `exact(10)` and `fill` and
    /// `fraction(0.5)` could all be set at once and the layout would silently
    /// honour whichever its `if / else if` chain reached first.
    #[test]
    fn the_four_sizing_cases_are_mutually_exclusive() {
        assert_eq!(Length::default().extent(), Extent::Auto);
        assert_eq!(Length::from(10.0f32).extent(), Extent::Exact(10.0));
        assert_eq!(fill().extent(), Extent::Fill);
        assert_eq!(fraction(0.5).extent(), Extent::Fraction(0.5));

        // A bound is orthogonal to the extent and does not disturb it.
        assert_eq!(fill().at_least(4.0).extent(), Extent::Fill);
        assert_eq!(at_least(4.0).extent(), Extent::Auto);
    }

    /// An absent bound reads as absent, although it is stored as a number.
    ///
    /// `0.0` and `f32::MAX` are the sentinels — the hand-rolled version of the
    /// niche stable Rust will not let us declare for a float. Both are finite,
    /// which is what keeps `AllFinite` reporting a bad number rather than a
    /// missing one.
    /// A bound is applied where it was declared, and nowhere else.
    ///
    /// The sentinels make `clamp` branch-free, and the risk in that is a
    /// sentinel doing arithmetic it was not asked to: `0.0` as "no minimum"
    /// would floor a negative size at zero, which nobody declared. The true
    /// identities do not, so a length with no bounds hands back exactly what
    /// it was given.
    #[test]
    fn an_undeclared_bound_changes_nothing_it_is_applied_to() {
        let unbounded = Length::default();
        assert_eq!(unbounded.clamp(-10.0), -10.0, "no minimum floors nothing");
        assert_eq!(unbounded.clamp(1.0e30), 1.0e30, "no maximum caps nothing");
        assert_eq!(
            unbounded.clamp(f32::INFINITY),
            f32::INFINITY,
            "and an unbounded size passes through, which is what a scrolled \
             axis hands down"
        );

        assert_eq!(at_least(4.0).clamp(-10.0), 4.0);
        assert_eq!(at_most(40.0).clamp(1.0e30), 40.0);

        // Declared the wrong way round, which the public API permits.
        // `f32::clamp` would panic here; the maximum wins, as it does in CSS.
        assert_eq!(at_least(400.0).at_most(50.0).clamp(100.0), 50.0);
    }

    #[test]
    fn a_bound_nobody_declared_reads_as_none() {
        assert_eq!(Length::default().min(), None);
        assert_eq!(Length::default().max(), None);
        assert_eq!(at_least(50.0).min(), Some(50.0));
        assert_eq!(at_least(50.0).max(), None);
        assert_eq!(at_most(400.0).max(), Some(400.0));
        assert_eq!(at_least(50.0).at_most(400.0).min(), Some(50.0));
        assert_eq!(at_least(50.0).at_most(400.0).max(), Some(400.0));

        // The sentinels are infinities, so declaring one reads back as the
        // absence it is — which is what `at_most(INFINITY)` means, rather than
        // a number `AllFinite` should report.
        assert_eq!(at_most(f32::INFINITY).max(), None);
        assert_eq!(at_least(f32::NEG_INFINITY).min(), None);

        // A bound that is genuinely not a number still reads as declared, so
        // `AllFinite` can find it.
        assert!(at_most(f32::NAN).max().is_some_and(f32::is_nan));
    }
}
