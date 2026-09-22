//! Stable identity for the compositor's outputs.
//!
//! `wl_output` globals come and go as monitors are plugged, unplugged or
//! remoded, and the protocol object itself carries no identity across that.
//! This registry hands each one a `OutputId` that stays put for as long as the
//! global lives, and never reuses it afterwards: a monitor that comes back is
//! a new output, so a surface pinned to the old one does not silently land on
//! it. A monitor that has left is no output at all: the mapping is what says
//! a global is real, and only a global the compositor has just advertised is
//! given one.

use std::hash::Hash;

use rustc_hash::FxHashMap;
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
    output_ids: FxHashMap<K, OutputId>,
    /// Next OutputId to allocate.
    next_output_id: u32,
}

/// By hand rather than derived: a registry with no monitors in it yet needs
/// nothing of `K`, and a derived bound would ask for `K: Default`.
impl<K> Default for OutputRegistry<K> {
    fn default() -> Self {
        Self {
            output_ids: FxHashMap::default(),
            next_output_id: 0,
        }
    }
}

impl<K: Eq + Hash> OutputRegistry<K> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Mint the id for a global the compositor has just advertised. The only
    /// place an id is allocated.
    pub(crate) fn add(&mut self, key: K) -> OutputId {
        let id = OutputId::from_raw(self.next_output_id);
        self.next_output_id += 1;
        self.output_ids.insert(key, id);
        id
    }

    /// The id of a global, or `None` if it has none.
    pub(crate) fn id_for(&self, key: &K) -> Option<OutputId> {
        self.output_ids.get(key).copied()
    }

    /// Forget a global the compositor has destroyed, reporting the id it held.
    pub(crate) fn remove(&mut self, key: &K) -> Option<OutputId> {
        self.output_ids.remove(key)
    }

    /// The globals that still have a mapping, described and ordered by id.
    ///
    /// `live` is what the compositor reports, which lags the truth: a global
    /// [`Self::remove`] has already let go of is still in it while the destroy
    /// is being processed. Such a global has no id and gets none here.
    ///
    /// `describe` turns each global into an [`OutputInfo`] under the id the
    /// registry holds for it, and returns `None` for one the compositor has
    /// not finished describing.
    pub(crate) fn connected<D>(
        &self,
        live: impl IntoIterator<Item = (K, D)>,
        describe: impl Fn(D, OutputId) -> Option<OutputInfo>,
    ) -> Vec<OutputInfo> {
        let mut list: Vec<OutputInfo> = live
            .into_iter()
            .filter_map(|(key, global)| {
                let id = self.id_for(&key)?;
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
        let list = self.outputs.connected(
            self.output_state.outputs().map(|o| (o.id(), o)),
            |global, id| {
                let info = self.output_state.info(&global)?;
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
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: OutputId, name: &str) -> OutputInfo {
        OutputInfo {
            model: "LG HDR 4K".to_string(),
            ..OutputInfo::named(id, name)
        }
    }

    /// The compositor goes on listing a global for one more round: when
    /// `output_destroyed` drops the mapping, sctk's `outputs()` still holds
    /// the dying `wl_output`. The rebuild has to leave it out rather than mint
    /// it a fresh id, which would put a phantom output in the list under the
    /// unplugged monitor's name.
    #[test]
    fn an_unplugged_monitor_leaves_no_phantom_behind() {
        let mut registry = OutputRegistry::new();
        let laptop = registry.add("eDP-1");
        let external = registry.add("DP-2");
        assert_eq!(registry.remove(&"DP-2"), Some(external));

        let list = registry.connected([("eDP-1", "eDP-1"), ("DP-2", "DP-2")], |name, id| {
            Some(info(id, name))
        });

        assert_eq!(list, vec![info(laptop, "eDP-1")]);
        assert_eq!(
            registry.next_output_id, 2,
            "rebuilding the list minted an id; only a new global may do that"
        );
    }
}
