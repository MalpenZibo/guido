//! Output (monitor) enumeration and per-surface output tracking.
//!
//! The compositor's outputs are exposed as a reactive list: read it inside any
//! tracked closure (effects, dynamic children, reactive properties) and the
//! closure re-runs when monitors are added, removed, or reconfigured.
//!
//! One bar per monitor, reacting to hotplug. The effect re-runs on every
//! change to the list, so what it has already built has to be remembered:
//! spawning for each output it finds would hand a monitor a second bar the
//! first time any other one is plugged in.
//!
//! ```no_run
//! # use guido::prelude::*;
//! # use std::cell::RefCell;
//! # use std::collections::HashMap;
//! # use std::rc::Rc;
//! # let bar_widget = || text("bar");
//! let bars: Rc<RefCell<HashMap<OutputId, SurfaceHandle>>> = Rc::default();
//! create_effect(move || {
//!     let current = outputs().get();
//!     let mut bars = bars.borrow_mut();
//!
//!     // A monitor that has gone takes its bar with it.
//!     bars.retain(|id, bar| {
//!         let alive = current.iter().any(|o| o.id == *id);
//!         if !alive {
//!             bar.close();
//!         }
//!         alive
//!     });
//!
//!     // And one that has arrived gets one, once.
//!     for info in current {
//!         bars.entry(info.id).or_insert_with(|| {
//!             spawn_surface(
//!                 SurfaceConfig::new().height(32).output(info.id),
//!                 move || bar_widget(),
//!             )
//!         });
//!     }
//! });
//! ```
//!
//! `examples/multi_output.rs` is this with something in the bar.
//!
//! `surface_output(id)` reports which output a surface is currently shown on
//! (tracked read — reactive when called inside a tracked closure).

use rustc_hash::FxHashMap;

use crate::reactive::global::GlobalSignal;
use crate::reactive::{RwSignal, Signal};
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

/// Behind the test cfgs because both callers are tests — the registry's own
/// and the recorder's. Without them a compositor describes every output it
/// advertises, and there is nothing for this to build.
#[cfg(any(test, feature = "testing"))]
impl OutputInfo {
    /// A monitor the compositor has said nothing about but its connector
    /// name, which is what a test needs and all it needs: the id says which
    /// monitor, the name says which one a failing assertion means.
    pub(crate) fn named(id: OutputId, name: &str) -> Self {
        Self {
            id,
            name: Some(name.to_string()),
            description: None,
            make: String::new(),
            model: String::new(),
            scale_factor: 1,
            logical_size: None,
            logical_position: None,
        }
    }
}

/// Reactive list of connected outputs.
static OUTPUTS: GlobalSignal<Vec<OutputInfo>> = GlobalSignal::new(Vec::new);
/// Which output each surface is currently shown on (latest entered).
static SURFACE_OUTPUTS: GlobalSignal<FxHashMap<SurfaceId, OutputId>> =
    GlobalSignal::new(FxHashMap::default);

fn outputs_signal() -> RwSignal<Vec<OutputInfo>> {
    OUTPUTS.get()
}

fn surface_outputs_signal() -> RwSignal<FxHashMap<SurfaceId, OutputId>> {
    SURFACE_OUTPUTS.get()
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

/// Untracked snapshot of the current outputs (for main-loop code that must
/// not subscribe).
pub(crate) fn current_outputs() -> Vec<OutputInfo> {
    outputs_signal().with_untracked(|v| v.clone())
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
