//! Fractional scaling (`wp_fractional_scale_v1` + `wp_viewporter`).
//!
//! An output at 1.5 has no integer to offer. `wl_surface.set_buffer_scale`
//! takes one, so the compositor rounds its preference **up** and sends 2
//! through `wl_surface.preferred_buffer_scale` — which is the number
//! smithay-client-toolkit can carry, its `SurfaceData::scale_factor` being an
//! `AtomicI32`. A surface that believes it renders 1.8 times the pixels it
//! needs, and rasterises its glyphs at a scale they are not shown at.
//!
//! The pair of protocols here is how a client says the real number. The
//! compositor names a scale in 120ths on a `wp_fractional_scale_v1`; the
//! surface draws a buffer of `logical * scale` and declares, through a
//! `wp_viewport` destination, the logical size that buffer stands for. The
//! buffer scale stays 1 throughout — the protocol requires it, and it is also
//! the only way the two mechanisms do not multiply.
//!
//! Both globals are optional and neither is useful alone, so a surface holds
//! one [`SurfaceScaling`] or none. Without it nothing here runs and the integer
//! `set_buffer_scale` path is exactly what it was.
//!
//! winit binds these two directly on the same toolkit and for the same reason;
//! swaybg, a layer-shell client from the sway project, takes the same shape.

use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle, delegate_noop, globals::GlobalList, protocol::wl_surface,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use super::wayland::WaylandState;
use crate::surface::SurfaceId;

/// The denominator `wp_fractional_scale_v1` names its scales over, and the only
/// one it has: the event carries "the numerator of a fraction with a
/// denominator of 120".
const SCALE_DENOMINATOR: u32 = 120;

/// The scale a compositor's `preferred_scale` asks for.
fn scale_from_preferred(preferred: u32) -> f32 {
    preferred as f32 / SCALE_DENOMINATOR as f32
}

/// The compositor's two globals.
///
/// One value rather than two `Option`s, because a surface can do nothing with
/// either alone: a scale it cannot express, or a destination nobody asked it
/// for. So [`WaylandState`] holds `Option<Scaling>` and "both or neither" is
/// the type rather than a rule every reader has to re-check.
pub struct Scaling {
    manager: WpFractionalScaleManagerV1,
    viewporter: WpViewporter,
}

/// One surface's half: the object the compositor names a scale through, and
/// the viewport that says what its buffer stands for.
///
/// Both or neither, for the reason above — which is why they are a struct and
/// not two fields on the surface state that three creation sites would each
/// have to keep in step.
pub struct SurfaceScaling {
    fractional: WpFractionalScaleV1,
    viewport: WpViewport,
}

impl SurfaceScaling {
    /// Destroy both objects. Called before the `wl_surface` they hang from
    /// goes away, as the blur proxy is.
    pub(super) fn destroy(self) {
        self.fractional.destroy();
        self.viewport.destroy();
    }
}

impl Scaling {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<WaylandState>) -> Option<Self> {
        let manager: Option<WpFractionalScaleManagerV1> = globals.bind(qh, 1..=1, ()).ok();
        let viewporter: Option<WpViewporter> = globals.bind(qh, 1..=1, ()).ok();

        // Which of the two is missing is worth saying once, here, where both
        // answers are still in hand — the value that comes out cannot hold a
        // half of this.
        let (Some(manager), Some(viewporter)) = (manager, viewporter) else {
            log::info!(
                "fractional scaling unavailable - surfaces will render at the \
                 compositor's preferred integer scale"
            );
            return None;
        };

        Some(Self {
            manager,
            viewporter,
        })
    }
}

impl WaylandState {
    /// Give a surface a fractional scale and a viewport, where the compositor
    /// offers both.
    ///
    /// Called by every creation path — layer, popup and lock — before the
    /// surface's first commit, so the scale the compositor answers with arrives
    /// alongside the first configure rather than after the first frame has
    /// been drawn at the wrong size.
    pub(super) fn create_surface_scaling(
        &self,
        id: SurfaceId,
        wl_surface: &wl_surface::WlSurface,
    ) -> Option<SurfaceScaling> {
        let scaling = self.scaling.as_ref()?;
        Some(SurfaceScaling {
            fractional: scaling
                .manager
                .get_fractional_scale(wl_surface, &self.qh, id),
            viewport: scaling.viewporter.get_viewport(wl_surface, &self.qh, ()),
        })
    }

    /// Declare the logical size this surface's buffer stands for.
    ///
    /// A request and not a commit: it is latched by the `present` that attaches
    /// the buffer it describes, which is what keeps the two in step — see
    /// `resolve_geometry`, which is the only caller.
    pub(crate) fn set_viewport_destination(&self, id: SurfaceId, width: u32, height: u32) {
        let Some(scaling) = self.surfaces.get(&id).and_then(|s| s.scaling.as_ref()) else {
            return;
        };
        scaling
            .viewport
            .set_destination(width as i32, height as i32);
    }
}

impl Dispatch<WpFractionalScaleV1, SurfaceId> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        id: &SurfaceId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else {
            return;
        };
        let Some(surface_state) = state.surfaces.get_mut(id) else {
            return;
        };

        let scale = scale_from_preferred(scale);
        if surface_state.adopt_scale(scale) {
            log::info!("Surface {id:?} preferred fractional scale is {scale}");
        }
    }
}

delegate_noop!(WaylandState: ignore WpFractionalScaleManagerV1);
delegate_noop!(WaylandState: ignore WpViewporter);
delegate_noop!(WaylandState: ignore WpViewport);

#[cfg(test)]
mod tests {
    use super::*;

    /// The event carries a numerator and never a denominator, so the
    /// denominator is a constant that has to be the right one. 120 divides by
    /// 2, 3, 4, 5, 6 and 8, which is why every scale a compositor offers is
    /// exact in it — and why these are exact here rather than approximate.
    #[test]
    fn a_preferred_scale_is_its_numerator_over_a_hundred_and_twenty() {
        assert_eq!(scale_from_preferred(120), 1.0);
        assert_eq!(scale_from_preferred(180), 1.5);
        assert_eq!(scale_from_preferred(240), 2.0);
    }
}
