//! Compositor-side backdrop blur (`ext-background-effect-v1`).
//!
//! This is the half of `backdrop_blur` the client cannot do itself: blurring
//! what sits *behind* a translucent surface, which only the compositor can
//! see. It is advertised as a capability that can appear and disappear at
//! runtime, so everything here degrades to a no-op rather than failing.

use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop,
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{
        self, Capability as BgCapability, ExtBackgroundEffectManagerV1,
    },
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

use super::wayland::WaylandState;
use crate::blur::BlurRect;
use crate::surface::SurfaceId;

/// The compositor's background-effect manager and what it currently offers.
pub struct Backdrop {
    /// `None` where the protocol is unsupported — the feature simply no-ops.
    pub(super) bg_effect_manager: Option<ExtBackgroundEffectManagerV1>,
    /// Whether the compositor currently advertises the Blur capability.
    pub(super) bg_effect_supports_blur: bool,
}

impl Backdrop {
    pub(super) fn new(bg_effect_manager: Option<ExtBackgroundEffectManagerV1>) -> Self {
        Self {
            bg_effect_manager,
            bg_effect_supports_blur: false,
        }
    }
}

impl WaylandState {
    /// Whether publishing a blur region can do anything at all.
    ///
    /// Asked *before* building one: on a compositor without
    /// `ext-background-effect-v1` — most of them — `sync_blur_region` would
    /// return at its first line, after the caller had already walked the frame's
    /// whole command list to hand it something to ignore.
    pub(crate) fn supports_blur_region(&self) -> bool {
        self.backdrop.bg_effect_supports_blur && self.backdrop.bg_effect_manager.is_some()
    }

    /// Whether this surface has a non-empty region standing with the compositor.
    ///
    /// A frame carrying no compositor blur skips the scan — except this one,
    /// where the scan produces the empty region that withdraws what is still
    /// published. Once withdrawn the answer is `false`, so it happens once.
    pub(crate) fn has_published_blur(&self, id: SurfaceId) -> bool {
        self.surfaces
            .get(&id)
            .and_then(|s| s.blur_region.as_ref())
            .is_some_and(|rects| !rects.is_empty())
    }

    /// Whether a surface that is *not* repainting still owes the compositor a
    /// region, taking the debt as it answers.
    ///
    /// A frame that paints publishes its region from the commands it just
    /// flattened, so the only reason an idle one has to say anything is that the
    /// compositor forgot: the blur capability going away drops its regions, and
    /// they have to be pushed again when it returns. Asking the retained command
    /// list every idle frame instead walked a whole frame's commands, on every
    /// still frame, to rebuild the region that was already published.
    pub(crate) fn take_blur_resync(&mut self, id: SurfaceId) -> bool {
        if !self.supports_blur_region() {
            return false;
        }
        self.surfaces
            .get_mut(&id)
            .is_some_and(|s| std::mem::take(&mut s.blur_resync_owed))
    }

    /// Push a surface's blur region to the compositor if it changed.
    ///
    /// The `set_blur_region` request is double-buffered: with `commit: false`
    /// it rides the buffer commit performed inside the upcoming present, so
    /// region and content change in the same frame. Pass `commit: true` on
    /// paths that skip presenting (e.g. a capability change without repaint).
    ///
    /// Surfaces that never requested blur are left untouched — declaring an
    /// empty region would override compositor-side blur rules (e.g. blur by
    /// namespace). Once a surface has blurred, dropping to zero rects sends
    /// an *empty* region, never NULL: NULL only withdraws our opinion and
    /// lets such a rule blur the whole surface, where an empty region says
    /// "blur exactly nothing".
    pub(crate) fn sync_blur_region(&mut self, id: SurfaceId, rects: Vec<BlurRect>, commit: bool) {
        if !self.backdrop.bg_effect_supports_blur {
            return;
        }
        let Some(manager) = self.backdrop.bg_effect_manager.clone() else {
            return;
        };
        let Some(surface_state) = self.surfaces.get_mut(&id) else {
            return;
        };
        surface_state.blur_resync_owed = false;

        // Never used blur and still doesn't — don't claim the surface.
        if rects.is_empty()
            && surface_state.blur_region.is_none()
            && surface_state.bg_effect_surface.is_none()
        {
            return;
        }

        if surface_state.blur_region.as_deref() == Some(rects.as_slice()) {
            return;
        }

        // Asking the manager twice for the same surface is a protocol error.
        let effect = surface_state.bg_effect_surface.get_or_insert_with(|| {
            manager.get_background_effect(&surface_state.wl_surface, &self.qh, ())
        });

        let Ok(region) = Region::new(&self.compositor_state) else {
            log::warn!("Failed to create wl_region for blur");
            return;
        };
        for r in &rects {
            region.add(r.x, r.y, r.width, r.height);
        }
        effect.set_blur_region(Some(region.wl_region()));

        log::debug!(
            "Surface {:?} blur region set to {} rect(s)",
            id,
            rects.len()
        );
        surface_state.blur_region = Some(rects);

        if commit {
            surface_state.wl_surface.commit();
        }
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            let blur = match flags {
                WEnum::Value(c) => c.contains(BgCapability::Blur),
                WEnum::Unknown(_) => false,
            };
            if blur == state.backdrop.bg_effect_supports_blur {
                return;
            }
            log::info!(
                "Compositor blur capability {}",
                if blur { "available" } else { "lost" }
            );
            state.backdrop.bg_effect_supports_blur = blur;
            crate::compositor::set_blur_capability(blur);

            // The compositor drops its regions when the capability goes away:
            // forget ours and wake the loop to push them again if it's back.
            for surface_state in state.surfaces.values_mut() {
                surface_state.blur_region = None;
                surface_state.blur_resync_owed = true;
            }
            crate::jobs::wake_loop();
        }
    }
}

delegate_noop!(WaylandState: ignore ExtBackgroundEffectSurfaceV1);
