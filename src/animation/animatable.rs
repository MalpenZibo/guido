use smallvec::SmallVec;

use crate::transform::{Scale, Translate};
use crate::widgets::{Color, Padding};

/// The channels of one animatable value. `Corners` is the widest at five —
/// four radii and a curvature — and six costs the same 32 bytes as five, so
/// the inline capacity is six: the headroom is free and a value that spills
/// costs four heap allocations per spring retarget in `carry_velocity`.
pub type Channels = SmallVec<[f32; 6]>;

/// Trait for types that can be animated by interpolating between values
pub trait Animatable: Copy + PartialEq + Send + Sync + 'static {
    /// Linear interpolation between two values
    /// t = 0.0 returns `from`, t = 1.0 returns `to`
    /// t can exceed [0, 1] range for overshoot effects
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;

    /// Whether transitioning from `from` to `to` is a "reverse" direction.
    /// Used to select the `.reverse()` transition when configured.
    /// - `f32`: value decreasing
    /// - `Translate`: distance from the origin decreasing
    /// - `Scale`: area decreasing
    /// - `Color`: alpha decreasing, then luminance decreasing
    /// - `Padding`: total padding decreasing
    fn is_reverse(_from: &Self, _to: &Self) -> bool {
        false
    }

    /// The value as the vector of numbers the animation interpolates.
    ///
    /// Only `carry_velocity` reads this, and only as a direction: a spring's
    /// momentum is a vector in this space, and what a new segment inherits is
    /// the part of it pointing along itself. The channels may be in different
    /// units — `Corners` carries four radii in pixels beside a unitless
    /// curvature — and that is fine precisely because they are never summed
    /// into a length that is used on its own.
    fn channels(&self) -> Channels;
}

/// How fast the new segment should start, given the speed of the old one.
///
/// A spring integrates in a space normalised over its own segment, so its
/// velocity means "segments per second" of *that* segment. In the property's
/// own units the motion is `(target - start) * velocity`; the new segment
/// inherits the part of that vector that points along it:
///
/// ```text
/// v' = velocity * <old, new> / |new|²
/// ```
///
/// One projection does three jobs. It gets the **direction** right, sign
/// included — a negative result is a spring that still has to be turned
/// around, and keeping it is the whole point. It keeps **units apart**: a
/// translation interrupted by a pure scale change projects to nothing, because
/// the two directions are orthogonal, so momentum cannot leak between channels
/// that have nothing to do with each other. And the **overshoot it produces is
/// bounded** in the property's own units however short the new segment is,
/// since `v' * |new|` is the projected speed rather than an amplification of
/// it.
///
/// The cap is not for that: it is for a segment so short that the normalised
/// velocity, while physically harmless, would keep the spring from ever
/// reading as settled.
pub(crate) fn carry_velocity<T: Animatable>(
    velocity: f32,
    start: &T,
    target: &T,
    current: &T,
    new_target: &T,
) -> f32 {
    /// Crossing the whole segment once per frame at 60fps. Past this the
    /// motion is not something anyone can see, and the normalised units stop
    /// being meaningful.
    const MAX_CARRIED: f32 = 60.0;

    if !velocity.is_finite() || velocity == 0.0 {
        return 0.0;
    }

    let (from, to) = (start.channels(), target.channels());
    let (here, there) = (current.channels(), new_target.channels());

    let mut dot = 0.0;
    let mut new_len_sq = 0.0;
    for i in 0..here.len() {
        let old_axis = to[i] - from[i];
        let new_axis = there[i] - here[i];
        dot += old_axis * new_axis;
        new_len_sq += new_axis * new_axis;
    }

    if new_len_sq <= f32::MIN_POSITIVE {
        return 0.0;
    }
    let carried = velocity * dot / new_len_sq;
    if carried.is_finite() {
        carried.clamp(-MAX_CARRIED, MAX_CARRIED)
    } else {
        0.0
    }
}

impl Animatable for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        to < from
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[*self])
    }
}

impl Animatable for Color {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Color {
            r: from.r + (to.r - from.r) * t,
            g: from.g + (to.g - from.g) * t,
            b: from.b + (to.b - from.b) * t,
            a: from.a + (to.a - from.a) * t,
        }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        // Reverse when fading out (alpha decreasing),
        // or when darkening (luminance decreasing) at same alpha
        (to.a, to.luminance()) < (from.a, from.luminance())
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.r, self.g, self.b, self.a])
    }
}

impl Animatable for Padding {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Padding {
            left: from.left + (to.left - from.left) * t,
            right: from.right + (to.right - from.right) * t,
            top: from.top + (to.top - from.top) * t,
            bottom: from.bottom + (to.bottom - from.bottom) * t,
        }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        let to_total = to.left + to.right + to.top + to.bottom;
        let from_total = from.left + from.right + from.top + from.bottom;
        to_total < from_total
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.left, self.right, self.top, self.bottom])
    }
}

impl Animatable for crate::renderer::CornerRadii {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            top_left: from.top_left + (to.top_left - from.top_left) * t,
            top_right: from.top_right + (to.top_right - from.top_right) * t,
            bottom_right: from.bottom_right + (to.bottom_right - from.bottom_right) * t,
            bottom_left: from.bottom_left + (to.bottom_left - from.bottom_left) * t,
        }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        let total = |r: &Self| r.top_left + r.top_right + r.bottom_right + r.bottom_left;
        total(to) < total(from)
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ])
    }
}

impl Animatable for Translate {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            x: from.x + (to.x - from.x) * t,
            y: from.y + (to.y - from.y) * t,
        }
    }

    /// Moving back towards where it started.
    ///
    /// A change from the old `Transform`, which compared `extract_scale()` and
    /// so answered `false` for every pure translation: a container that slid
    /// out and back played its forward transition both ways, whatever it had
    /// declared for the reverse. A return leg is a reversal, and now says so.
    fn is_reverse(from: &Self, to: &Self) -> bool {
        to.x.hypot(to.y) < from.x.hypot(from.y)
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.x, self.y])
    }
}

impl Animatable for Scale {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            x: from.x + (to.x - from.x) * t,
            y: from.y + (to.y - from.y) * t,
        }
    }

    /// Getting smaller, measured as the two factors added rather than
    /// multiplied, and unsigned.
    ///
    /// Unsigned because a negative factor is a mirror and a mirror that grows
    /// is growing: the signed product sent `(-1, 1) -> (-2, 1)` down the
    /// reverse transition while it got bigger.
    ///
    /// Added rather than multiplied because the product is blind to a change
    /// of aspect: every `(k, 1/k)` stretch has area one, so `(1, 1) -> (2, 0.5)`
    /// and its return leg both read as forward and a declared `.reverse()`
    /// could never fire. `Padding` totals its four edges for the same reason.
    fn is_reverse(from: &Self, to: &Self) -> bool {
        to.x.abs() + to.y.abs() < from.x.abs() + from.y.abs()
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.x, self.y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stretch that keeps its area still has a direction. The product does
    /// not see it — every `(k, 1/k)` has area one — so the reverse transition
    /// was unreachable for the whole family.
    #[test]
    fn a_change_of_aspect_has_a_direction() {
        let square = Scale::NONE;
        let wide = Scale::new(2.0, 0.5);
        assert!(!Scale::is_reverse(&square, &wide), "going out is forward");
        assert!(Scale::is_reverse(&wide, &square), "and coming back is not");
    }

    /// A mirror that grows is growing.
    #[test]
    fn a_mirror_is_measured_by_its_size_not_its_sign() {
        let small = Scale::new(-1.0, 1.0);
        let large = Scale::new(-2.0, 1.0);
        assert!(!Scale::is_reverse(&small, &large));
        assert!(Scale::is_reverse(&large, &small));
    }

    /// A slide back is a reversal — which the old `Transform` could not say,
    /// because it compared scale and a translation does not change it.
    #[test]
    fn a_return_leg_is_a_reversal() {
        let home = Translate::NONE;
        let away = Translate::new(200.0, 0.0);
        assert!(!Translate::is_reverse(&home, &away));
        assert!(Translate::is_reverse(&away, &home));
    }

    #[test]
    fn test_f32_lerp() {
        assert_eq!(f32::lerp(&0.0, &10.0, 0.0), 0.0);
        assert_eq!(f32::lerp(&0.0, &10.0, 0.5), 5.0);
        assert_eq!(f32::lerp(&0.0, &10.0, 1.0), 10.0);
        // Overshoot
        assert_eq!(f32::lerp(&0.0, &10.0, 1.5), 15.0);
    }

    #[test]
    fn test_color_lerp() {
        let black = Color::rgb(0.0, 0.0, 0.0);
        let white = Color::rgb(1.0, 1.0, 1.0);
        let mid = Color::lerp(&black, &white, 0.5);
        assert_eq!(mid.r, 0.5);
        assert_eq!(mid.g, 0.5);
        assert_eq!(mid.b, 0.5);
    }

    #[test]
    fn test_padding_lerp() {
        let p1 = Padding::all(0.0);
        let p2 = Padding::all(10.0);
        let mid = Padding::lerp(&p1, &p2, 0.5);
        assert_eq!(mid.left, 5.0);
        assert_eq!(mid.right, 5.0);
        assert_eq!(mid.top, 5.0);
        assert_eq!(mid.bottom, 5.0);
    }

    #[test]
    fn channels_are_the_numbers_the_lerp_moves() {
        assert_eq!(Color::WHITE.channels().len(), 4);
        assert_eq!(Padding::all(2.0).channels().len(), 4);
        assert_eq!(Translate::new(1.0, 2.0).channels().len(), 2);
        // The widest, and so the one the inline capacity is sized from: four
        // radii and a curvature. `carry_velocity` builds four of these per
        // retarget, so a spilled one is four heap allocations per spring.
        assert!(
            crate::widgets::Corners::SQUARE.channels().len() <= Channels::new().inline_size(),
            "Channels must hold the widest animatable value without spilling"
        );
        assert_eq!(Scale::uniform(2.0).channels().len(), 2);
        assert_eq!(3.0_f32.channels().as_slice(), &[3.0]);
    }

    /// The velocity a new segment inherits is the part of the old motion that
    /// points along it — so the same speed sent the same way is kept whole.
    #[test]
    fn carrying_the_same_way_keeps_the_speed() {
        // 0 -> 1 at 2 spans/sec, retargeted to 2 from halfway: the remaining
        // segment is half as long, so the same physical speed is twice the
        // normalised one.
        let carried = carry_velocity(2.0, &0.0_f32, &1.0, &0.5, &1.5);
        assert!((carried - 2.0).abs() < 1e-5, "got {carried}");
    }

    /// And sent the other way it comes back negative, which is a spring that
    /// still has to be turned around.
    #[test]
    fn carrying_the_other_way_comes_back_negative() {
        let carried = carry_velocity(2.0, &0.0_f32, &1.0, &0.5, &-0.5);
        assert!(carried < 0.0, "got {carried}");
    }

    /// A motion that was going backwards along its own segment is going
    /// backwards in the property too — the integrator's sign is not something
    /// the geometry can be asked to re-derive.
    #[test]
    fn a_negative_velocity_is_read_as_the_motion_it_is() {
        // Past the target and falling back toward it: retargeting *below* the
        // current value is retargeting the way it is already moving.
        let carried = carry_velocity(-1.0, &0.0_f32, &1.0, &1.2, &0.0);
        assert!(
            carried > 0.0,
            "already falling, so the new segment is closing: got {carried}"
        );
    }

    /// Directions that share no channel share no momentum: a slide to the
    /// right, retargeted straight down, starts from rest.
    #[test]
    fn orthogonal_directions_carry_nothing() {
        let right = Translate::new(200.0, 0.0);
        let down = Translate::new(200.0, 200.0);

        let carried = carry_velocity(10.0, &Translate::NONE, &right, &right, &down);
        assert_eq!(carried, 0.0);
    }

    /// A retarget that asks for no movement at all has nothing to carry, and
    /// must not divide by it.
    #[test]
    fn a_degenerate_segment_carries_nothing() {
        assert_eq!(carry_velocity(5.0, &0.0_f32, &1.0, &0.5, &0.5), 0.0);
        assert_eq!(carry_velocity(f32::NAN, &0.0_f32, &1.0, &0.5, &1.0), 0.0);
    }

    /// A segment short enough to make the normalised velocity meaningless is
    /// capped rather than left to keep the spring from ever settling.
    #[test]
    fn an_almost_zero_segment_is_capped() {
        let carried = carry_velocity(10.0, &0.0_f32, &1.0, &0.5, &0.500001);
        assert!(carried.is_finite() && carried <= 60.0, "got {carried}");
    }
}
