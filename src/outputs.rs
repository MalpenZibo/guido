//! Output (monitor) enumeration and per-surface output tracking.
//!
//! The compositor's outputs are exposed as a reactive list: read it inside any
//! tracked closure (effects, dynamic children, reactive properties) and the
//! closure re-runs when monitors are added, removed, or reconfigured.
//!
//! ```ignore
//! // One bar per monitor, reacting to hotplug:
//! create_effect(move || {
//!     for info in outputs().get() {
//!         spawn_surface(
//!             SurfaceConfig::new().height(32).output(info.id),
//!             move || bar_widget(),
//!         );
//!     }
//! });
//! ```
//!
//! `surface_output(id)` reports which output a surface is currently shown on
//! (tracked read — reactive when called inside a tracked closure).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::reactive::{RwSignal, Signal, create_signal};
use crate::surface::SurfaceId;

/// Stable identifier for a connected output (monitor).
///
/// Ids are unique for the lifetime of the app and never reused: an output
/// that is unplugged and reconnected gets a fresh id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutputId(u32);

impl OutputId {
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Get the raw id value (for debugging/logging).
    pub fn raw(&self) -> u32 {
        self.0
    }
}

/// Information about a connected output (monitor).
#[derive(Debug, Clone, PartialEq)]
pub struct OutputInfo {
    /// Stable identifier, usable with [`crate::surface::SurfaceConfig::output`].
    pub id: OutputId,
    /// Connector name such as `"DP-1"` or `"eDP-1"`. `None` when the
    /// compositor doesn't support wl_output v4.
    pub name: Option<String>,
    /// Human-readable description such as `"Dell U2720Q"`.
    pub description: Option<String>,
    /// Monitor manufacturer as advertised by the server.
    pub make: String,
    /// Monitor model as advertised by the server.
    pub model: String,
    /// Integer scale factor of the output.
    pub scale_factor: i32,
    /// Logical size in compositor coordinates, if known.
    pub logical_size: Option<(i32, i32)>,
    /// Logical position in the global compositor space, if known.
    pub logical_position: Option<(i32, i32)>,
}

thread_local! {
    /// Reactive list of connected outputs. Lazily created so it works both
    /// before and after platform init; wiped by `reset_outputs()`.
    static OUTPUTS: RefCell<Option<RwSignal<Vec<OutputInfo>>>> = const { RefCell::new(None) };

    /// Which output each surface is currently shown on (latest entered).
    static SURFACE_OUTPUTS: RefCell<Option<RwSignal<HashMap<SurfaceId, OutputId>>>> =
        const { RefCell::new(None) };
}

fn outputs_signal() -> RwSignal<Vec<OutputInfo>> {
    OUTPUTS.with(|cell| {
        *cell
            .borrow_mut()
            .get_or_insert_with(|| create_signal(Vec::new()))
    })
}

fn surface_outputs_signal() -> RwSignal<HashMap<SurfaceId, OutputId>> {
    SURFACE_OUTPUTS.with(|cell| {
        *cell
            .borrow_mut()
            .get_or_insert_with(|| create_signal(HashMap::new()))
    })
}

/// Reactive list of connected outputs (monitors), sorted by [`OutputId`].
///
/// Reading the returned signal inside a tracked closure subscribes it to
/// monitor hotplug and configuration changes.
pub fn outputs() -> Signal<Vec<OutputInfo>> {
    outputs_signal().read_only()
}

/// The output a surface is currently shown on (tracked read).
///
/// Returns `None` until the compositor maps the surface onto an output. For a
/// surface spanning multiple outputs this reports the one entered most
/// recently. Reactive when called inside a tracked closure.
pub fn surface_output(id: SurfaceId) -> Option<OutputId> {
    surface_outputs_signal().with(|m| m.get(&id).copied())
}

/// Replace the reactive output list. Called by the platform layer whenever
/// the compositor adds, removes, or reconfigures an output.
pub(crate) fn sync_outputs(list: Vec<OutputInfo>) {
    outputs_signal().set(list);
}

/// Record that `surface` entered `output`. Called by the platform layer.
pub(crate) fn surface_entered_output(surface: SurfaceId, output: OutputId) {
    surface_outputs_signal().update(|m| {
        m.insert(surface, output);
    });
}

/// Record that `surface` left `output`. Only clears the entry if that output
/// is still the current one (a surface spanning two outputs enters the second
/// before leaving the first).
pub(crate) fn surface_left_output(surface: SurfaceId, output: OutputId) {
    surface_outputs_signal().update(|m| {
        if m.get(&surface) == Some(&output) {
            m.remove(&surface);
        }
    });
}

/// Drop all tracking for a surface (closed) or an output (disconnected
/// without per-surface leave events).
pub(crate) fn surface_closed(surface: SurfaceId) {
    surface_outputs_signal().update(|m| {
        m.remove(&surface);
    });
}

/// Remove entries pointing at a destroyed output.
pub(crate) fn output_removed(output: OutputId) {
    surface_outputs_signal().update(|m| {
        m.retain(|_, o| *o != output);
    });
}

/// Reset output state.
///
/// Called during `App::drop()`; the signals themselves die with the reactive
/// storage reset, this just drops the stale handles.
pub(crate) fn reset_outputs() {
    OUTPUTS.with(|cell| *cell.borrow_mut() = None);
    SURFACE_OUTPUTS.with(|cell| *cell.borrow_mut() = None);
}
