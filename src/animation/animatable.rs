use smallvec::SmallVec;

use crate::renderer::{Shadow, Shadows};
use crate::transform::{Scale, Translate};
use crate::widgets::{Color, Padding};

/// The channels of one animatable value. `Shadow` is eight — two offsets, a
/// blur, a spread and four colour components — and it is what sets the inline
/// capacity: a value that spills costs four heap allocations per spring
/// retarget in `carry_velocity`, which is a worse trade than the eight bytes
/// eight slots cost over the six `Corners` needs. A `Shadows` of more than one
/// spills, and pays that on a retarget rather than every value paying for its
/// thirty-two on the stack.
pub type Channels = SmallVec<[f32; 8]>;

/// Trait for types that can be animated by interpolating between values
pub trait Animatable: Copy + PartialEq + Send + Sync + 'static {
    /// Linear interpolation between two values
    /// t = 0.0 returns `from`, t = 1.0 returns `to`
    /// t can exceed [0, 1] range for overshoot effects
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;

    /// The value as the vector of numbers the animation interpolates.
    ///
    /// `carry_velocity` and the finiteness check read this, and only as a direction: a spring's
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

    // A channel one value has and another lacks is zero in the other: a list
    // of shadows answers with as many as it holds, and the shorter list is the
    // longer one with nothing drawn at the end.
    let at = |c: &Channels, i: usize| c.get(i).copied().unwrap_or(0.0);
    let len = [&from, &to, &here, &there]
        .map(|c| c.len())
        .into_iter()
        .max()
        .unwrap_or(0);

    let mut dot = 0.0;
    let mut new_len_sq = 0.0;
    for i in 0..len {
        let old_axis = at(&to, i) - at(&from, i);
        let new_axis = at(&there, i) - at(&here, i);
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

    fn channels(&self) -> Channels {
        Channels::from_slice(&[*self])
    }
}

impl Animatable for Color {
    /// With alpha premultiplied — see [`Color::mix`].
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from.mix(*to, t)
    }

    /// Premultiplied, because that is the space `lerp` is straight in, and a
    /// spring's momentum has to be read where the motion is a line.
    fn channels(&self) -> Channels {
        Channels::from_slice(&self.premultiplied())
    }
}

impl Animatable for Shadow {
    /// Every length and the colour, continuously. Inset or not is a step,
    /// taken halfway, as CSS takes every value it cannot interpolate.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        Self {
            offset: (f(from.offset.0, to.offset.0), f(from.offset.1, to.offset.1)),
            blur: f(from.blur, to.blur),
            spread: f(from.spread, to.spread),
            color: Color::lerp(&from.color, &to.color, t),
            inset: if t < 0.5 { from.inset } else { to.inset },
        }
    }

    fn channels(&self) -> Channels {
        let [r, g, b, a] = self.color.premultiplied();
        Channels::from_slice(&[
            self.offset.0,
            self.offset.1,
            self.blur,
            self.spread,
            r,
            g,
            b,
            a,
        ])
    }
}

impl Animatable for Shadows {
    /// Shadow by shadow, the way CSS interpolates a `box-shadow` list.
    ///
    /// The shorter list is padded with `Shadow::none()`, inset where its
    /// counterpart is; colours interpolate premultiplied, so one arriving
    /// fades in in its own colour. Where a shadow is inset at one end and not
    /// at the other there is no shape between the two, and the whole list
    /// steps halfway instead.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let (a, b) = (from.as_slice(), to.as_slice());
        let absent = |opposite: &Shadow| Shadow {
            inset: opposite.inset,
            ..Shadow::none()
        };
        let mut between = Shadows::none();
        for i in 0..a.len().max(b.len()) {
            let (x, y) = match (a.get(i), b.get(i)) {
                (Some(x), Some(y)) => (*x, *y),
                (Some(x), None) => (*x, absent(x)),
                (None, Some(y)) => (absent(y), *y),
                (None, None) => unreachable!("`i` is below the longer length"),
            };
            if x.inset != y.inset {
                return if t < 0.5 { *from } else { *to };
            }
            between.push(Shadow::lerp(&x, &y, t));
        }
        between
    }

    /// Each shadow's, one after another. A shorter list has fewer, which
    /// `carry_velocity` reads as zero — as `lerp` pads it.
    fn channels(&self) -> Channels {
        self.as_slice().iter().flat_map(Shadow::channels).collect()
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
    /// Component by component, the pixels and the fractions apart — CSS
    /// interpolates `calc(px + %)` the same way — so a slide between two
    /// relative offsets never has to know the width it will be resolved at.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            x: mix(from.x, to.x),
            y: mix(from.y, to.y),
            relative_x: mix(from.relative_x, to.relative_x),
            relative_y: mix(from.relative_y, to.relative_y),
        }
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.x, self.y, self.relative_x, self.relative_y])
    }
}

impl Animatable for Scale {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            x: from.x + (to.x - from.x) * t,
            y: from.y + (to.y - from.y) * t,
        }
    }

    fn channels(&self) -> Channels {
        Channels::from_slice(&[self.x, self.y])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every channel at a known `t`, because the behavioural test can only see
    /// a direction and a bound.
    ///
    /// `a + (b - a) * t` with the sign flipped to `a + (b + a) * t` moves every
    /// channel the same way it should, just at the wrong rate, so a test that
    /// asks "did it leave, is it short of the target, is it further along" says
    /// yes to both. Exact values at `t = 0.5` are what tell them apart.
    #[test]
    fn a_shadow_lerps_every_one_of_its_channels() {
        let from = Shadow::new((6.0, 2.0), 4.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.4));
        let to = Shadow::new((-10.0, 12.0), 24.0, 6.0, Color::rgba(1.0, 0.0, 0.0, 0.6));

        assert_eq!(
            Shadow::lerp(&from, &to, 0.0),
            from,
            "t=0 is where it starts"
        );
        assert_eq!(Shadow::lerp(&from, &to, 1.0), to, "and t=1 is the target");

        let mid = Shadow::lerp(&from, &to, 0.5);
        assert_eq!(mid.offset, (-2.0, 7.0), "both axes, one crossing zero");
        assert_eq!(mid.blur, 14.0);
        assert_eq!(mid.spread, 3.0);
        // Premultiplied: red weighted by the 0.6 it is drawn at against the
        // black's 0.4, so it is more than half red.
        assert!((mid.color.r - 0.6).abs() < 1e-6, "{:?}", mid.color);
        assert!((mid.color.a - 0.5).abs() < 1e-6);

        // Past the target, which is what a spring needs of it.
        assert_eq!(Shadow::lerp(&from, &to, 1.5).blur, 34.0);
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

    /// A fade from `TRANSPARENT` keeps the colour it fades to. `TRANSPARENT`
    /// is black at no alpha, and interpolated channel by channel the halfway
    /// was grey at half alpha — over a white page, darker than either end.
    #[test]
    fn a_fade_from_transparent_keeps_the_colour_it_fades_to() {
        let half_white = Color::rgba(1.0, 1.0, 1.0, 0.5);
        assert_eq!(
            Color::lerp(&Color::TRANSPARENT, &Color::WHITE, 0.5),
            half_white
        );
        assert_eq!(
            Color::lerp(&Color::WHITE, &Color::TRANSPARENT, 0.5),
            half_white
        );
    }

    /// Premultiplying changes nothing where nothing is translucent: between
    /// two opaque colours the lerp is the straight one, to the bit. An
    /// overshoot past either end is clamped into `[0, 1]`, as CSS clamps an
    /// interpolated colour.
    #[test]
    fn between_opaque_colours_the_lerp_is_channel_by_channel() {
        let pairs = [
            (Color::BLACK, Color::WHITE),
            (Color::RED, Color::BLUE),
            (Color::rgb(0.2, 0.4, 0.6), Color::from_hex(0x3a7bd5)),
        ];
        for (from, to) in pairs {
            for t in [0.0, 0.1, 0.25, 0.5, 0.9, 1.0, 1.3, -0.2] {
                let f = |a: f32, b: f32| a + (b - a) * t;
                let straight = Color::rgba(f(from.r, to.r), f(from.g, to.g), f(from.b, to.b), 1.0);
                let expected = if (0.0..=1.0).contains(&t) {
                    straight
                } else {
                    let c = |x: f32| x.clamp(0.0, 1.0);
                    Color::rgba(c(straight.r), c(straight.g), c(straight.b), 1.0)
                };
                assert_eq!(
                    Color::lerp(&from, &to, t),
                    expected,
                    "{from:?} -> {to:?} at {t}"
                );
            }
        }
    }

    /// Where the alpha comes out at zero or below — two transparent ends, or a
    /// spring overshooting a fade out — there is no premultiplied colour to
    /// divide back, and the lerp still answers a colour rather than a NaN.
    #[test]
    fn a_colour_with_no_alpha_left_is_still_a_number() {
        let clear_red = Color::rgba(1.0, 0.0, 0.0, 0.0);
        for (from, to, t) in [
            (Color::TRANSPARENT, clear_red, 0.5),
            (Color::WHITE, Color::TRANSPARENT, 1.0),
            (Color::WHITE, Color::TRANSPARENT, 1.2),
            (Color::TRANSPARENT, Color::WHITE, -0.1),
        ] {
            let c = Color::lerp(&from, &to, t);
            assert!(
                c.channels().iter().all(|x| x.is_finite()),
                "{from:?} -> {to:?} at {t} gave {c:?}"
            );
        }
        assert_eq!(
            Color::lerp(&Color::TRANSPARENT, &clear_red, 0.5),
            Color::rgba(0.5, 0.0, 0.0, 0.0)
        );
    }

    /// A spring overshooting a fade toward a nearly transparent colour takes
    /// the mixed alpha through zero — white to black at 0.1 does at t ≈ 1.11.
    /// Unclamped, the premultiplied channels just before it are below zero
    /// and darken what is behind, and just after it nothing is drawn: a step
    /// of about 0.11 over white in one frame. Clamped, as CSS clamps, the
    /// colour drawn over white moves by no more than the alpha does.
    #[test]
    fn an_overshoot_through_zero_alpha_does_not_jump() {
        let faint = Color::rgba(0.0, 0.0, 0.0, 0.1);
        let over_white = |c: Color| {
            let a = c.a.clamp(0.0, 1.0);
            c.r * a + (1.0 - a)
        };
        let mut previous = over_white(Color::lerp(&Color::WHITE, &faint, 1.0));
        for step in 1..=300 {
            let t = 1.0 + step as f32 * 0.001;
            let c = Color::lerp(&Color::WHITE, &faint, t);
            let drawn = over_white(c);
            assert!(
                (drawn - previous).abs() < 0.002,
                "at {t}: {previous} -> {drawn}"
            );
            previous = drawn;
        }
        for t in [1.05, 1.11, 1.2] {
            let c = Color::lerp(&Color::WHITE, &faint, t);
            assert!(
                (0.0..=1.0).contains(&c.a)
                    && [c.r, c.g, c.b].iter().all(|x| (0.0..=1.0).contains(x)),
                "at {t} it is clamped: {c:?}"
            );
        }
    }

    /// A spring carried through a clamped value keeps a finite speed: the
    /// clamp moves the value, not the spring, so a retarget from a clamped
    /// colour reads a direction rather than sticking or dividing by nothing.
    #[test]
    fn a_retarget_from_a_clamped_colour_carries_a_finite_speed() {
        let faint = Color::rgba(0.0, 0.0, 0.0, 0.1);
        let clamped = Color::lerp(&Color::WHITE, &faint, 1.2);
        assert_eq!(clamped.a, 0.0, "past the crossing nothing shows");
        let carried = carry_velocity(3.0, &Color::WHITE, &faint, &clamped, &Color::WHITE);
        assert!(carried.is_finite() && carried < 0.0, "got {carried}");
    }

    /// A spring's momentum is read in the space its lerp is straight in. A
    /// fade in turned back halfway has half the way to go back, so the same
    /// speed is twice the normalised one, reversed. Read in straight channels
    /// the way back is a different direction — the colour channels fall while
    /// the alpha falls by half as much — and the spring left at about half the
    /// speed it had.
    #[test]
    fn a_fade_turned_back_halfway_keeps_its_speed() {
        let halfway = Color::lerp(&Color::TRANSPARENT, &Color::WHITE, 0.5);
        let carried = carry_velocity(
            2.0,
            &Color::TRANSPARENT,
            &Color::WHITE,
            &halfway,
            &Color::TRANSPARENT,
        );
        assert!((carried + 4.0).abs() < 1e-5, "got {carried}");
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
        assert_eq!(Translate::new(1.0, 2.0).channels().len(), 4);
        // The widest, and so the one the inline capacity is sized from: two
        // offsets, a blur, a spread and four colour components.
        // `carry_velocity` builds four of these per retarget, so a spilled one
        // is four heap allocations per spring. `Corners` is the runner-up at
        // five, and is asserted too so that shrinking the capacity back to it
        // fails here rather than in a profile.
        let inline = Channels::new().inline_size();
        assert_eq!(Shadow::none().channels().len(), 8);
        assert!(
            Shadow::none().channels().len() <= inline,
            "Channels must hold the widest animatable value without spilling"
        );
        assert!(crate::widgets::Corners::SQUARE.channels().len() <= inline);
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

    /// A relative translate interpolates its fraction, not a width it was
    /// never told.
    #[test]
    fn a_relative_translate_lerps_its_fraction() {
        let out = Translate::relative(-1.0, 0.0);
        let mid = Translate::lerp(&Translate::NONE, &out, 0.5);
        assert_eq!(mid, Translate::relative(-0.5, 0.0));
    }
}
