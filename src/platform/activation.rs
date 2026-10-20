//! `xdg_activation_v1`: tokens that let another application take the focus.
//!
//! A token is asked for against the serial of the latest input the seat sent,
//! which is what a compositor checks it against: niri honours one whose serial
//! is no older than the pointer's or the keyboard's last enter, so a bar that
//! never takes the keyboard still gets one for a click.

use rustc_hash::FxHashMap;
use smithay_client_toolkit::activation::{ActivationHandler, ActivationState, RequestData};
use smithay_client_toolkit::globals::ProvidesBoundGlobal;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::XdgActivationTokenV1;

use super::wayland::WaylandState;
use crate::surface::SurfaceId;

/// The global, and the token objects waiting for their `done`.
///
/// sctk's `request_token` keeps no handle on the object it creates and its
/// dispatch never destroys it, so a bar asking on every tray click would leave
/// one behind in the compositor each time. The requests are made here instead,
/// and each object is destroyed once its token has arrived, as winit does.
pub(crate) struct Activation {
    state: ActivationState,
    in_flight: FxHashMap<u64, XdgActivationTokenV1>,
}

impl Activation {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<WaylandState>) -> Option<Self> {
        let Ok(state) = ActivationState::bind(globals, qh) else {
            log::info!("xdg-activation not available - activation tokens will not be issued");
            return None;
        };
        Some(Self {
            state,
            in_flight: FxHashMap::default(),
        })
    }
}

impl WaylandState {
    /// Ask for a token for `surface`, answered under `request` when it lands,
    /// or at once with `None` where the compositor has no `xdg_activation_v1`.
    pub(crate) fn request_activation_token(&mut self, surface: SurfaceId, request: u64) {
        let Some(activation) = &mut self.activation else {
            crate::activation::answer(request, None);
            return;
        };
        let Ok(global) = activation.state.bound_global() else {
            crate::activation::answer(request, None);
            return;
        };
        let seat = self.seat_state.seats().next();
        let wl_surface = self.surfaces.get(&surface).map(|s| s.wl_surface.clone());
        let token = global.get_activation_token(
            &self.qh,
            RequestData {
                app_id: None,
                seat_and_serial: None,
                surface: None,
                udata: request,
            },
        );
        if let Some(seat) = &seat {
            token.set_serial(self.input.latest_input_serial, seat);
        }
        if let Some(wl_surface) = &wl_surface {
            token.set_surface(wl_surface);
        }
        token.commit();
        activation.in_flight.insert(request, token);
    }
}

impl ActivationHandler for WaylandState {
    type RequestUdata = u64;

    fn new_token(&mut self, token: String, data: &RequestData<u64>) {
        if let Some(proxy) = self
            .activation
            .as_mut()
            .and_then(|activation| activation.in_flight.remove(&data.udata))
        {
            proxy.destroy();
        }
        crate::activation::answer(data.udata, Some(token));
    }
}
