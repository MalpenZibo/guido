//! Stable identity for the compositor's outputs.
//!
//! `wl_output` globals come and go as monitors are plugged, unplugged or
//! remoded, and the protocol object itself carries no identity across that.
//! This registry hands each one a `OutputId` that stays put for as long as the
//! global lives, and never reuses it afterwards: a monitor that comes back is
//! a new output, so a surface pinned to the old one does not silently land on
//! it.

use std::collections::HashMap;
use std::hash::Hash;

use smithay_client_toolkit::{
    delegate_output,
    output::{OutputHandler, OutputState},
};

use super::wayland::WaylandState;
use crate::outputs::{self, OutputId, OutputInfo};
use smithay_client_toolkit::reexports::client::{
    Connection, Proxy, QueueHandle, protocol::wl_output,
};

/// The `OutputId` assigned to each live `wl_output`.
///
/// `K` is whatever identifies a global: the `ObjectId` of its proxy on a real
/// compositor, anything hashable in a test.
pub struct OutputRegistry<K> {
    /// Stable OutputId for each wl_output global. Ids are never reused: a
    /// reconnected monitor gets a fresh id.
    output_ids: HashMap<K, OutputId>,
    /// Next OutputId to allocate.
    next_output_id: u32,
}

impl<K: Eq + Hash> OutputRegistry<K> {
    pub(super) fn new() -> Self {
        Self {
            output_ids: HashMap::new(),
            next_output_id: 0,
        }
    }

    /// Mint the id for a global the compositor has just advertised.
    pub(super) fn add(&mut self, key: K) -> OutputId {
        if let Some(id) = self.output_ids.get(&key) {
            return *id;
        }
        let id = OutputId::from_raw(self.next_output_id);
        self.next_output_id += 1;
        self.output_ids.insert(key, id);
        id
    }

    /// The id of a global, or `None` if it has none.
    pub(super) fn id_for(&self, key: &K) -> Option<OutputId> {
        self.output_ids.get(key).copied()
    }

    /// Forget a global the compositor has destroyed, reporting the id it held.
    pub(super) fn remove(&mut self, key: &K) -> Option<OutputId> {
        self.output_ids.remove(key)
    }

    /// The compositor's globals, described and ordered by id.
    ///
    /// `describe` turns each global into an [`OutputInfo`] under the id the
    /// registry holds for it, and returns `None` for one the compositor has
    /// not finished describing.
    pub(super) fn connected<D>(
        &mut self,
        live: impl IntoIterator<Item = (K, D)>,
        describe: impl Fn(D, OutputId) -> Option<OutputInfo>,
    ) -> Vec<OutputInfo> {
        let mut list: Vec<OutputInfo> = live
            .into_iter()
            .filter_map(|(key, global)| {
                let id = self.add(key);
                describe(global, id)
            })
            .collect();
        list.sort_by_key(|o| o.id);
        list
    }
}

impl WaylandState {
    /// Find the wl_output for a stable OutputId, if still connected.
    pub(super) fn wl_output_for(&self, id: OutputId) -> Option<wl_output::WlOutput> {
        self.output_state
            .outputs()
            .find(|o| self.outputs.id_for(&o.id()) == Some(id))
    }

    /// Rebuild the reactive output list from current compositor state.
    fn sync_outputs(&mut self) {
        let wl_outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let output_state = &self.output_state;
        let list = self
            .outputs
            .connected(wl_outputs.iter().map(|o| (o.id(), o)), |global, id| {
                let info = output_state.info(global)?;
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
            });
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
        let id = self.outputs.add(output.id());
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
        if let Some(id) = self.outputs.remove(&output.id()) {
            log::info!("Output {:?} disconnected", id);
            outputs::output_removed(id);
        }
        self.sync_outputs();
    }
}

delegate_output!(WaylandState);
