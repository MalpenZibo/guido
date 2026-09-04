//! Whether a declared value is a number arithmetic can carry on with.
//!
//! A property is a closure the application wrote, and a division by a measured
//! zero is all it takes to produce a NaN. Once one is in it stays: `lerp` from
//! a NaN start is NaN at every `t`, so a later and perfectly finite target
//! animates from nothing to nothing for the life of the process — and
//! `animate_to`'s `new_target == self.target` early-out never fires, because
//! NaN is equal to nothing including itself, so the surface asks for another
//! frame at the compositor's rate for ever.
//!
//! The answer is CSS's: a value that is not a number is coerced *where it is
//! resolved*, once, so no consumer downstream ever has to know it happened. The
//! alternative — a guard at each consumer — is what was tried before, and each
//! guard changed what the next one saw.
//!
//! This lives apart from [`Animatable`](crate::animation::Animatable) on
//! purpose. "Can this be interpolated" and "is this a number" are different
//! questions, and hanging the second on the first put the door out of reach of
//! every declared value that is not animatable — a [`Length`], a [`Pivot`], a
//! gradient — which is most of the ways a bad number gets in.
//!
//! [`Length`]: crate::layout::Length
//! [`Pivot`]: crate::pivot::Pivot

use crate::reactive::Signal;

/// A value that can say whether every number inside it is finite.
///
/// Implemented for every type a property can be declared with. The name is
/// `all_finite` rather than `is_finite` because `f32` already has an inherent
/// method by that name: a trait method taking `&self` matches a `&f32` receiver
/// *exactly* while the inherent one needs a deref, so `c.is_finite()` inside a
/// blanket body resolves to the trait and calls itself until the stack ends.
pub(crate) trait AllFinite {
    /// Whether this value is one arithmetic can carry on with.
    fn all_finite(&self) -> bool;
}

impl AllFinite for f32 {
    fn all_finite(&self) -> bool {
        f32::is_finite(*self)
    }
}

/// Reading a declared property, with a value nothing can compute treated as no
/// declaration at all.
pub(crate) trait FiniteOr<T> {
    /// The value, or `default` where it is not finite.
    ///
    /// `property` is only for the diagnostic, and is the name a caller would
    /// recognise — the setter they wrote, not the accessor that resolves it.
    fn get_finite_or(&self, default: T, id: crate::tree::WidgetId, property: &'static str) -> T;

    /// The same, for the paths that read a snapshot rather than subscribing —
    /// the reach calculation, which runs beside a paint that is reading the
    /// same signal and has already reported it.
    fn get_finite_or_untracked(&self, default: T) -> T;
}

impl<T: AllFinite + Clone + 'static> FiniteOr<T> for Option<Signal<T>> {
    fn get_finite_or(&self, default: T, id: crate::tree::WidgetId, property: &'static str) -> T {
        let value = crate::reactive::OptionSignalExt::get_or(self, default.clone());
        if value.all_finite() {
            value
        } else {
            crate::reactive::diagnostics::non_finite_value(id, property);
            default
        }
    }

    fn get_finite_or_untracked(&self, default: T) -> T {
        let value = crate::reactive::OptionSignalExt::get_or_untracked(self, default.clone());
        if value.all_finite() { value } else { default }
    }
}

/// Every type a `Container` property is declared with, answered from the
/// numbers it is made of.
///
/// The animatable ones already have to say which numbers those are in order to
/// be interpolated at all, so they answer from
/// [`channels`](crate::animation::Animatable::channels) and cannot go stale
/// when a field is added — the last attempt at this wrote the check out per
/// type, covered three of seven, and nobody noticed the other four.
macro_rules! finite_by_channels {
    ($($t:ty),* $(,)?) => {
        $(impl AllFinite for $t {
            fn all_finite(&self) -> bool {
                crate::animation::Animatable::channels(self)
                    .iter()
                    .copied()
                    .all(f32::is_finite)
            }
        })*
    };
}

finite_by_channels!(
    crate::widgets::Color,
    crate::renderer::Shadow,
    crate::widgets::Padding,
    crate::widgets::Corners,
    crate::transform::Translate,
    crate::transform::Scale,
);

impl AllFinite for crate::layout::Length {
    /// Four optional numbers and a flag. An absent one cannot be bad, and
    /// `fill` is not a number at all.
    fn all_finite(&self) -> bool {
        [self.min, self.max, self.exact, self.fraction]
            .into_iter()
            .flatten()
            .all(f32::is_finite)
    }
}

impl AllFinite for crate::pivot::Pivot {
    /// An anchor is only a number when it is a `Percent` or a `Px`; the named
    /// ones are constants and cannot be anything else.
    fn all_finite(&self) -> bool {
        use crate::pivot::{HorizontalAnchor as H, VerticalAnchor as V};
        let horizontal = match self.horizontal {
            H::Percent(p) | H::Px(p) => p.is_finite(),
            _ => true,
        };
        let vertical = match self.vertical {
            V::Percent(p) | V::Px(p) => p.is_finite(),
            _ => true,
        };
        // Both arms, and both are watched: the enumeration test puts the bad
        // number in the horizontal anchor and the hit-testing one puts it in
        // the vertical, so neither can be deleted unnoticed.
        horizontal && vertical
    }
}
