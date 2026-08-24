//! What the compositor can do for us, exposed reactively.
//!
//! Effects like blur are negotiated at runtime: the compositor advertises
//! them, may withdraw them, and may not implement the protocol at all. A
//! widget that wants to adapt — falling back to a plain translucent panel
//! where a blurred one is not on offer — needs to read that as a signal
//! rather than ask once at startup.

use crate::reactive::global::GlobalSignal;
use crate::reactive::{RwSignal, Signal};

/// Compositor-side effects currently on offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompositorEffects {
    /// The compositor can blur what it composites behind a surface
    /// (`ext-background-effect-v1`).
    ///
    /// False when the protocol is missing, when the compositor advertises no
    /// blur capability, or before it has answered.
    pub blur: bool,
}

static EFFECTS: GlobalSignal<CompositorEffects> = GlobalSignal::new(CompositorEffects::default);

fn effects_signal() -> RwSignal<CompositorEffects> {
    EFFECTS.get()
}

/// Reactive view of the compositor's effect capabilities.
///
/// Reading this inside a tracked closure subscribes to capability changes —
/// a compositor may gain or lose blur while the app is running.
pub fn compositor_effects() -> Signal<CompositorEffects> {
    effects_signal().read_only()
}

/// Record the compositor's blur capability. Called by the platform layer.
pub(crate) fn set_blur_capability(blur: bool) {
    effects_signal().update(|effects| effects.blur = blur);
}
