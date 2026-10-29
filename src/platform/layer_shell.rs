//! `zwlr_layer_shell_v1`, bound by guido rather than by smithay-client-toolkit.
//!
//! sctk binds the global at version 4 at most, and the type holding it is
//! private, so a layer surface made through it can never send a request of
//! version 5: `set_exclusive_edge`, without which a surface stretched between
//! two parallel edges reserves nothing (#909). This is the part of sctk's
//! `wlr_layer` module guido used, bound at the version the compositor offers.
//! Its `Anchor`, `Layer` and `KeyboardInteractivity` do not depend on who
//! bound the global, so they are still sctk's.

use std::cell::Cell;

use smithay_client_toolkit::compositor::Surface;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    globals::{BindError, GlobalList},
    protocol::{wl_output, wl_surface},
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_popup::XdgPopup;
use smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::wayland::WaylandState;
use crate::surface::{Edge, Reservation, SurfaceId};

/// The first version with `set_exclusive_edge`.
const EXCLUSIVE_EDGE_SINCE: u32 = 5;

/// The bound `zwlr_layer_shell_v1` global.
#[derive(Debug)]
pub struct LayerShell {
    global: ZwlrLayerShellV1,
}

impl LayerShell {
    /// Bind the global at the highest version both sides know, up to 5.
    pub(crate) fn bind(
        globals: &GlobalList,
        qh: &QueueHandle<WaylandState>,
    ) -> Result<Self, BindError> {
        Ok(Self {
            global: globals.bind(qh, 1..=EXCLUSIVE_EDGE_SINCE, ())?,
        })
    }

    /// Give `surface` the layer-shell role. Its events are dispatched with
    /// `id`, so they need no lookup to find the surface they are for.
    pub(crate) fn create_layer_surface(
        &self,
        qh: &QueueHandle<WaylandState>,
        surface: wl_surface::WlSurface,
        layer: Layer,
        namespace: String,
        output: Option<&wl_output::WlOutput>,
        id: SurfaceId,
    ) -> LayerSurface {
        let role = self
            .global
            .get_layer_surface(&surface, output, layer.into(), namespace, qh, id);
        LayerSurface {
            role,
            surface: Surface::from(surface),
            asked_edge: Cell::new(None),
        }
    }
}

/// A `wl_surface` with the layer-shell role. Dropping it destroys both, the
/// role first, as the protocol requires.
#[derive(Debug)]
pub struct LayerSurface {
    role: ZwlrLayerSurfaceV1,
    surface: Surface,
    /// The exclusive edge last asked for: none until one is, as the protocol
    /// starts it. Kept on a compositor too old to be sent it as well, so that
    /// it is refused once rather than on every resize.
    asked_edge: Cell<Option<Edge>>,
}

impl LayerSurface {
    /// The surface this role was given to.
    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        self.surface.wl_surface()
    }

    pub(crate) fn get_popup(&self, popup: &XdgPopup) {
        self.role.get_popup(popup);
    }

    pub(crate) fn set_size(&self, width: u32, height: u32) {
        self.role.set_size(width, height);
    }

    pub(crate) fn set_anchor(&self, anchor: Anchor) {
        // The protocol's bits, so one is the other.
        self.role
            .set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                anchor.bits(),
            ));
    }

    /// Ask for a reservation: the zone, and the edge it applies to where it
    /// changed. Before version 5 there is no edge to send, and the zone is what
    /// the anchor makes of it.
    ///
    /// After the anchor, always: Hyprland checks the edge against the anchor
    /// as the request arrives, not at the commit, so an edge sent before the
    /// anchor that makes it valid ends the connection. 0 clears it, as smithay,
    /// KWin and Hyprland all read it.
    pub(crate) fn set_reservation(&self, reservation: Reservation) {
        self.role.set_exclusive_zone(reservation.zone);
        if reservation.edge == self.asked_edge.replace(reservation.edge) {
            return;
        }
        if self.role.version() >= EXCLUSIVE_EDGE_SINCE {
            let edge = reservation.edge.map_or(Anchor::empty(), Edge::anchor);
            self.role
                .set_exclusive_edge(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                    edge.bits(),
                ));
        } else if let Some(edge) = reservation.edge {
            log::warn!(
                "the compositor's layer shell is version {}, before \
                 set_exclusive_edge; {edge:?} is not sent, and a zone on a \
                 stretched surface counts for nothing",
                self.role.version()
            );
        }
    }

    pub(crate) fn set_margin(&self, top: i32, right: i32, bottom: i32, left: i32) {
        self.role.set_margin(top, right, bottom, left);
    }

    pub(crate) fn set_keyboard_interactivity(&self, value: KeyboardInteractivity) {
        self.role.set_keyboard_interactivity(value.into());
    }

    pub(crate) fn set_layer(&self, layer: Layer) {
        self.role.set_layer(layer.into());
    }
}

impl Drop for LayerSurface {
    fn drop(&mut self) {
        // `surface` destroys the wl_surface after this, as fields drop in order.
        self.role.destroy();
    }
}

delegate_noop!(WaylandState: ZwlrLayerShellV1);

impl Dispatch<ZwlrLayerSurfaceV1, SurfaceId> for WaylandState {
    fn event(
        state: &mut Self,
        role: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        id: &SurfaceId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                role.ack_configure(serial);
                state.layer_configured(*id, width, height);
            }
            zwlr_layer_surface_v1::Event::Closed => state.layer_closed(*id),
            _ => {}
        }
    }
}
