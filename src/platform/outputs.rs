//! Stable identity for the compositor's outputs.
//!
//! `wl_output` globals come and go as monitors are plugged, unplugged or
//! remoded, and the protocol object itself carries no identity across that.
//! This registry hands each one a `OutputId` that stays put for as long as the
//! global lives, and never reuses it afterwards: a monitor that comes back is
//! a new output, so a surface pinned to the old one does not silently land on
//! it.

use std::collections::HashMap;

use smithay_client_toolkit::{
    delegate_output,
    output::{OutputHandler, OutputState},
};

use smithay_client_toolkit::reexports::client::{
    Connection, Proxy, QueueHandle, protocol::wl_output,
};
use wayland_backend::sys::client::ObjectId;

use super::wayland::WaylandState;
use crate::outputs::{self, OutputId, OutputInfo};

/// The `OutputId` assigned to each live `wl_output`.
pub struct OutputRegistry {
    /// Stable OutputId for each wl_output global. Ids are never reused: a
    /// reconnected monitor gets a fresh id.
    pub(super) output_ids: HashMap<ObjectId, OutputId>,
    /// Next OutputId to allocate.
    pub(super) next_output_id: u32,
}

impl OutputRegistry {
    pub(super) fn new() -> Self {
        Self {
            output_ids: HashMap::new(),
            next_output_id: 0,
        }
    }
}

impl WaylandState {
    /// Get (or allocate) the stable OutputId for a wl_output.
    pub(super) fn ensure_output_id(&mut self, output: &wl_output::WlOutput) -> OutputId {
        let object_id = output.id();
        if let Some(id) = self.outputs.output_ids.get(&object_id) {
            return *id;
        }
        let id = OutputId::from_raw(self.outputs.next_output_id);
        self.outputs.next_output_id += 1;
        self.outputs.output_ids.insert(object_id, id);
        id
    }

    /// Find the wl_output for a stable OutputId, if still connected.
    pub(super) fn wl_output_for(&self, id: OutputId) -> Option<wl_output::WlOutput> {
        self.output_state
            .outputs()
            .find(|o| self.outputs.output_ids.get(&o.id()) == Some(&id))
    }

    /// Rebuild the reactive output list from current compositor state.
    fn sync_outputs(&mut self) {
        let wl_outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let mut list: Vec<OutputInfo> = wl_outputs
            .iter()
            .filter_map(|o| {
                let id = self.ensure_output_id(o);
                let info = self.output_state.info(o)?;
                Some(OutputInfo {
                    id,
                    name: info.name,
                    description: info.description,
                    make: info.make,
                    model: info.model,
                    scale_factor: info.scale_factor,
                    logical_size: info.logical_size,
                    logical_position: info.logical_position,
                })
            })
            .collect();
        list.sort_by_key(|o| o.id);
        outputs::sync_outputs(list);
    }
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let id = self.ensure_output_id(&output);
        log::info!(
            "Output {:?} connected: {:?}",
            id,
            self.output_state.info(&output).and_then(|i| i.name)
        );
        self.sync_outputs();
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        self.sync_outputs();
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(id) = self.outputs.output_ids.remove(&output.id()) {
            log::info!("Output {:?} disconnected", id);
            outputs::output_removed(id);
        }
        self.sync_outputs();
    }
}

delegate_output!(WaylandState);
