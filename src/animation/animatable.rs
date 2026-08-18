use crate::transform::Transform;
use crate::widgets::{Color, Padding};

/// Trait for types that can be animated by interpolating between values
pub trait Animatable: Copy + PartialEq + Send + Sync + 'static {
    /// Linear interpolation between two values
    /// t = 0.0 returns `from`, t = 1.0 returns `to`
    /// t can exceed [0, 1] range for overshoot effects
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;

    /// Whether transitioning from `from` to `to` is a "reverse" direction.
    /// Used to select the `.reverse()` transition when configured.
    /// - `f32`: value decreasing
    /// - `Transform`: scale decreasing
    /// - `Color`: alpha decreasing, then luminance decreasing
    /// - `Padding`: total padding decreasing
    fn is_reverse(_from: &Self, _to: &Self) -> bool {
        false
    }

    /// How far apart two values are, in whatever unit the type animates in.
    ///
    /// Only the ratio between two distances is ever used, so the unit does not
    /// matter — what matters is that it is proportional to how much of the
    /// animation is left. A spring integrates in a space normalised over its
    /// own segment, and carrying its momentum into a new segment means
    /// rescaling by the two lengths.
    fn distance(from: &Self, to: &Self) -> f32;
}

impl Animatable for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        to < from
    }

    fn distance(from: &Self, to: &Self) -> f32 {
        (to - from).abs()
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

    fn distance(from: &Self, to: &Self) -> f32 {
        // The largest channel move: a colour that has travelled most of the
        // way in red and none in blue is most of the way there.
        (to.r - from.r)
            .abs()
            .max((to.g - from.g).abs())
            .max((to.b - from.b).abs())
            .max((to.a - from.a).abs())
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

    fn distance(from: &Self, to: &Self) -> f32 {
        (to.left - from.left)
            .abs()
            .max((to.right - from.right).abs())
            .max((to.top - from.top).abs())
            .max((to.bottom - from.bottom).abs())
    }
}

impl Animatable for Transform {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let mut data = [0.0f32; 6];
        for (i, val) in data.iter_mut().enumerate() {
            *val = from.data[i] + (to.data[i] - from.data[i]) * t;
        }
        Transform { data }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        to.extract_scale() < from.extract_scale()
    }

    fn distance(from: &Self, to: &Self) -> f32 {
        // Over the matrix the animation actually interpolates, since that is
        // the space the spring is normalised in. The translation terms are in
        // pixels and the rest is unitless, so this is not a length in any
        // geometric sense — only a ratio between two of them is ever used.
        from.data
            .iter()
            .zip(to.data.iter())
            .map(|(a, b)| (b - a).abs())
            .fold(0.0f32, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
