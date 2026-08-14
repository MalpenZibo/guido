//! Backdrop effects: filtering what is already behind a container.
//!
//! There are two things behind a container, and they live on opposite sides
//! of the Wayland surface:
//!
//! - what *this surface* has already drawn — a wallpaper, a photo, the panel
//!   underneath a card;
//! - what the *compositor* composites below the surface — the desktop showing
//!   through wherever the surface is translucent.
//!
//! [`backdrop_blur`](crate::widgets::Container::backdrop_blur) blurs both, and
//! that is not a compromise between two mechanisms: blur is a linear
//! operator, so where the surface has a uniform alpha `a` over a region,
//!
//! ```text
//! blur(a·ours + (1−a)·theirs) = a·blur(ours) + (1−a)·blur(theirs)
//! ```
//!
//! and blurring each layer separately, then compositing, is *exactly* the
//! same result. Which is why the two are never chosen between — a translucent
//! panel is neither "ours" nor "theirs" at any pixel, so a per-box choice
//! could only be wrong for half of them.
//!
//! The one place the decomposition is approximate is where alpha *varies*
//! within the blur radius, at the edges of opaque content inside the box:
//! neither blur bleeds into the other's layer. That cannot be fixed from
//! here, or from any design — `ext-background-effect-v1` is fire-and-forget
//! (`set_blur_region` takes a region and nothing comes back), so the
//! compositor's pixels are never ours to filter across.

use bitflags::bitflags;

bitflags! {
    /// Which backdrop a blur reaches.
    ///
    /// Both by default. Restricting is an aesthetic choice, not a way to pick
    /// a mechanism: `COMPOSITOR` alone leaves the surface's own content crisp
    /// under a translucent panel while the desktop behind it softens.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct BackdropSources: u8 {
        /// What this surface has already drawn.
        const SURFACE = 1;
        /// What the compositor composites behind this surface, via
        /// `ext-background-effect-v1`. The protocol carries no radius — the
        /// compositor picks its own — so `radius` does not apply here.
        const COMPOSITOR = 2;
    }
}

impl Default for BackdropSources {
    fn default() -> Self {
        Self::all()
    }
}

/// A backdrop blur request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropBlur {
    /// Blur radius in logical pixels, for the surface's own content.
    pub radius: f32,
    pub sources: BackdropSources,
}

impl BackdropBlur {
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            sources: BackdropSources::default(),
        }
    }

    /// Restrict which backdrops this blur reaches.
    pub fn sources(mut self, sources: BackdropSources) -> Self {
        self.sources = sources;
        self
    }
}

impl From<f32> for BackdropBlur {
    fn from(radius: f32) -> Self {
        Self::new(radius)
    }
}

impl From<i32> for BackdropBlur {
    fn from(radius: i32) -> Self {
        Self::new(radius as f32)
    }
}
