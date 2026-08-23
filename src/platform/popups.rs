//! xdg popups anchored to a layer surface.
//!
//! Unlike every other surface here, a popup does not choose where it goes: the
//! client describes an anchor, a gravity and how it may be adjusted, and the
//! compositor decides, so a menu near a screen edge flips instead of falling
//! off. That answer arrives as a configure, which is also why growing a popup
//! means asking to be repositioned rather than simply resizing.

use smithay_client_toolkit::{
    delegate_xdg_popup,
    shell::xdg::{
        XdgPositioner, XdgShell,
        popup::{Popup, PopupConfigure, PopupHandler},
    },
};

use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner;

use super::wayland::{SurfaceRole, WaylandState, WaylandSurfaceState};
use crate::surface::SurfaceId;

/// The xdg shell and the state popups need across configures.
pub struct Popups {
    /// `None` where xdg_wm_base is unavailable — popups then fail with a log.
    pub(super) xdg_shell: Option<XdgShell>,
    /// Monotonic token for xdg_popup.reposition requests.
    pub(super) reposition_token: u32,
}

impl Popups {
    pub(super) fn new(xdg_shell: Option<XdgShell>) -> Self {
        Self {
            xdg_shell,
            reposition_token: 0,
        }
    }
}

impl WaylandState {
    /// Build an xdg_positioner for a popup config at the given size.
    fn build_popup_positioner(
        xdg_shell: &XdgShell,
        config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> Option<XdgPositioner> {
        let positioner = match XdgPositioner::new(xdg_shell) {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to create xdg_positioner: {e}");
                return None;
            }
        };
        positioner.set_size(size.0.max(1) as i32, size.1.max(1) as i32);
        let r = config.anchor_rect;
        positioner.set_anchor_rect(
            r.x.floor() as i32,
            r.y.floor() as i32,
            (r.width.ceil() as i32).max(1),
            (r.height.ceil() as i32).max(1),
        );
        positioner.set_anchor(popup_point_to_anchor(config.anchor));
        positioner.set_gravity(popup_point_to_gravity(config.gravity));
        positioner.set_offset(config.offset.0, config.offset.1);
        // Let the compositor keep the popup on screen: flip at the screen
        // edge on either axis, slide where flipping doesn't help.
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::FlipX
                | xdg_positioner::ConstraintAdjustment::FlipY
                | xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::SlideY,
        );
        Some(positioner)
    }

    /// Create an xdg popup anchored to `parent` with a specific SurfaceId.
    ///
    /// `size` is the resolved popup size (config height, or the measured
    /// content height for auto-height popups). The actual size/position
    /// arrive with the popup configure (the compositor may flip/slide it to
    /// stay on screen). Returns false when xdg_wm_base is missing or the
    /// parent can't host popups.
    pub fn create_popup_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        parent: SurfaceId,
        config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        let Some(ref xdg_shell) = self.popups.xdg_shell else {
            log::error!("Cannot create popup: compositor lacks xdg_wm_base");
            return false;
        };
        let Some(parent_state) = self.surfaces.get(&parent) else {
            log::warn!("Cannot create popup: parent surface {:?} is gone", parent);
            return false;
        };

        let Some(positioner) = Self::build_popup_positioner(xdg_shell, config, size) else {
            return false;
        };

        let wl_surface = self.compositor_state.create_surface(qh);
        let popup = match &parent_state.role {
            SurfaceRole::Layer(layer_surface) => {
                let popup =
                    match Popup::from_surface(None, &positioner, qh, wl_surface.clone(), xdg_shell)
                    {
                        Ok(popup) => popup,
                        Err(e) => {
                            log::error!("Failed to create xdg popup: {e}");
                            return false;
                        }
                    };
                // Assign the layer surface as the popup parent BEFORE the
                // initial commit (required for parentless xdg popups)
                layer_surface.get_popup(popup.xdg_popup());
                popup
            }
            SurfaceRole::Popup {
                popup: parent_popup,
                ..
            } => {
                // Nested popup (submenu): ordinary xdg parent
                let parent_xdg = parent_popup.xdg_surface().clone();
                match Popup::from_surface(
                    Some(&parent_xdg),
                    &positioner,
                    qh,
                    wl_surface.clone(),
                    xdg_shell,
                ) {
                    Ok(popup) => popup,
                    Err(e) => {
                        log::error!("Failed to create nested xdg popup: {e}");
                        return false;
                    }
                }
            }
            SurfaceRole::Lock(_) => {
                log::warn!("Cannot anchor a popup to a session-lock surface");
                return false;
            }
        };

        // Menu semantics: an input grab dismisses the popup on outside
        // click. Must be requested before mapping, with a recent serial.
        if config.grab
            && let Some(seat) = self.seat_state.seats().next()
        {
            popup
                .xdg_popup()
                .grab(&seat, self.input.latest_input_serial);
        }

        wl_surface.commit();

        self.surface_lookup.insert(wl_surface.id(), id);
        let surface_state = WaylandSurfaceState::new(
            SurfaceRole::Popup {
                popup,
                config: config.clone(),
                parent,
            },
            wl_surface,
            size.0,
            size.1,
        );
        self.surfaces.insert(id, surface_state);

        log::info!(
            "Created popup {:?} on parent {:?} ({}x{}, grab: {})",
            id,
            parent,
            size.0,
            size.1,
            config.grab
        );
        true
    }

    /// Live popup descendants of `root`, deepest first — the order the
    /// protocol demands for teardown (a popup must be destroyed before its
    /// parent, or the compositor raises `not_the_topmost_popup`).
    pub(crate) fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        let mut frontier = vec![root];
        while let Some(current) = frontier.pop() {
            for (id, state) in &self.surfaces {
                if let SurfaceRole::Popup { parent, .. } = &state.role
                    && *parent == current
                {
                    out.push(*id);
                    frontier.push(*id);
                }
            }
        }
        out.reverse(); // deepest first
        out
    }

    /// Grabbing popups that would make a new grab on `new_parent` illegal:
    /// xdg-shell requires a new grab to nest under the current grab holder,
    /// so any live grabbing popup that is not `new_parent` itself (or one
    /// of its ancestors) must be destroyed before the new popup is created.
    /// Returned deepest-chain-first, ready for ordered teardown.
    pub(crate) fn conflicting_grab_popups(&self, new_parent: SurfaceId) -> Vec<SurfaceId> {
        // Ancestor chain of the new popup (surfaces a nested grab may sit on)
        let mut ancestors = vec![new_parent];
        let mut current = new_parent;
        while let Some(state) = self.surfaces.get(&current) {
            match &state.role {
                SurfaceRole::Popup { parent, .. } => {
                    ancestors.push(*parent);
                    current = *parent;
                }
                _ => break,
            }
        }

        let mut conflicts: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(id, state)| {
                matches!(&state.role, SurfaceRole::Popup { config, .. } if config.grab)
                    && !ancestors.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        // Close whole chains children-first: append descendants and dedup
        let mut ordered = Vec::new();
        for id in conflicts.drain(..) {
            for d in self.popup_descendants_bottom_up(id) {
                if !ordered.contains(&d) {
                    ordered.push(d);
                }
            }
            if !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        ordered
    }

    /// For auto-height popups: the width to measure content at.
    /// Returns `None` for non-popup surfaces or fixed-height popups.
    pub(crate) fn popup_auto_width(&self, id: SurfaceId) -> Option<u32> {
        match &self.surfaces.get(&id)?.role {
            SurfaceRole::Popup { config, .. } if config.height.is_none() => Some(config.width),
            _ => None,
        }
    }

    /// Reposition an auto-height popup when its content height changed.
    /// The compositor answers with a new configure carrying the final size.
    pub(crate) fn reposition_popup_if_changed(
        &mut self,
        id: SurfaceId,
        new_height: u32,
        qh: &QueueHandle<Self>,
    ) {
        let _ = qh;
        let Some(ref xdg_shell) = self.popups.xdg_shell else {
            return;
        };
        let Some(surface_state) = self.surfaces.get_mut(&id) else {
            return;
        };
        if surface_state.height == new_height
            || surface_state.pending_popup_height == Some(new_height)
        {
            return;
        }
        let SurfaceRole::Popup { popup, config, .. } = &surface_state.role else {
            return;
        };
        // xdg_popup.reposition needs protocol v3
        if popup.xdg_popup().version() < 3 {
            return;
        }
        let Some(positioner) =
            Self::build_popup_positioner(xdg_shell, config, (config.width, new_height))
        else {
            return;
        };
        self.popups.reposition_token = self.popups.reposition_token.wrapping_add(1);
        popup.reposition(&positioner, self.popups.reposition_token);
        surface_state.pending_popup_height = Some(new_height);
        log::debug!("Popup {:?} repositioning to height {}", id, new_height);
    }
}

/// Map guido's popup anchor point to the xdg_positioner anchor.
fn popup_point_to_anchor(point: crate::surface::PopupAnchor) -> xdg_positioner::Anchor {
    use crate::surface::PopupAnchor as P;
    match point {
        P::None => xdg_positioner::Anchor::None,
        P::Top => xdg_positioner::Anchor::Top,
        P::Bottom => xdg_positioner::Anchor::Bottom,
        P::Left => xdg_positioner::Anchor::Left,
        P::Right => xdg_positioner::Anchor::Right,
        P::TopLeft => xdg_positioner::Anchor::TopLeft,
        P::BottomLeft => xdg_positioner::Anchor::BottomLeft,
        P::TopRight => xdg_positioner::Anchor::TopRight,
        P::BottomRight => xdg_positioner::Anchor::BottomRight,
    }
}

/// Map guido's popup gravity to the xdg_positioner gravity.
fn popup_point_to_gravity(point: crate::surface::PopupAnchor) -> xdg_positioner::Gravity {
    use crate::surface::PopupAnchor as P;
    match point {
        P::None => xdg_positioner::Gravity::None,
        P::Top => xdg_positioner::Gravity::Top,
        P::Bottom => xdg_positioner::Gravity::Bottom,
        P::Left => xdg_positioner::Gravity::Left,
        P::Right => xdg_positioner::Gravity::Right,
        P::TopLeft => xdg_positioner::Gravity::TopLeft,
        P::BottomLeft => xdg_positioner::Gravity::BottomLeft,
        P::TopRight => xdg_positioner::Gravity::TopRight,
        P::BottomRight => xdg_positioner::Gravity::BottomRight,
    }
}

impl PopupHandler for WaylandState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        // Route to the matching surface state, like the layer-shell path.
        // sctk has already acked; the compositor may have adjusted size and
        // position to keep the popup on screen.
        let surface_id = self.surface_lookup.get(&popup.wl_surface().id()).copied();
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            log::info!(
                "Popup {:?} configure: {}x{} at {:?}",
                id,
                config.width,
                config.height,
                config.position
            );
            if config.width > 0 {
                surface_state.width = config.width as u32;
            }
            if config.height > 0 {
                surface_state.height = config.height as u32;
            }
            surface_state.pending_popup_height = None;
            surface_state.configured = true;
            crate::jobs::wake_loop();
        }
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, popup: &Popup) {
        // Compositor dismissed the popup (outside click on a grab, parent
        // gone). Mark it dismissed for anything watching, and route through
        // the normal close command for full teardown.
        if let Some(id) = self.surface_lookup.get(&popup.wl_surface().id()).copied() {
            log::info!("Popup {:?} dismissed by compositor", id);
            crate::surface::mark_popup_dismissed(id);
            crate::surface::push_surface_command(crate::surface::SurfaceCommand::Close(id));
        }
    }
}

// Not delegate_xdg_shell!: that macro (and sctk's decoration dispatches)
// drag in WindowHandler bounds — guido has no toplevel windows. Popups need
// only xdg_wm_base (ping/pong), plus an inert stub for the decoration
// manager that XdgShell::bind insists on binding (the protocol object has
// no events).
smithay_client_toolkit::reexports::client::delegate_dispatch!(WaylandState: [
    smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_wm_base::XdgWmBase: smithay_client_toolkit::globals::GlobalData
] => XdgShell);

impl
    Dispatch<
        smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
        smithay_client_toolkit::globals::GlobalData,
    > for WaylandState
{
    fn event(
        _: &mut Self,
        _: &smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
        _: smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::Event,
        _: &smithay_client_toolkit::globals::GlobalData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("zxdg_decoration_manager_v1 has no events");
    }
}

delegate_xdg_popup!(WaylandState);
