//! How a blur is run: the one sum flatten, the layers and the backdrop need.
//!
//! Flatten has to know how far a blurred layer spreads past its content,
//! because that is what the layer covers when the next draw is tested against
//! it; the renderer has to run exactly that blur. One function, so the bounds
//! a draw group splits on and the pixels the blur writes cannot disagree.

/// Cap on gaussian taps per axis. Beyond this the shrink is doing the work
/// anyway, and an unbounded loop would let one huge radius stall a frame.
pub(crate) const MAX_TAPS: f32 = 24.0;

/// Frames a texture nothing drew into is kept for before it is released —
/// the backdrop's target and a layer's alike. Not released on the first such
/// frame: a menu that blurs while open, or a popup that fades in every time it
/// opens, would otherwise allocate again every time.
pub(crate) const IDLE_FRAMES_BEFORE_RELEASE: u32 = 120;

/// The passes a blur of a given radius takes.
///
/// The backdrop blur's recipe — shrink, then two separable gaussian passes in
/// the shrunk texture — with the shrink chosen by the radius rather than
/// fixed. A backdrop is frosted glass and nobody reads the texels under it;
/// a layer's blur is often small, a softening of text or an icon on its way in,
/// and a quarter-size working texture would draw a two-pixel blur as a blocky
/// eight. So the shrink grows with the radius: none below a sigma of two
/// pixels, half up to eight, a quarter past it — where the backdrop always
/// works — and further only where the kernel would otherwise not fit in the
/// taps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlurPlan {
    /// How many physical pixels one working texel stands for, each way.
    pub(crate) shrink: u32,
    /// The gaussian's sigma, in working texels.
    pub(crate) sigma: f32,
    /// Taps either side of the centre, per axis.
    pub(crate) taps: f32,
}

impl BlurPlan {
    /// The blur `radius` logical pixels asks for at `scale`, or `None` for one
    /// that blurs nothing.
    ///
    /// The radius means what a backdrop blur's means: the sigma is half of it,
    /// in physical pixels.
    pub(crate) fn new(radius: f32, scale: f32) -> Option<Self> {
        if radius.is_nan() || radius <= 0.0 {
            return None;
        }
        let sigma_px = radius * scale / 2.0;
        let mut shrink: u32 = if sigma_px < 2.0 {
            1
        } else if sigma_px < 8.0 {
            2
        } else {
            4
        };
        // And further, for a radius whose kernel the cap on taps would cut
        // short: two and a half sigmas have to fit in the taps.
        while sigma_px * 2.5 / shrink as f32 > MAX_TAPS {
            shrink *= 2;
        }
        Some(Self::shrunk(radius, scale, shrink))
    }

    /// The blur `radius` asks for at `scale` with the shrink fixed — what the
    /// backdrop always takes, at a quarter.
    pub(crate) fn shrunk(radius: f32, scale: f32, shrink: u32) -> Self {
        let sigma = (radius * scale / shrink as f32 / 2.0).max(0.5);
        let taps = (sigma * 2.5).ceil().min(MAX_TAPS);
        Self {
            shrink,
            sigma,
            taps,
        }
    }

    /// How far the blur carries a pixel, in physical pixels: the taps either
    /// side, and one working texel more for the bilinear read that puts it back
    /// at full size.
    pub(crate) fn reach(&self) -> u32 {
        (self.taps as u32 + 1) * self.shrink
    }
}

/// How far past its content a layer blurred by `radius` draws, in logical
/// pixels. Zero for no blur.
pub(crate) fn blur_margin(radius: f32, scale: f32) -> f32 {
    BlurPlan::new(radius, scale).map_or(0.0, |plan| plan.reach() as f32 / scale)
}

/// A reach no blur of `radius` exceeds at any scale of one or more, in logical
/// pixels — what a container's damage is sized by, ahead of knowing which
/// surface it will be drawn on.
///
/// One and a quarter of the radius is two and a half sigmas; rounding the
/// taps up and the shrink to a power of two adds at most half the radius again
/// and eight pixels.
pub(crate) fn blur_reach_bound(radius: f32) -> f32 {
    radius.max(0.0) * 1.75 + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small blur is not shrunk at all, a large one is shrunk as far as the
    /// backdrop's, and a huge one further — so the cap on taps never cuts a
    /// kernel short of two and a half sigmas.
    #[test]
    fn the_shrink_grows_with_the_radius() {
        assert_eq!(BlurPlan::new(2.0, 1.0).unwrap().shrink, 1);
        assert_eq!(BlurPlan::new(8.0, 1.0).unwrap().shrink, 2);
        assert_eq!(BlurPlan::new(8.0, 2.0).unwrap().shrink, 4);
        assert!(BlurPlan::new(0.0, 1.0).is_none());
        for radius in [8.0, 30.0, 60.0, 400.0] {
            let plan = BlurPlan::new(radius, 2.0).unwrap();
            assert!(
                plan.taps >= 2.5 * plan.sigma && plan.taps <= MAX_TAPS,
                "the kernel reaches two and a half sigmas: {plan:?}"
            );
        }
    }

    /// The margin is the reach, so a blurred layer's bounds cover every pixel
    /// its blur writes.
    #[test]
    fn the_margin_is_the_reach_in_logical_pixels() {
        let plan = BlurPlan::new(8.0, 2.0).unwrap();
        assert_eq!(blur_margin(8.0, 2.0), plan.reach() as f32 / 2.0);
        assert_eq!(blur_margin(0.0, 2.0), 0.0);
    }

    /// The bound a container's damage is sized by holds at every scale a
    /// surface is drawn at.
    #[test]
    fn no_blur_reaches_past_its_bound() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            for step in 0..400 {
                let radius = step as f32 * 0.5;
                assert!(
                    blur_margin(radius, scale) <= blur_reach_bound(radius),
                    "a blur of {radius} at {scale} reaches {} past the bound {}",
                    blur_margin(radius, scale),
                    blur_reach_bound(radius)
                );
            }
        }
    }
}
