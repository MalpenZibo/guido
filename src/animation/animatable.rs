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
}

impl Animatable for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        to < from
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
        let to_lum = to.r * 0.299 + to.g * 0.587 + to.b * 0.114;
        let from_lum = from.r * 0.299 + from.g * 0.587 + from.b * 0.114;
        (to.a, to_lum) < (from.a, from_lum)
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
}

impl Animatable for Transform {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let mut data = [0.0f32; 16];
        for (i, val) in data.iter_mut().enumerate() {
            *val = from.data[i] + (to.data[i] - from.data[i]) * t;
        }
        Transform { data }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        to.extract_scale() < from.extract_scale()
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
