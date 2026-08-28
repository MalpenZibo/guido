pub mod animation;
pub mod backdrop;
mod blur;
pub mod compositor;
mod deferred;
pub mod image_metadata;
mod ingress;
mod jobs;
pub mod keyboard;
pub mod layout;
pub mod outputs;
pub mod pivot;
pub mod reactive;
pub mod render_stats;
pub mod session_lock;
pub mod surface;
mod surface_manager;
#[cfg(feature = "testing")]
pub mod testing;
pub mod transform;
pub mod tree;
pub mod widget_ref;
pub mod widgets;

// These modules are public for advanced use cases
pub mod platform;
pub mod renderer;

// Re-export macros
pub use guido_macros::{SignalFields, component};

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use layout::Constraints;
use platform::create_wayland_app;
use reactive::owner::with_owner;
use reactive::{OwnerId, take_clipboard_change, take_cursor_change};
use renderer::{GpuContext, PaintContext, Renderer, flatten_root_into};
use surface::{SurfaceCommand, SurfaceConfig, SurfaceId, drain_surface_commands};
use surface_manager::{ManagedSurface, SurfaceManager};
use widgets::Widget;
use widgets::font::FontFamily;

// Calloop imports for event-driven main loop (via smithay-client-toolkit re-exports)
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::channel as calloop_channel;
use smithay_client_toolkit::reexports::calloop::ping::make_ping;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;

// Thread-local storage for the default font family
thread_local! {
    static DEFAULT_FONT_FAMILY: RefCell<FontFamily> = const { RefCell::new(FontFamily::SansSerif) };
    static CUSTOM_FONTS: RefCell<Vec<Arc<Vec<u8>>>> = const { RefCell::new(Vec::new()) };
    static CUSTOM_FONT_HASHES: RefCell<rustc_hash::FxHashSet<u64>> =
        RefCell::new(rustc_hash::FxHashSet::default());
    static FONTS_CONSUMED: Cell<bool> = const { Cell::new(false) };
}

/// Set the application-wide default font family.
///
/// This should be called before creating any widgets. Widgets created after this
/// call will use the specified font family as their default.
///
/// # Example
///
/// ```ignore
/// set_default_font_family(FontFamily::Name("Inter".into()));
/// ```
pub fn set_default_font_family(family: FontFamily) {
    DEFAULT_FONT_FAMILY.with(|f| {
        *f.borrow_mut() = family;
    });
}

/// Get the current application-wide default font family.
pub fn default_font_family() -> FontFamily {
    DEFAULT_FONT_FAMILY.with(|f| f.borrow().clone())
}

/// Load custom font data into the application.
///
/// The font bytes will be loaded into all internal FontSystem instances,
/// making the font available for use via `FontFamily::Name(...)`.
///
/// This should be called before creating any widgets or surfaces.
///
/// # Example
///
/// ```ignore
/// const NERD_FONT: &[u8] = include_bytes!("../assets/MyFont.ttf");
/// guido::load_font(NERD_FONT.to_vec());
/// ```
pub fn load_font(data: Vec<u8>) {
    if FONTS_CONSUMED.with(|f| f.get()) {
        log::warn!(
            "load_font() called after FontSystem initialization — \
             this font will not be available. Call load_font() before App::run()."
        );
    }
    // Idempotent: an app that reloads its config re-registers the same fonts
    // on every run, and without this each run would keep another copy.
    let mut hasher = std::hash::BuildHasher::build_hasher(&rustc_hash::FxBuildHasher);
    std::hash::Hasher::write(&mut hasher, &data);
    let hash = std::hash::Hasher::finish(&hasher);
    let is_new = CUSTOM_FONT_HASHES.with(|seen| seen.borrow_mut().insert(hash));
    if !is_new {
        return;
    }
    CUSTOM_FONTS.with(|fonts| {
        fonts.borrow_mut().push(Arc::new(data));
    });
}

/// Get all registered custom font data (for loading into FontSystems).
///
/// Returns cloned `Arc` pointers so every FontSystem (measurer, renderer)
/// receives the same set of fonts.
pub(crate) fn get_registered_fonts() -> Vec<Arc<Vec<u8>>> {
    FONTS_CONSUMED.with(|f| f.set(true));
    CUSTOM_FONTS.with(|fonts| fonts.borrow().clone())
}

/// The reason the application's main loop exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// Normal exit (compositor closed, all surfaces destroyed, etc.)
    Quit,
    /// Restart requested (e.g. config change). The caller should re-create `App` and run again.
    Restart,
    /// The platform layer failed (no Wayland session, compositor without
    /// layer-shell support, connection lost). Previously these ordinary
    /// environmental conditions aborted the process with a panic.
    Error(platform::PlatformError),
}

/// Request a clean application restart.
///
/// The current `App::run()` loop will exit and return `ExitReason::Restart`.
/// Call this from any thread — it uses an atomic + ping to wake the event loop.
pub fn restart_app() {
    jobs::set_exit_request(jobs::ExitRequest::Restart);
}

/// Request a clean application quit.
///
/// The current `App::run()` loop will exit and return `ExitReason::Quit`.
pub fn quit_app() {
    jobs::set_exit_request(jobs::ExitRequest::Quit);
}

/// Everything an application needs, and nothing it does not.
///
/// Writing a *widget* or a *layout* rather than an application is a different
/// job with a different vocabulary — the tree, the paint context, the tracking
/// scope. That lives in [`widget_prelude`], and is not
/// re-exported here.
pub mod prelude {
    pub use crate::animation::{
        Keyframes, SpringConfig, TimingFunction, Transition, TransitionConfig,
    };
    pub use crate::backdrop::{BackdropBlur, BackdropSources};
    pub use crate::compositor::{CompositorEffects, compositor_effects};
    pub use crate::keyboard::keyboard_modifiers;
    pub use crate::layout::{
        Axis, CrossAlignment, Flex, Length, MainAlignment, Size, ZStack, at_least, at_most, fill,
        fraction,
    };
    pub use crate::outputs::{OutputId, OutputInfo, outputs, surface_output};
    pub use crate::pivot::{HorizontalAnchor, Pivot, VerticalAnchor};
    pub use crate::platform::{Anchor, KeyboardInteractivity, Layer};
    pub use crate::reactive::{
        Callback, CursorIcon, IntoSignal, IntoVal, Memo, RwSignal, Service, Signal, Trigger,
        WriteSignal, create_derived, create_effect, create_memo, create_service, create_signal,
        create_stored, create_task, create_trigger, expect_context, has_context, on_cleanup,
        provide_context, provide_signal_context, set_cursor, use_context, with_context,
    };
    pub use crate::renderer::{Shadow, measure_text};
    pub use crate::session_lock::{
        LockState, lock_session, lock_state, session_locked, unlock_session,
    };
    pub use crate::surface::{
        ExclusiveZone, Margin, PopupAnchor, PopupConfig, PopupGravity, PopupHandle, SurfaceConfig,
        SurfaceExtent, SurfaceHandle, SurfaceId, content, spawn_popup, spawn_surface,
        surface_handle,
    };
    pub use crate::transform::{Scale, Translate};
    pub use crate::widget_ref::{WidgetRef, create_widget_ref};
    pub use crate::widgets::{
        AnyWidget, Border, Color, Container, ContentFit, Control, CornerRadii, Corners, Event,
        EventResponse, FontFamily, FontWeight, GradientDirection, Image, ImageSource, InputStyle,
        InputStyled, IntoChildren, IntoClickHandler, Key, LinearGradient, Modifiers, MouseButton,
        Overflow, Padding, Rect, ScrollAxis, ScrollSource, ScrollbarBuilder, ScrollbarVisibility,
        Selection, StateStyle, Stateful, Text, TextInput, TextShadow, TextStroke, TextStyle,
        TextStyled, Widget, container, image, keyed, text, text_input,
    };
    pub use crate::{
        App, ExitReason, SignalFields, component, default_font_family, load_font, quit_app,
        restart_app, set_default_font_family,
    };
}

/// What a widget or a layout written outside the crate needs, on top of
/// [`prelude`].
///
/// The two are meant to be imported together:
///
/// ```ignore
/// use guido::prelude::*;
/// use guido::widget_prelude::*;
/// ```
///
/// [`Widget`] has two required methods — `layout` and
/// `paint`; [`Layout`](crate::layout::Layout) has one. Everything else here is
/// what those signatures name, plus
/// [`with_signal_tracking`](crate::reactive::with_signal_tracking) — the scope
/// that makes a widget's signal reads *its own*, so a change to its content
/// re-runs it rather than the nearest ancestor that happened to open a scope.
pub mod widget_prelude {
    pub use crate::layout::{Constraints, IntoF32, Layout};
    pub use crate::reactive::{JobType, OptionSignalExt, with_signal_tracking};
    pub use crate::renderer::{PaintContext, RenderNode};
    /// The composed matrix. An application says `translate`, `rotate` and
    /// `scale`; a widget written outside the crate positions what it paints,
    /// and for that it needs the thing those three compose into.
    pub use crate::transform::Transform;
    pub use crate::tree::{Tree, WidgetId};
    pub use crate::widgets::{LayoutHints, Widget};
}

use crate::{
    jobs::{get_exit_request, has_pending_jobs, init_wakeup, process_jobs, take_wake_request},
    tree::{DamageRegion, Tree, WidgetId},
};

/// A surface definition that stores configuration and widget factory.
#[allow(clippy::type_complexity)]
struct SurfaceDefinition {
    id: SurfaceId,
    config: SurfaceConfig,
    widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
}

/// Close one surface synchronously: dismissal signal, widget-tree
/// teardown, GPU surface, Wayland objects. The managed surface drops
/// FIRST — its wgpu `Surface<'static>` borrows the wl_surface through
/// erased raw pointers, so the GPU surface must die before the Wayland
/// surface it points at.
fn close_surface_now<P: Platform>(
    id: SurfaceId,
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut P,
    tree: &mut Tree,
) {
    surface::mark_popup_dismissed(id);
    if let Some(managed) = surface_manager.remove(id) {
        // Unregister the widget tree and its signal subscribers before the
        // drop disposes the reactive owner
        surface_manager::teardown_widget_subtree(tree, managed.widget_id);
    }
    wayland_state.destroy_surface(id);
}

/// Re-publish the reservation after something it *follows* has moved — the
/// size, the margin, the anchor.
///
/// Only [`ExclusiveZone::Auto`] follows anything: every other policy is a
/// number, and republishing it would send the compositor a value it already has
/// and log a line saying so, on every resize and every margin change.
/// `measured` is for the one caller that knows a size the compositor has not
/// been told about yet: the content-measure pass, which has just computed it.
fn resync_exclusive_zone<S: Surface>(
    surface: &mut S,
    config: &SurfaceConfig,
    measured: Option<(u32, u32)>,
) {
    if config.exclusive_zone == surface::ExclusiveZone::Auto {
        publish_exclusive_zone(surface, config, measured);
    }
}

/// Resolve the reservation policy against what the surface is anchored to and
/// how big it is, and send the protocol value.
///
/// The size is the one just *asked* for, not the one the compositor last told us
/// about: `set_surface_size` only sends a request, and `WaylandSurfaceState`
/// learns the new size a round trip later, in `configure`. Resolving against
/// that reserves for the size the surface is leaving — a bar going from 32 to 48
/// keeps reserving 32.
///
/// A `Fixed` axis is therefore authoritative here. A `Content` one has no number
/// yet, so it falls back to `measured` if the caller has one and to the live
/// size otherwise, the measure pass re-resolving once it does. And the axis an
/// `Auto` reservation follows is always one we own — `follow_axis` gives up on
/// an axis anchored to both edges, the only kind the compositor sizes — so the
/// value we asked for is the value that will take effect.
fn publish_exclusive_zone<S: Surface>(
    surface: &mut S,
    config: &SurfaceConfig,
    measured: Option<(u32, u32)>,
) {
    // A content axis waiting to be measured has no size to reserve for, and the
    // confirmed one is the wrong answer rather than an old one: a surface
    // declared `width(content()).anchor(TOP | LEFT | RIGHT)` is stretched, so
    // 1920 is what the compositor imposed on an axis that was not ours. Re-anchor
    // it into a side dock and that number becomes a reservation for the whole
    // screen, pushing every other window off it, until the measure lands.
    //
    // So it waits for the measure, which resyncs whenever it runs — but only
    // `Auto` waits, because only `Auto` reads a size. `Fixed`, `None` and
    // `Ignore` are constants, and deferring one defers it for ever: nothing
    // republishes a policy that is not `Auto`, so a `set_exclusive_zone(Fixed(40))`
    // on a `content()` surface would simply never reach the compositor.
    if config.exclusive_zone == surface::ExclusiveZone::Auto
        && measured.is_none()
        && surface::needs_content_measure(config)
    {
        return;
    }

    let known = measured.or_else(|| surface.configured_size());
    let width = surface::requested_extent(config.width, known.map(|(w, _)| w));
    let height = surface::requested_extent(config.height, known.map(|(_, h)| h));
    let zone = config
        .exclusive_zone
        .resolve(config.anchor, config.margin, width, height);
    surface.set_exclusive_zone(zone);
}

/// Process dynamic surface commands (create, close, property changes).
/// Returns false if all surfaces have been closed and the app should exit.
fn process_surface_commands<P: Platform>(
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut P,
    tree: &mut Tree,
) -> bool {
    for cmd in drain_surface_commands() {
        match cmd {
            SurfaceCommand::Create {
                id,
                config,
                widget_fn,
            } => {
                log::info!("Creating dynamic surface {:?}", id);

                // Create Wayland surface
                wayland_state.create_surface(id, &config);

                // Create the widget inside an owner scope so that signals/effects
                // created in the factory are properly owned.
                let (widget, owner_id) = with_owner(widget_fn);
                let managed = ManagedSurface::new(id, config, widget, owner_id, tree);
                surface_manager.add(managed);
            }
            SurfaceCommand::Close(id) => {
                log::info!("Closing dynamic surface {:?}", id);
                // Popup chains tear down children-first: destroying a popup
                // that still has a live child raises not_the_topmost_popup
                // and the compositor kills the connection.
                for child in wayland_state.popup_descendants_bottom_up(id) {
                    close_surface_now(child, surface_manager, wayland_state, tree);
                }
                close_surface_now(id, surface_manager, wayland_state, tree);

                // If no surfaces left, exit
                if surface_manager.is_empty() {
                    wayland_state.request_exit();
                    return false;
                }
            }
            SurfaceCommand::SetLayer { id, layer } => {
                with_surface(wayland_state, id, |s| s.set_layer(layer));
            }
            SurfaceCommand::SetKeyboardInteractivity { id, mode } => {
                with_surface(wayland_state, id, |s| s.set_keyboard_interactivity(mode));
            }
            SurfaceCommand::SetAnchor { id, anchor } => {
                let asked = reconfigure(id, surface_manager, wayland_state, |managed, surface| {
                    managed.config.anchor = anchor;
                    surface.set_anchor(anchor);
                    // The size goes with it: which axes are the compositor's
                    // follows from the anchor, so a bar re-anchored to one edge
                    // has to stop asking for zero on the axis it now owns.
                    send_size(managed, surface)
                });
                if let Some((true, root)) = asked {
                    jobs::request_job(root, jobs::JobRequest::Layout);
                }
            }
            SurfaceCommand::SetSize { id, width, height } => {
                let asked = reconfigure(id, surface_manager, wayland_state, |managed, surface| {
                    managed.config.width = width;
                    managed.config.height = height;
                    // Here every value is one the caller just named, so anything
                    // the anchor discards is worth saying.
                    surface::warn_size_request_on_stretched_axis(id, &managed.config);
                    send_size(managed, surface)
                });
                if let Some((true, root)) = asked {
                    jobs::request_job(root, jobs::JobRequest::Layout);
                }
            }
            SurfaceCommand::SetExclusiveZone { id, zone } => {
                // Recorded through `reconfigure` like the rest; the publishing is
                // left to the resync it already does, which for `Auto` is exactly
                // this. Doing it inside the closure too sent the request twice —
                // and logged it twice — for the one policy the resync is for.
                //
                // The other policies are numbers the resync skips, so they
                // publish here.
                let asked = reconfigure(id, surface_manager, wayland_state, |managed, surface| {
                    managed.config.exclusive_zone = zone;
                    if zone != surface::ExclusiveZone::Auto {
                        publish_exclusive_zone(surface, &managed.config, None);
                    }
                    // Like its three sisters. Declaring `Auto` on a content
                    // surface leaves the resync waiting for a measure — this arm
                    // publishes nothing itself for `Auto`, on purpose — so
                    // without asking for a layout the measure pass never runs on
                    // a settled UI and the reservation is never sent at all.
                    (
                        surface::needs_content_measure(&managed.config),
                        managed.widget_id,
                    )
                });
                if let Some((true, root)) = asked {
                    jobs::request_job(root, jobs::JobRequest::Layout);
                }
            }
            SurfaceCommand::SetMargin { id, margin } => {
                let asked = reconfigure(id, surface_manager, wayland_state, |managed, surface| {
                    managed.config.margin = margin;
                    surface.set_margin(margin);
                    // Like its three sisters. An `Auto` reservation is the margin
                    // *plus* the extent, so moving the margin moves it — and on a
                    // content axis the resync declines to guess and waits for a
                    // measure. Without asking for one, nothing lays anything out,
                    // the measure pass never runs, and the reservation keeps the
                    // old margin for as long as the surface lives.
                    (
                        surface::needs_content_measure(&managed.config),
                        managed.widget_id,
                    )
                });
                if let Some((true, root)) = asked {
                    jobs::request_job(root, jobs::JobRequest::Layout);
                }
            }
            SurfaceCommand::SetInputRegion { id, rects } => {
                with_surface(wayland_state, id, |s| s.set_input_region(rects.as_deref()));
            }
            SurfaceCommand::CreatePopup {
                id,
                parent,
                config,
                widget_fn,
            } => {
                log::info!("Creating popup {:?} anchored to {:?}", id, parent);

                // Build the widget tree FIRST: auto-height popups size to
                // their content, so the content must exist to be measured
                // before the xdg_positioner is created.
                let (widget, owner_id) = with_owner(widget_fn);
                let surface_config = SurfaceConfig::new()
                    .width(config.width)
                    .height(config.height.unwrap_or(1))
                    .background_color(config.background_color);
                let managed = ManagedSurface::new(id, surface_config, widget, owner_id, tree);

                let height = config.height.unwrap_or_else(|| {
                    measure_popup_height(tree, managed.widget_id, config.width, parent)
                });

                // A new grab must nest under the current grab holder:
                // xdg-shell forbids a grabbing popup whose parent chain
                // does not include the popup currently holding the grab
                // (the compositor kills the connection with
                // not_the_topmost_popup). Destroy conflicting grab chains
                // before creating, children first.
                if config.grab {
                    for stale in wayland_state.conflicting_grab_popups(parent) {
                        log::info!("Closing popup {:?} before new grab popup {:?}", stale, id);
                        close_surface_now(stale, surface_manager, wayland_state, tree);
                    }
                }

                if wayland_state.create_popup(id, parent, &config, (config.width, height)) {
                    surface_manager.add(managed);
                } else {
                    // Creation failed (no xdg_wm_base, parent gone): drop the
                    // widget tree and report the popup as dismissed.
                    drop(managed);
                    surface::mark_popup_dismissed(id);
                }
            }
        }
    }
    true
}

/// Measure a popup's natural content height at the given width.
fn measure_popup_height(tree: &mut Tree, root: WidgetId, width: u32, parent: SurfaceId) -> u32 {
    measure_natural_size(tree, root, Some(width), None, parent).1
}

/// Measure a widget tree's natural size. `fixed_w`/`fixed_h` pin an axis;
/// `None` measures it loose, capped at the surface's output size (minus a
/// margin) so a runaway or `fill()` content can't request an absurd
/// surface. Runs under the measure-final flag: animation TARGETS, not
/// in-flight values, so the result is animation-invariant.
fn measure_natural_size(
    tree: &mut Tree,
    root: WidgetId,
    fixed_w: Option<u32>,
    fixed_h: Option<u32>,
    surface: SurfaceId,
) -> (u32, u32) {
    let output_size = outputs::surface_output(surface)
        .and_then(|oid| {
            outputs::current_outputs()
                .into_iter()
                .find(|o| o.id == oid)
                .and_then(|o| o.logical_size)
        })
        .or_else(|| {
            outputs::current_outputs()
                .into_iter()
                .find_map(|o| o.logical_size)
        });
    let cap = |v: i32| (v as f32 - 64.0).max(100.0);
    let (cap_w, cap_h) = match output_size {
        Some((w, h)) => (cap(w), cap(h)),
        None => (800.0, 800.0),
    };

    let (min_w, max_w) = match fixed_w {
        Some(w) => (w as f32, w as f32),
        None => (0.0, cap_w),
    };
    let (min_h, max_h) = match fixed_h {
        Some(h) => (h as f32, h as f32),
        None => (0.0, cap_h),
    };

    let constraints = Constraints::new(min_w, min_h, max_w, max_h);
    let measured = widgets::container::with_measure_final(|| {
        tree.with_widget_mut(root, |widget, id, tree| {
            widget.layout(tree, id, constraints)
        })
    })
    .unwrap_or(layout::Size::new(100.0, 100.0));

    let w = fixed_w.unwrap_or_else(|| (measured.width.ceil() as u32).max(1));
    let h = fixed_h.unwrap_or_else(|| (measured.height.ceil() as u32).max(1));
    (w, h)
}

/// Walk the render tree after painting and cache each node's output.
///
/// The third phase of a frame, after [`renderer::flatten_root_into`] and in
/// that order: flatten writes `cached_flatten` onto the nodes, and this walk
/// is what makes them reusable next frame. Public for the same reason flatten
/// is — a frame can be driven without a compositor, and the paint cache only
/// exists across frames, so a test that never runs this one never reaches it.
/// The loop calls it per child of the surface root, never on the root itself.
///
/// Caching is an `Rc::clone` of the node already sitting in the frame's
/// render tree — O(1) per node instead of the previous deep subtree clone.
///
/// Returns whether the subtree is partial (this node or any descendant had
/// children culled by cull_rect). Partial-ness propagates UP: an ancestor
/// whose subtree embeds an incomplete paint must not be cached either, or a
/// later cache reuse would permanently hide the culled grandchildren. It
/// invalidates as well as refuses — see the body.
///
/// Also clears needs_paint flags and the per-frame `repainted` marker for
/// freshly painted widgets (skipping already-clean subtrees entirely).
pub fn cache_paint_results(tree: &mut Tree, node: &std::rc::Rc<renderer::RenderNode>) -> bool {
    if !node.repainted.get() {
        // Reused from cache: its subtree is by construction complete and its
        // cache entries are already valid — nothing to do below it.
        return false;
    }

    let mut subtree_partial = node.partial;
    for child in &node.children {
        subtree_partial |= cache_paint_results(tree, child);
    }

    let widget_id = WidgetId::from_u64(node.id);
    if tree.contains(widget_id) {
        if subtree_partial {
            // Partial subtree — this paint cannot be cached, and neither can
            // the one before it stay: the widget painted this frame, and it
            // painted something else. Keeping the last complete entry as a
            // stand-in for "how it looks when nothing is culled" is how a
            // scrolled list came back at the top — culling starts the moment
            // it scrolls, so the newest complete entry is the list at rest,
            // and `reuse_cached` serves it whole the next time some other
            // widget asks for a frame.
            tree.clear_cached_paint(widget_id);
        } else {
            // Complete paint — safe to cache for future reuse.
            tree.cache_paint(widget_id, std::rc::Rc::clone(node));
        }
        tree.clear_needs_paint(widget_id);
    }
    // Mark as cached/clean for the flattener and future walks. The cache
    // entry shares this Cell, so reused nodes come out with the flag
    // already cleared.
    node.repainted.set(false);

    subtree_partial
}

/// Deliver this surface's queued input to the widget tree.
///
/// Handlers read signals for their current value, not to subscribe — a click
/// wants the value at click time — so the whole dispatch is a snapshot zone.
///
/// Jobs the handlers push (hover and pressed state changes, clicks) land in
/// the inbox, and distributing them here is what keeps hover from lagging a
/// frame behind the pointer: the drain later in this same frame picks them up.
fn dispatch_events(
    events: &[(std::time::Instant, widgets::Event)],
    root: WidgetId,
    tree: &mut Tree,
    active_roots: &rustc_hash::FxHashSet<WidgetId>,
) {
    for (at, event) in events {
        // Declared per event, not per pass: they are delivered one at a time
        // and each has its own moment.
        tree.set_event_instant(Some(*at));
        reactive::diagnostics::snapshot_zone(|| {
            tree.with_widget_mut(root, |widget, id, tree| {
                widget.event(tree, id, event);
            });
        });
    }
    tree.set_event_instant(None);

    jobs::distribute_jobs(tree, active_roots);
}

/// Push anything the widgets changed back out to the compositor: the
/// clipboard after a copy, the primary selection after a select-to-copy, the
/// cursor shape after a hover.
///
/// Called once per iteration from the loop, not per surface. What it carries
/// belongs to the seat rather than to any one surface, and running it inside
/// the per-surface pass meant it only ran when a surface had a frame to give
/// it: an application with no surface yet — one that spawns its bars from an
/// event, a lock screen before its lock is granted — could copy or set a
/// cursor and have it sit in the queue with no pass coming to drain it. The
/// loop's pre-block check reads that queue and would have called it a lost
/// wakeup, which it is not: the producer woke the loop, and the loop had
/// nowhere to put the work.
fn sync_platform_state<P: Platform>(wayland_state: &mut P) {
    if let Some(text) = take_clipboard_change() {
        wayland_state.set_clipboard(text);
    }
    if let Some(text) = reactive::take_primary_change() {
        wayland_state.set_primary(text);
    }
    if let Some(cursor) = take_cursor_change() {
        wayland_state.set_cursor(cursor);
    }
}

/// Run every job this surface owns, in the one order that works.
///
/// 1. Unregister — remove dead widgets first
/// 2. Animation — advance animated values so layout and paint see current
///    ones, not last frame's. May push Layout/Paint follow-ups.
/// 3. Reconcile — create and remove dynamic children. May push Layout.
/// 4. Layout — needs the advanced animations and the reconciled children
/// 5. Paint — needs the final positions
///
/// Animations run here rather than at end-of-frame because a hover that
/// retargets an animation must reach paint in the same frame; deferring it
/// left every animated value one frame stale.
///
/// The drain is surface-owned: `distribute_jobs` already resolved ownership,
/// so this returns only jobs belonging to this surface's subtree. That gives
/// the pacing gate and the queue the same granularity — a gated surface's
/// animation continuations sit untouched in its own queue until its frame
/// callback fires, whatever the other surfaces do meanwhile.
fn run_jobs(
    root: WidgetId,
    tree: &mut Tree,
    layout_roots: &mut Vec<WidgetId>,
    active_roots: &rustc_hash::FxHashSet<WidgetId>,
) {
    let jobs = jobs::drain_surface_jobs(root);
    process_jobs(&jobs, tree, layout_roots);
    jobs::recycle_job_buffer(jobs);

    // Follow-ups from the animation advances and the reconciliation land in
    // the inbox, so they need distributing before they can be drained.
    jobs::distribute_jobs(tree, active_roots);
    let followup = jobs::drain_surface_non_animation_jobs(root);
    if !followup.is_empty() {
        process_jobs(&followup, tree, layout_roots);
    }
    jobs::recycle_job_buffer(followup);
}

/// What this surface tells us about itself, read once at the top of a frame.
///
/// Copying it frees the borrow on `wayland_state`, which the phases below
/// need mutably — and nothing the compositor sends can change it before this
/// frame ends anyway.
struct Frame {
    /// What arrived for this surface, each with when it happened.
    events: Vec<(std::time::Instant, widgets::Event)>,
    scale_factor: f32,
    width: u32,
    height: u32,
    /// A `wl_surface.frame` callback is still in flight: the compositor has
    /// not shown the previous frame yet.
    frame_callback_pending: bool,
    /// No first frame at a known scale yet. Such a surface bypasses the
    /// pacing gate and repaints in full — it has nothing on screen to pace
    /// against.
    force_render_surface: bool,
}

/// Whether this frame would only queue behind one the compositor has not shown.
///
/// The three that open the gate anyway — the fields say why each is what it is
/// — are a surface with nothing on screen yet, a resize, and a scale change:
/// all three have something to tell the compositor that cannot wait for a
/// callback.
///
/// A function rather than the condition inline, because it is the one decision
/// in the frame path that can be taken from a `Frame` and a `Geometry` alone,
/// and so the one a test could reach first.
fn paced_out(frame: &Frame, geometry: &Geometry) -> bool {
    frame.frame_callback_pending
        && !frame.force_render_surface
        && !geometry.needs_resize
        && !geometry.scale_changed
}

/// What the frame path asks of whatever is showing one surface.
///
/// Every request a frame makes of a compositor, and nothing that is not about
/// this surface: no id parameter, because a handle already knows which one it
/// is. `supports_blur_region` is the one that is not here — a capability
/// belongs to the connection, not to a window — and it lives on [`Platform`].
///
/// One required method. `open_frame` has no default because a surface that
/// cannot say what a frame is has nothing to show; every other request defaults
/// to doing nothing and knowing nothing, which is the honest answer for a surface
/// that does not have that protocol. So a second implementation writes down
/// what it wants to watch and stays silent about the rest.
///
/// What that cannot catch is a method added here and forgotten in the Wayland
/// implementation: it would silently do nothing against a real compositor
/// rather than fail to build. The defaults are for a surface that *cannot* do
/// the thing, never for one that has not been taught to.
pub(crate) trait Surface {
    /// The facts a frame is built from, or `None` if this surface has no size
    /// to draw at yet. Takes the queued input with it, so calling it twice for
    /// one frame loses events.
    fn open_frame(&mut self) -> Option<Frame>;

    /// The size the compositor has confirmed, if it has confirmed one.
    ///
    /// Not the size that was *requested*: `WaylandSurfaceState` is created
    /// holding that, which for a content axis is the 1px placeholder — so
    /// reading it unconditionally would report a number nobody agreed on as
    /// though it were live, and make `requested_extent`'s "no size yet" branch
    /// unreachable while the case it describes was still happening.
    fn configured_size(&self) -> Option<(u32, u32)> {
        None
    }

    /// The scale the compositor confirmed for this surface, if it has.
    fn scale_factor(&self) -> Option<f32> {
        None
    }

    /// The width an auto-width popup should be measured against.
    fn popup_auto_width(&self) -> Option<u32> {
        None
    }

    /// Tell the compositor an auto-height popup's content changed height.
    fn reposition_popup_if_changed(&mut self, new_height: u32) {
        let _ = new_height;
    }

    /// Ask for a size. A request: the answer arrives as a configure.
    fn set_size(&mut self, width: u32, height: u32) {
        let _ = (width, height);
    }

    /// Publish the screen space this surface reserves.
    fn set_exclusive_zone(&mut self, zone: i32) {
        let _ = zone;
    }

    /// Run `f` and let its requests reach the compositor as one commit, so a
    /// group of them never shows an intermediate surface on the way.
    fn batch_layer_requests<F: FnOnce(&mut Self)>(&mut self, f: F) {
        f(self);
    }

    /// Whether this surface has a blur region published that would have to be
    /// withdrawn.
    fn has_published_blur(&self) -> bool {
        false
    }

    /// Whether the last published region was dropped and owes republishing.
    fn take_blur_resync(&mut self) -> bool {
        false
    }

    /// Publish the region to blur behind this surface.
    fn sync_blur_region(&mut self, rects: Vec<blur::BlurRect>, commit: bool) {
        let _ = (rects, commit);
    }

    /// Ask to be told when this surface's last frame has been shown.
    fn request_frame_callback(&mut self) {}

    /// Report damaged buffer pixels, in buffer coordinates. Why it has to
    /// happen before presenting is on the implementation.
    fn damage(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let _ = (x, y, width, height);
    }

    /// Record that the callback asked for is now committed.
    fn mark_frame_callback_pending(&mut self) {}

    /// Move this surface between the compositor's layers.
    fn set_layer(&mut self, layer: platform::Layer) {
        let _ = layer;
    }

    /// Re-anchor it, which also decides which axes the compositor owns.
    fn set_anchor(&mut self, anchor: platform::Anchor) {
        let _ = anchor;
    }

    /// Hold it off the edges it is anchored to.
    fn set_margin(&mut self, margin: surface::Margin) {
        let _ = margin;
    }

    /// Restrict where input reaches it. `None` is the whole surface.
    fn set_input_region(&mut self, rects: Option<&[widgets::Rect]>) {
        let _ = rects;
    }

    /// How much of the keyboard this surface wants.
    fn set_keyboard_interactivity(&mut self, mode: platform::KeyboardInteractivity) {
        let _ = mode;
    }
}

/// Whatever the application is running on.
///
/// The thin half: it hands out surfaces and answers for the things that belong
/// to the connection rather than to any one window. `WaylandState` is one
/// implementation; a recorder that keeps what it was asked is the other.
pub(crate) trait Platform {
    /// One surface, borrowed for as long as the caller needs it.
    ///
    /// A handle rather than an id on every method, because a Wayland surface is
    /// its own state *and* the connection's — `sync_blur_region` needs the
    /// effect manager, `set_size` needs the layer surface — so what a caller
    /// holds has to be able to reach both.
    type Surface<'a>: Surface
    where
        Self: 'a;

    fn surface(&mut self, id: SurfaceId) -> Option<Self::Surface<'_>>;

    /// Whether backdrop blur regions can be published at all. A capability of
    /// the connection, which is why it is here and not on a surface.
    fn supports_blur_region(&self) -> bool {
        false
    }

    /// Bring a surface into being. It has no size until a configure arrives.
    fn create_surface(&mut self, id: SurfaceId, config: &SurfaceConfig) {
        let _ = (id, config);
    }

    /// Bring a popup into being, anchored to another surface. `false` where
    /// popups are unavailable or the parent cannot host one.
    fn create_popup(
        &mut self,
        id: SurfaceId,
        parent: SurfaceId,
        config: &surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        let _ = (id, parent, config, size);
        false
    }

    /// Take a surface away.
    fn destroy_surface(&mut self, id: SurfaceId) {
        let _ = id;
    }

    /// Popups holding a grab that a new one under `parent` would conflict with,
    /// which have to be dismissed before it opens.
    fn conflicting_grab_popups(&self, parent: SurfaceId) -> Vec<SurfaceId> {
        let _ = parent;
        Vec::new()
    }

    /// A popup's descendants, deepest first — the order they must close in.
    fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        let _ = root;
        Vec::new()
    }

    /// Ask to lock the session. `false` where the protocol is unavailable.
    fn start_session_lock(&mut self) -> bool {
        false
    }

    /// Give one output its lock surface. `false` without an active lock.
    fn create_lock_surface(&mut self, id: SurfaceId, output: outputs::OutputId) -> bool {
        let _ = (id, output);
        false
    }

    /// What the lock has said since this was last asked.
    fn take_lock_events(&mut self) -> Vec<platform::LockEvent> {
        Vec::new()
    }

    /// Release the session.
    fn unlock_session(&mut self) {}

    /// Whether the platform has asked the application to stop — the compositor
    /// went away, or the connection died.
    fn should_exit(&self) -> bool {
        false
    }

    /// Ask it to stop: the last surface closed, so there is nothing to show.
    fn request_exit(&mut self) {}

    /// Hand the seat what the widgets changed this iteration.
    fn set_clipboard(&mut self, text: String) {
        let _ = text;
    }

    fn set_primary(&mut self, text: String) {
        let _ = text;
    }

    fn set_cursor(&self, cursor: reactive::CursorIcon) {
        let _ = cursor;
    }

    /// Send everything this iteration queued. `false` means the connection is
    /// gone and the application is over.
    fn flush(&self) -> bool {
        true
    }

    /// Where this surface's frames should land, once it has a size.
    ///
    /// The one place the two implementations genuinely differ rather than one
    /// recording what the other sends: a compositor hands out a swapchain tied
    /// to a real window, and a driver with no compositor allocates a texture.
    /// `None` where the surface is not ready to be given one.
    fn create_render_target(
        &self,
        id: SurfaceId,
        gpu: &renderer::GpuContext,
        size: (u32, u32),
    ) -> Option<renderer::RenderTarget> {
        let _ = (id, gpu, size);
        None
    }
}

/// One Wayland surface, and the connection it needs to speak.
///
/// A pair rather than a borrow of `WaylandSurfaceState`, because half of what a
/// surface does reaches past itself: the blur region wants the effect manager,
/// a layer request wants the layer surface *and* the queue. Both live on the
/// state, so the handle carries the state and the name of one surface in it.
pub(crate) struct WaylandSurface<'a> {
    state: &'a mut platform::WaylandState,
    id: SurfaceId,
}

impl Surface for WaylandSurface<'_> {
    fn open_frame(&mut self) -> Option<Frame> {
        let surface = self.state.get_surface_mut(self.id)?;
        if !surface.configured {
            return None;
        }
        // Taken before the caller's GPU check, not after: a surface still
        // building its GPU state drops this frame *and* the input queued for
        // it. Holding the input instead would deliver a burst of stale events —
        // a pointer position from several frames ago among them — on the first
        // frame that renders.
        let events = surface.take_events();
        let fully_initialized = surface.first_frame_presented && surface.scale_factor_received;
        Some(Frame {
            events,
            scale_factor: surface.scale_factor,
            width: surface.width,
            height: surface.height,
            frame_callback_pending: surface.frame_callback_pending,
            force_render_surface: !fully_initialized,
        })
    }

    fn configured_size(&self) -> Option<(u32, u32)> {
        self.state
            .get_surface(self.id)
            .filter(|s| s.configured)
            .map(|s| (s.width, s.height))
    }

    fn scale_factor(&self) -> Option<f32> {
        Some(self.state.get_surface(self.id)?.scale_factor)
    }

    fn popup_auto_width(&self) -> Option<u32> {
        self.state.popup_auto_width(self.id)
    }

    fn reposition_popup_if_changed(&mut self, new_height: u32) {
        self.state.reposition_popup_if_changed(self.id, new_height)
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.state.set_surface_size(self.id, width, height)
    }

    fn set_exclusive_zone(&mut self, zone: i32) {
        self.state.set_surface_exclusive_zone(self.id, zone)
    }

    fn batch_layer_requests<F: FnOnce(&mut Self)>(&mut self, f: F) {
        let id = self.id;
        // The batching is the state's — one commit for the group — and what the
        // closure is handed back is this surface, so nothing inside it can
        // reach another one.
        let batch = self.state.open_layer_batch(id);
        f(self);
        self.state.close_layer_batch(batch);
    }

    fn has_published_blur(&self) -> bool {
        self.state.has_published_blur(self.id)
    }

    fn take_blur_resync(&mut self) -> bool {
        self.state.take_blur_resync(self.id)
    }

    fn sync_blur_region(&mut self, rects: Vec<blur::BlurRect>, commit: bool) {
        self.state.sync_blur_region(self.id, rects, commit)
    }

    fn request_frame_callback(&mut self) {
        self.state.request_frame_callback(self.id)
    }

    fn damage(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.state.damage_surface(self.id, x, y, width, height)
    }

    fn mark_frame_callback_pending(&mut self) {
        self.state.mark_frame_callback_pending(self.id)
    }

    fn set_layer(&mut self, layer: platform::Layer) {
        self.state.set_surface_layer(self.id, layer)
    }

    fn set_anchor(&mut self, anchor: platform::Anchor) {
        self.state.set_surface_anchor(self.id, anchor)
    }

    fn set_margin(&mut self, margin: surface::Margin) {
        self.state.set_surface_margin(self.id, margin)
    }

    fn set_input_region(&mut self, rects: Option<&[widgets::Rect]>) {
        self.state.set_surface_input_region(self.id, rects)
    }

    fn set_keyboard_interactivity(&mut self, mode: platform::KeyboardInteractivity) {
        self.state.set_surface_keyboard_interactivity(self.id, mode)
    }
}

impl Platform for platform::WaylandState {
    type Surface<'a> = WaylandSurface<'a>;

    fn surface(&mut self, id: SurfaceId) -> Option<WaylandSurface<'_>> {
        self.get_surface(id)?;
        Some(WaylandSurface { state: self, id })
    }

    fn supports_blur_region(&self) -> bool {
        self.supports_blur_region()
    }

    fn create_surface(&mut self, id: SurfaceId, config: &SurfaceConfig) {
        self.create_surface_with_id(id, config)
    }

    fn create_popup(
        &mut self,
        id: SurfaceId,
        parent: SurfaceId,
        config: &surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        self.create_popup_surface_with_id(id, parent, config, size)
    }

    fn destroy_surface(&mut self, id: SurfaceId) {
        self.destroy_surface(id)
    }

    fn conflicting_grab_popups(&self, parent: SurfaceId) -> Vec<SurfaceId> {
        self.conflicting_grab_popups(parent)
    }

    fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        self.popup_descendants_bottom_up(root)
    }

    fn start_session_lock(&mut self) -> bool {
        self.start_session_lock()
    }

    fn create_lock_surface(&mut self, id: SurfaceId, output: outputs::OutputId) -> bool {
        self.create_lock_surface_with_id(id, output)
    }

    fn take_lock_events(&mut self) -> Vec<platform::LockEvent> {
        self.take_lock_events()
    }

    fn unlock_session(&mut self) {
        self.unlock_session()
    }

    fn should_exit(&self) -> bool {
        self.exit
    }

    fn request_exit(&mut self) {
        self.exit = true;
    }

    fn set_clipboard(&mut self, text: String) {
        self.set_clipboard(text)
    }

    fn set_primary(&mut self, text: String) {
        self.set_primary(text)
    }

    fn set_cursor(&self, cursor: reactive::CursorIcon) {
        self.set_cursor(cursor)
    }

    fn flush(&self) -> bool {
        if let Err(e) = self.connection.flush() {
            log::error!("Wayland connection flush failed: {e}");
            return false;
        }
        true
    }

    fn create_render_target(
        &self,
        id: SurfaceId,
        gpu: &renderer::GpuContext,
        size: (u32, u32),
    ) -> Option<renderer::RenderTarget> {
        let wl_surface = &self.get_surface(id)?.wl_surface;
        let window = platform::WaylandWindowWrapper::new(&self.connection, wl_surface);
        Some(renderer::RenderTarget::Swapchain(
            gpu.create_surface(window, size.0, size.1),
        ))
    }
}

/// The physical size, and what about it changed since the last frame.
struct Geometry {
    physical_width: u32,
    physical_height: u32,
    needs_resize: bool,
    scale_changed: bool,
}

/// Read the surface's state and take its queued input, or give up on this
/// frame: an unconfigured surface has no size to render at, and one whose GPU
/// state has not been built yet gets it on the next iteration.
fn open_frame<P: Platform>(ctx: &mut FrameContext<P>) -> Option<Frame> {
    // The host answers for the compositor; the GPU readiness is ours, and a
    // surface still building its swapchain has nowhere to draw.
    let frame = ctx.wayland_state.surface(ctx.id)?.open_frame()?;
    ctx.surface.is_gpu_ready().then_some(frame)
}

/// Resolve the physical size and bring the swapchain in line with it.
///
/// Runs after input rather than with the rest of the snapshot: dispatching
/// events cannot change the surface size, but resizing the swapchain before
/// the widgets have been told anything would reorder the frame for no gain.
fn resolve_geometry<P: Platform>(ctx: &mut FrameContext<P>, frame: &Frame) -> Geometry {
    let id = ctx.id;
    let surface = &mut *ctx.surface;
    let scale = frame.scale_factor as u32;
    let physical_width = frame.width * scale;
    let physical_height = frame.height * scale;

    let wgpu_surface = surface.wgpu_surface.as_mut().unwrap();
    let needs_resize =
        wgpu_surface.width() != physical_width || wgpu_surface.height() != physical_height;
    let scale_changed = frame.scale_factor != surface.previous_scale_factor;

    if needs_resize {
        log::info!(
            "Resizing surface {:?} to {}x{} (physical), scale {}",
            id,
            physical_width,
            physical_height,
            scale
        );
        wgpu_surface.resize(physical_width, physical_height);
    }

    if scale_changed {
        log::info!(
            "Surface {:?} scale factor changed: {} -> {}",
            id,
            surface.previous_scale_factor,
            frame.scale_factor
        );
        surface.previous_scale_factor = frame.scale_factor;
    }

    Geometry {
        physical_width,
        physical_height,
        needs_resize,
        scale_changed,
    }
}

/// Bring the tree's geometry up to date, and let a content-sized surface
/// tell the compositor how big it wants to be.
///
/// Partial layout is the normal path: only the subtrees that marked
/// themselves re-run, from their relayout boundaries. A full layout from the
/// root happens only on a resize, because that is the one change no widget
/// can have noticed on its own.
fn layout_pass<P: Platform>(
    ctx: &mut FrameContext<P>,
    frame: &Frame,
    geometry: &Geometry,
    has_pending_layouts: bool,
    layout_roots: &mut Vec<WidgetId>,
) {
    let id = ctx.id;
    let surface = &mut *ctx.surface;
    let wayland_state = &mut *ctx.wayland_state;
    let renderer = &mut *ctx.renderer;
    let tree = &mut *ctx.tree;

    // Update renderer for this surface
    renderer.set_screen_size(
        geometry.physical_width as f32,
        geometry.physical_height as f32,
    );
    renderer.set_scale_factor(frame.scale_factor);

    // Re-layout using partial layout from boundaries when available
    let constraints = Constraints::new(0.0, 0.0, frame.width as f32, frame.height as f32);
    if !layout_roots.is_empty() {
        // Partial layout: only update dirty subtrees starting from boundaries
        let mut roots = Vec::new();
        std::mem::swap(&mut roots, layout_roots);
        for root_id in &roots {
            // Use cached constraints for boundaries, or fall back to parent constraints
            let cached = tree.cached_constraints(*root_id).unwrap_or(constraints);

            tree.with_widget_mut(*root_id, |widget, id, tree| {
                widget.layout(tree, id, cached);
            });
        }
        // Paint invalidation is per widget, not per subtree: every widget
        // that actually ran its layout marked itself (and its ancestors)
        // inside Tree::cache_layout, and Tree::set_origin damaged what
        // moved. Marking the whole subtree here instead would repaint
        // every descendant of the layout root — which in a
        // content-sized tree is the whole surface — and throw away the
        // paint and flatten caches the layout pass just earned.
    } else if geometry.needs_resize {
        // Full layout from root only when explicitly needed (first frame, resize, etc.)
        tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
            widget.layout(tree, id, constraints);
        });
        tree.mark_subtree_needs_paint(surface.widget_id);
    }
    // If neither condition is true, skip layout entirely - nothing is dirty

    // Auto-height popups follow their content: when this surface had
    // layout activity, re-measure the natural height and ask the
    // compositor to reposition if it changed (submenus expanding, lists
    // growing). The measure pass runs under different constraints, so
    // the real layout is restored right after.
    if has_pending_layouts
        && let Some(popup_width) = wayland_state.surface(id).and_then(|s| s.popup_auto_width())
    {
        let natural = measure_popup_height(tree, surface.widget_id, popup_width, id);
        tree.with_widget_mut(surface.widget_id, |widget, wid, tree| {
            widget.layout(tree, wid, constraints);
        });
        with_surface(wayland_state, id, |s| {
            s.reposition_popup_if_changed(natural)
        });
    } else if (has_pending_layouts || frame.force_render_surface)
        && surface::needs_content_measure(&surface.config)
    {
        // Initialization included: a freshly spawned surface reaches
        // its first frames through the full-layout path, not
        // layout_roots — without measuring here a single-toast spawn
        // would stay at its 1px initial size until the NEXT content
        // change.
        // Content-sized layer surface (toast stacks, OSDs): measure
        // the natural size and follow it — the layer counterpart of
        // the popup reposition above. Without this a content-sized
        // surface is stuck at its initial size: the real layout
        // clamps to the surface, so a measured rect can never exceed
        // it. The measure reads animation TARGETS, so an animated
        // growth resizes once and the animation plays inside.
        let fixed_w = (!surface.config.width.is_content()).then_some(frame.width);
        let fixed_h = (!surface.config.height.is_content()).then_some(frame.height);
        let (nw, nh) = measure_natural_size(tree, surface.widget_id, fixed_w, fixed_h, id);
        tree.with_widget_mut(surface.widget_id, |widget, wid, tree| {
            widget.layout(tree, wid, constraints);
        });
        // Compared as they will be *asked for*, not as they were measured. On an
        // axis the compositor owns both sides are zero, so a `content()` width
        // under `LEFT | RIGHT` cannot make this true for ever by measuring 300
        // against a screen's 1920 — which it did, resizing, resyncing,
        // committing and logging once a frame for the whole life of the surface.
        let asking = surface::honour_owned_axes(surface.config.anchor, nw, nh);
        let resizing =
            asking != surface::honour_owned_axes(surface.config.anchor, frame.width, frame.height);
        // The resize is conditional; the resync is not. This is the only place
        // the measurement is known, and the reservation may be waiting on it even
        // when the size did not move — `publish_exclusive_zone` declines to
        // resolve `Auto` against an unmeasured content axis, so a re-anchoring
        // that measures back to the number it already had would otherwise leave
        // the reservation deferred for ever.
        {
            // One commit, like every other pair of these. A content surface whose
            // content animates resizes on frame after frame, and two commits a
            // frame show the compositor the new size against the old reservation
            // in between.
            let config = &surface.config;
            if let Some(mut handle) = wayland_state.surface(id) {
                handle.batch_layer_requests(|surface| {
                    if resizing {
                        // Through the same rule as every other resize: a measured
                        // number on an axis the compositor owns hands back an axis
                        // that is not ours, and a full-width bar would then stay at
                        // whatever the screen was when it was measured.
                        let (ask_w, ask_h) = asking;
                        surface.set_size(ask_w, ask_h);
                    }
                    // An `Auto` reservation follows an automatic resize too, and this
                    // is the one caller that knows the measured size before the
                    // compositor does — so it passes it rather than letting the
                    // helper look it up.
                    resync_exclusive_zone(surface, config, Some((nw, nh)));
                });
            }
        }
    }

    // Update widget ref signals with current bounds after layout
    widget_ref::update_widget_refs(tree);

    // A `WidgetRef::focus()` from application code lands here: after layout, so
    // a handle attached this very frame has resolved to a widget, and before
    // `paint_and_present`, so the frame that resolves it is the frame that
    // draws the caret. Applying it after the render pass instead reads
    // tidier — one call per iteration rather than one per surface — and costs
    // an autofocused field a frame of looking unfocused.
    //
    // Staying parked is this one's resting state, unlike the queues in
    // `deferred`: a request for a widget that does not exist yet is the
    // ordinary shape of `focus()` from a startup effect, and it waits as many
    // frames as it takes. Which is also why it does not matter that a surface
    // must exist for this to run — without one there is no tree to resolve
    // against.
    reactive::focus::apply_pending_focus(tree);
}

/// Send the size the surface is currently asking for, and report whether the
/// content-measure pass has to run before that answer is right.
///
/// Shared by the two commands that can change it — a resize, and a re-anchoring
/// that changes which axes are ours — so the anchor and the size cannot end up
/// describing different surfaces.
///
/// An axis crossing from the compositor's side to ours has to be given a number:
/// layer-shell makes zero on an axis not anchored to opposite edges a protocol
/// error, so a re-anchored bar that says nothing is disconnected at its next
/// commit. The number is the config's, **default included** — a bar written
/// `height(32).anchor(TOP | LEFT | RIGHT)` never names a width, so re-anchoring
/// it to `TOP | LEFT` asks for [`SurfaceConfig::default`]'s 400 rather than the
/// screen width it was stretched to. Handing back the size it happens to have
/// instead would read better there and worse where it counts: it would also
/// override a width the app *did* name and had ignored while stretched. An app
/// re-anchoring to an axis it cares about sets the size for it.
///
/// It does not warn. A re-anchoring names no sizes, so warning from here fired
/// on every anchor change for an axis that was stretched before and still is —
/// the noise
/// [`warn_content_on_stretched_axis`](surface::warn_content_on_stretched_axis)
/// was split off to avoid. The caller that was handed a size says so.
fn send_size<S: Surface>(managed: &ManagedSurface, surface: &mut S) -> (bool, WidgetId) {
    let live = surface.configured_size();
    let (ask_w, ask_h, needs_measure) = surface::resize_request(&managed.config, live);
    surface.set_size(ask_w, ask_h);
    (needs_measure, managed.widget_id)
}

/// Apply a change to a surface, then re-publish what follows from it.
///
/// Every property an [`ExclusiveZone::Auto`] reservation follows — the anchor,
/// the size, the margin — goes through here, so the resync happens in one place
/// and a fifth trigger cannot be added to three of the four. Forgetting exactly
/// that is how a re-anchored dock went on reserving a bar's height at the top of
/// the screen.
///
/// The change records on the config *and* sends the protocol request, because
/// the two must not drift: the content-sizing pass reads the config every frame.
fn reconfigure<P: Platform, R>(
    id: SurfaceId,
    surface_manager: &mut SurfaceManager,
    platform: &mut P,
    change: impl FnOnce(&mut ManagedSurface, &mut P::Surface<'_>) -> R,
) -> Option<R> {
    let managed = surface_manager.get_mut(id)?;
    let mut handle = platform.surface(id)?;
    let mut result = None;
    // One commit for the group. A re-anchoring sends the anchor, the size and
    // the reservation, and each request committing on its own would show the
    // compositor two intermediate surfaces on the way.
    handle.batch_layer_requests(|surface| {
        result = Some(change(managed, surface));
        resync_exclusive_zone(surface, &managed.config, None);
    });
    result
}

/// Paint, flatten, hand the frame to the GPU, and re-arm the pacing gate.
///
/// The order at the end is not free-form: the frame callback and the damage
/// are both requested BEFORE presenting, because presenting is what commits
/// the surface. Anything set afterwards would ride an empty second commit
/// the compositor cannot use.
fn paint_and_present<P: Platform>(ctx: &mut FrameContext<P>, frame: &Frame, geometry: &Geometry) {
    let id = ctx.id;
    let surface = &mut *ctx.surface;
    let wayland_state = &mut *ctx.wayland_state;
    let renderer = &mut *ctx.renderer;
    let tree = &mut *ctx.tree;

    // Force full repaint on resize, scale change, or during initialization
    if frame.force_render_surface || geometry.needs_resize || geometry.scale_changed {
        tree.mark_subtree_needs_paint(surface.widget_id);
    }

    // Skip frame if nothing needs paint
    if !tree.needs_paint(surface.widget_id) {
        // Only when one is owed. The retained command buffer still holds the
        // last frame, which is what is on screen — so it is also the right
        // region, and rebuilding it from those commands is right but not free:
        // an idle surface would walk a whole frame's worth of them, every frame,
        // to arrive at the region it published already. The one thing that can
        // change while nothing paints is the compositor's blur capability, which
        // drops its regions, and that says so.
        if let Some(mut handle) = wayland_state.surface(id)
            && handle.take_blur_resync()
        {
            let blur_rects = blur::regions_from_commands(&surface.flattened_commands);
            handle.sync_blur_region(blur_rects, true);
        }
        render_stats::record_frame_skipped();
        render_stats::end_frame(&DamageRegion::None);
        return;
    }

    // Clear and reuse the root node (preserves capacity)
    surface.root_node.clear();
    surface.root_node.bounds =
        widgets::Rect::new(0.0, 0.0, frame.width as f32, frame.height as f32);

    time_phase!(render_stats::Phase::Paint, {
        tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
            let mut ctx = PaintContext::new(&mut surface.root_node);
            widget.paint(tree, id, &mut ctx);
        });
    });

    // Flatten tree into reused buffers
    let compositor_blur;
    time_phase!(render_stats::Phase::Flatten, {
        compositor_blur = flatten_root_into(
            &surface.root_node,
            &mut surface.flattened_commands,
            &mut surface.command_layers,
        );
    });

    // The compositor blur region is read off the frame that was just built, so
    // it cannot disagree with what is on screen — see `blur`. Set before
    // present() so it rides the buffer commit: region and content change
    // together.
    // Asked before the scan, not after: without the protocol there is nothing
    // to publish, and walking the frame's commands to find out would be a
    // per-frame cost on the compositors that can do the least with it.
    //
    // And on the compositors that *can*, only for a frame that asks. The flatten
    // counted them on its way past, so the scan is skipped for the surfaces —
    // almost all of them — with no compositor blur in the frame at all. The one
    // frame that has to be walked without carrying one is the frame that stops:
    // it owes the compositor the empty region that withdraws the last one.
    if wayland_state.supports_blur_region()
        && let Some(mut handle) = wayland_state.surface(id)
        && (compositor_blur || handle.has_published_blur())
    {
        let blur_rects = blur::regions_from_commands(&surface.flattened_commands);
        handle.sync_blur_region(blur_rects, false);
    }

    // Here, above `present`, because both of these have to ride its commit —
    // why is on the two methods. Previously the callback was requested exactly
    // once at startup and never again, and the only pacing was the event loop
    // blocking inside the Fifo swapchain.
    with_surface(wayland_state, id, |s| s.request_frame_callback());

    let damage = tree.take_damage(surface.widget_id);
    match damage {
        DamageRegion::None => {
            // Shouldn't happen since we're rendering, but report full damage to be safe
            with_surface(wayland_state, id, |s| {
                s.damage(
                    0,
                    0,
                    geometry.physical_width as i32,
                    geometry.physical_height as i32,
                )
            });
        }
        DamageRegion::Partial(rect) => {
            let scale = frame.scale_factor;
            with_surface(wayland_state, id, |s| {
                s.damage(
                    (rect.x * scale) as i32,
                    (rect.y * scale) as i32,
                    (rect.width * scale).ceil() as i32,
                    (rect.height * scale).ceil() as i32,
                )
            });
        }
        DamageRegion::Full => {
            with_surface(wayland_state, id, |s| {
                s.damage(
                    0,
                    0,
                    geometry.physical_width as i32,
                    geometry.physical_height as i32,
                )
            });
        }
    }

    let presented;
    time_phase!(render_stats::Phase::GpuRender, {
        presented = renderer.render(
            surface.wgpu_surface.as_mut().unwrap(),
            &surface.flattened_commands,
            &surface.command_layers,
            surface.config.background_color,
        );
    });

    if !presented {
        // Nothing reached the screen (lost/outdated swapchain — common
        // right after a resize). Keep all dirty flags so the content is
        // repainted next frame instead of staying stale, restore full
        // damage, and request that frame.
        tree.set_full_damage(surface.widget_id);
        jobs::wake_loop();
        return;
    }

    // Cache paint results AFTER flatten so cached_flatten data is preserved.
    // This enables incremental flatten for paint-cached nodes on subsequent
    // frames. The surface root itself is never cache-reused (it always
    // repaints), so only its children are cached.
    time_phase!(render_stats::Phase::CachePaintResults, {
        for child in &surface.root_node.children {
            cache_paint_results(tree, child);
        }
        tree.clear_needs_paint(surface.widget_id);
    });

    // Track render stats (when compiled with --features render-stats)
    render_stats::record_frame_painted();
    render_stats::end_frame(&damage);

    with_surface(wayland_state, id, |s| s.mark_frame_callback_pending());
}

/// The borrows a frame needs from start to finish, in one place.
///
/// They are distinct objects in the caller, so bundling them costs nothing —
/// and it spares every phase below the parameter list that had already grown
/// past what anyone reads. Each phase reborrows what it uses, so its body
/// reads as if it still owned the arguments.
struct FrameContext<'a, P: Platform> {
    id: SurfaceId,
    surface: &'a mut surface_manager::ManagedSurface,
    wayland_state: &'a mut P,
    renderer: &'a mut Renderer,
    tree: &'a mut Tree,
}

/// What the loop knows about waiting work when it decides how long to sleep.
struct Pending {
    /// A surface has nothing on screen yet, so it must not wait for input to
    /// arrive before drawing.
    force_render: bool,
    /// Jobs queued by the previous frame — animation continuations, mostly.
    jobs: bool,
    /// A wake request that landed *after* this iteration consumed the flag.
    /// Blocking on one already set would suppress every later ping, so it
    /// counts as pending work rather than as a reason to sleep — see
    /// [`jobs::wake_request_pending`].
    wake_requested: bool,
    /// When the next scheduled thing is due, if anything is scheduled.
    deadline: Option<std::time::Instant>,
}

/// How long this iteration may sleep before it has to look again.
///
/// `None` blocks until the compositor or a ping. Nothing queued can be stranded
/// by that: work and its wakeup are one gesture — see `deferred` and the
/// ingress channel.
///
/// Waiting work polls at frame rate instead, and it outranks a deadline: the
/// deadline is the longer sleep of the two, and the queued job would sit
/// through it.
///
/// A deadline alone sleeps exactly that long. A caret blinks twice a second, so
/// this wakes once, where treating a schedule as pending work would spin at 60
/// fps to repaint nothing 113 frames out of 114.
///
/// `now` is passed rather than read so the decision can be asked about.
fn wait_for(pending: &Pending, now: std::time::Instant) -> Option<std::time::Duration> {
    if pending.jobs || pending.force_render || pending.wake_requested {
        return Some(std::time::Duration::from_millis(16));
    }
    pending
        .deadline
        .map(|deadline| deadline.saturating_duration_since(now))
}

/// Ask one surface for something, if it is still there.
///
/// Nine call sites want exactly this, and none has anything to say about a
/// surface that vanished between the frame opening and the request being made —
/// so the lookup is here rather than spelled out at each of them.
fn with_surface<P: Platform>(platform: &mut P, id: SurfaceId, f: impl FnOnce(&mut P::Surface<'_>)) {
    if let Some(mut surface) = platform.surface(id) {
        f(&mut surface);
    }
}

/// One surface, one frame, in the order the phases have to run.
fn render_surface<P: Platform>(
    ctx: &mut FrameContext<P>,
    layout_roots: &mut Vec<WidgetId>,
    woken: bool,
    active_roots: &rustc_hash::FxHashSet<WidgetId>,
    at: std::time::Instant,
) {
    let Some(frame) = open_frame(ctx) else {
        return;
    };

    let root = ctx.surface.widget_id;
    dispatch_events(&frame.events, root, ctx.tree, active_roots);

    let geometry = resolve_geometry(ctx, &frame);

    // This returns BEFORE draining jobs, and that is the whole point:
    // animation continuations have to stay queued. Advancing them on every
    // loop iteration would re-ping the wakeup each time and spin the loop
    // flat out between frame callbacks. Input was dispatched above, so it is
    // never delayed by the gate; initialisation and resizes bypass it.
    if paced_out(&frame, &geometry) {
        render_stats::record_frame_skipped();
        return;
    }

    // What time it is, for this whole frame. Declared once here because a
    // frame is these three passes: the animations advance, layout measures what
    // they produced, paint draws it. Asking the clock inside each of them made
    // one frame several instants, and made the middle of an animation something
    // no test could ask about — only sleep towards.
    //
    // Handed in rather than read, and sampled once per iteration rather than
    // once per surface: two bars drawn in the same pass are the same moment,
    // and a driver that wants to say when a frame happened has somewhere to
    // say it.
    ctx.tree.set_frame_instant(Some(at));

    run_jobs(root, ctx.tree, layout_roots, active_roots);

    // Nothing moved and nothing is starting up: there is no frame to draw.
    let has_pending_layouts = !layout_roots.is_empty();
    if !(frame.force_render_surface
        || woken
        || has_pending_layouts
        || geometry.needs_resize
        || geometry.scale_changed
        || ctx.tree.needs_paint(root))
    {
        ctx.tree.set_frame_instant(None);
        return;
    }

    layout_pass(ctx, &frame, &geometry, has_pending_layouts, layout_roots);
    paint_and_present(ctx, &frame, &geometry);
    // The frame is over, and so is its instant. Leaving it set would let a
    // later call read a time that has nothing to do with it, silently — the
    // failure a stale clock always has.
    ctx.tree.set_frame_instant(None);
}

pub struct App {
    /// Surface definitions added via add_surface()
    surface_definitions: Vec<SurfaceDefinition>,
    /// The layout tree for widget storage (owned by App)
    tree: Tree,
    /// Layout roots that need re-layout, keyed by surface root so one
    /// surface's render pass never lays out another surface's subtrees
    /// (each inner Vec is deduped — typically 1–3 entries per frame)
    layout_roots: rustc_hash::FxHashMap<WidgetId, Vec<WidgetId>>,
    /// Root owner for the reactive graph. When disposed, cascades cleanup
    /// through all signals, effects, and cleanup callbacks.
    root_owner_id: Option<OwnerId>,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface_definitions: Vec::new(),
            tree: Tree::new(),
            layout_roots: rustc_hash::FxHashMap::default(),
            root_owner_id: None,
        }
    }

    /// Set the application-wide default font family.
    ///
    /// This sets the default font family that will be used by all text widgets
    /// that don't explicitly specify a font family.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new()
    ///     .default_font_family(FontFamily::Name("Inter".into()))
    ///     .add_surface(config, || view)
    ///     .run();
    /// ```
    pub fn default_font_family(self, family: FontFamily) -> Self {
        set_default_font_family(family);
        self
    }

    /// Add a surface to the application.
    ///
    /// This method allows creating multiple layer shell surfaces within a single app.
    /// Each surface has its own widget tree but all surfaces share the same reactive
    /// signals and app state.
    ///
    /// The widget factory closure creates the root widget for the surface.
    ///
    /// Returns a `SurfaceId` that can be used to get a `SurfaceHandle` later via
    /// `surface_handle()` to modify surface properties.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new().run(|app| {
    ///     let bar_id = app.add_surface(
    ///         SurfaceConfig::new()
    ///             .height(32)
    ///             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    ///             .layer(Layer::Top)
    ///             .namespace("status-bar"),
    ///         || status_bar_widget()
    ///     );
    /// });
    /// ```
    pub fn add_surface<W, F>(&mut self, config: SurfaceConfig, widget_fn: F) -> SurfaceId
    where
        W: Widget + 'static,
        F: FnOnce() -> W + 'static,
    {
        let id = SurfaceId::next();
        self.surface_definitions.push(SurfaceDefinition {
            id,
            config,
            widget_fn: Box::new(move || Box::new(widget_fn())),
        });
        id
    }

    /// Run the application with a setup closure.
    ///
    /// The setup closure runs inside a root owner scope — all signals, effects,
    /// and other reactive primitives created within it are automatically cleaned
    /// up when the `App` is dropped. Use `app.add_surface()` inside the closure
    /// to define surfaces.
    ///
    /// # Panics
    ///
    /// Panics if no surfaces were added via `add_surface()` inside the closure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new().run(|app| {
    ///     let count = create_signal(0);
    ///     app.add_surface(config, move || build_ui(count));
    /// });
    /// ```
    pub fn run(mut self, setup: impl FnOnce(&mut Self)) -> ExitReason {
        // The loop is created before the connection because the keyboard is
        // built during the first dispatch, and it needs the loop handle to arm
        // its key-repeat timer.
        let mut event_loop: EventLoop<platform::WaylandState> =
            EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();

        // Both wakeup mechanisms are installed before anything can reach
        // them, which means before `setup` runs: an app's very first act is
        // often a `create_task` that writes a signal, and the compositor
        // starts talking during `create_wayland_app`'s roundtrips and the
        // configure loop. A producer that finds neither a channel nor a ping
        // handle has nowhere to put its wakeup.
        // Create ping mechanism for wakeup on signal changes
        let (ping, ping_source) = make_ping().expect("Failed to create ping");
        init_wakeup(ping);

        // Insert ping source - this wakes the loop when signals change
        loop_handle
            .insert_source(ping_source, |_, _, _| {
                // Ping received - a signal was updated, frame will be rendered
            })
            .expect("Failed to insert ping source");

        // Cross-thread ingress channel: background threads send messages
        // here instead of hand-rolling wakeups. calloop guarantees a send
        // wakes the next dispatch — see the ingress module.
        let (ingress_tx, ingress_channel) = calloop_channel::channel();
        ingress::install_ingress(ingress_tx);
        loop_handle
            .insert_source(ingress_channel, |event, _, wayland_state| {
                if let calloop_channel::Event::Msg(message) = event {
                    match message {
                        // Doorbell only: the write payloads live in the
                        // reactive write queue, drained at the flush point
                        // later this same iteration.
                        ingress::IngressMessage::BgWritesQueued => {}
                        // Prefetched selection content from a reader thread
                        ingress::IngressMessage::ClipboardUpdate {
                            kind,
                            generation,
                            content,
                        } => wayland_state.apply_clipboard_update(kind, generation, content),
                    }
                }
            })
            .expect("Failed to insert ingress channel");

        // Create root owner scope — all signals/effects created in setup are owned
        self.root_owner_id = Some(reactive::create_root_owner());
        setup(&mut self);

        if self.surface_definitions.is_empty() {
            // Not an error: outputs-driven apps start with zero surfaces and
            // spawn one per monitor from an effect once the compositor
            // reports its outputs. An app that never spawns anything will
            // just idle in the event loop.
            log::info!(
                "No static surfaces defined; waiting for dynamic surfaces \
                 (spawn_surface, e.g. from an outputs() effect)"
            );
        }

        let (connection, mut event_queue, mut wayland_state) =
            match create_wayland_app(loop_handle.clone()) {
                Ok(app) => app,
                Err(e) => {
                    log::error!("Cannot start: {e}");
                    return ExitReason::Error(e);
                }
            };

        // Create surfaces from add_surface() calls
        for def in &self.surface_definitions {
            wayland_state.create_surface_with_id(def.id, &def.config);
        }

        // Wait for all surfaces to configure
        while !wayland_state.all_surfaces_configured() && !wayland_state.should_exit() {
            if let Err(e) = event_queue.blocking_dispatch(&mut wayland_state) {
                log::error!("Wayland dispatch failed during configure: {e}");
                return ExitReason::Error(platform::PlatformError::ConnectionLost);
            }
        }

        if wayland_state.should_exit() {
            return ExitReason::Quit;
        }

        // Create shared GPU context
        let gpu_context = GpuContext::new();

        // Create surface manager and runtime entries for each surface
        let mut surface_manager = SurfaceManager::new();
        let mut renderer: Option<Renderer> = None;

        // Create entries for surfaces added via add_surface()
        for def in self.surface_definitions.drain(..) {
            let wayland_surface = wayland_state
                .get_surface(def.id)
                .expect("Surface should exist after configure");

            // Create the widget inside an owner scope so that signals/effects
            // created in the factory (e.g. create_memo) are properly owned.
            let (widget, owner_id) = with_owner(|| (def.widget_fn)());
            let mut managed =
                ManagedSurface::new(def.id, def.config, widget, owner_id, &mut self.tree);

            // Initialize GPU surface
            managed.init_gpu(
                &gpu_context,
                &wayland_state,
                wayland_surface.width,
                wayland_surface.height,
                wayland_surface.scale_factor,
                &mut self.tree,
            );

            // Create renderer from first surface
            if renderer.is_none()
                && let Some(ref wgpu_surface) = managed.wgpu_surface
            {
                let r = Renderer::new(
                    wgpu_surface.device().clone(),
                    wgpu_surface.queue().clone(),
                    wgpu_surface.format(),
                );
                renderer = Some(r);
            }

            surface_manager.add(managed);
        }

        // Insert Wayland source - this handles all Wayland protocol events
        WaylandSource::new(connection.clone(), event_queue)
            .insert(loop_handle.clone())
            .expect("Failed to insert Wayland source");

        // Main loop - event-driven, blocks until Wayland event or signal update
        loop {
            // Anything whose deadline has arrived becomes ordinary pending work.
            jobs::promote_due_jobs();

            let pending = Pending {
                force_render: wayland_state.any_surface_needs_render(),
                jobs: has_pending_jobs(),
                wake_requested: jobs::wake_request_pending(),
                deadline: jobs::next_deadline(),
            };
            let timeout = wait_for(&pending, std::time::Instant::now());

            if let Err(e) = event_loop.dispatch(timeout, &mut wayland_state) {
                // The connection died (compositor exited, protocol error).
                // Exit cleanly instead of panicking the process.
                log::error!("Event loop dispatch failed: {e}");
                return ExitReason::Error(platform::PlatformError::ConnectionLost);
            }

            let ctx = LoopContext {
                wayland_state: &mut wayland_state,
                surface_manager: &mut surface_manager,
                gpu_context: &gpu_context,
                renderer: &mut renderer,
            };
            if let Some(reason) = iterate(ctx, &mut self.tree, &mut self.layout_roots, None) {
                return reason;
            }
        }
    }
}

/// Every handle one iteration needs, in one place.
struct LoopContext<'a, P: Platform> {
    wayland_state: &'a mut P,
    surface_manager: &'a mut SurfaceManager,
    gpu_context: &'a GpuContext,
    renderer: &'a mut Option<Renderer>,
}

/// Everything one iteration does once it has finished waiting.
///
/// Split from the waiting because the waiting is calloop's and this is
/// guido's: a driver that wants to step the application forward wants this
/// half and not the other. `run` calls it after every dispatch; #264 step 5
/// is where something else does.
///
/// `frame_at` names the moment every surface drawn this pass is drawn at, for a
/// driver that wants to. `None` takes it at the render pass, which is what the
/// loop passes: sampling before this function would date the frame from before
/// the session lock, the surface commands and — on the iteration that builds
/// one — the whole of `Renderer::new`.
///
/// `Some` ends the loop with that reason.
fn iterate<P: Platform>(
    ctx: LoopContext<'_, P>,
    tree: &mut Tree,
    layout_roots: &mut rustc_hash::FxHashMap<WidgetId, Vec<WidgetId>>,
    frame_at: Option<std::time::Instant>,
) -> Option<ExitReason> {
    let LoopContext {
        gpu_context,
        wayland_state,
        surface_manager,
        renderer,
    } = ctx;
    // Reset ping coalescing first: the first wake_loop from here on sends a
    // fresh ping, so the next dispatch cannot block on work queued during
    // this iteration.
    jobs::mark_loop_awake();

    // Check for programmatic exit/restart requests
    match get_exit_request() {
        jobs::ExitRequest::Quit => return Some(ExitReason::Quit),
        jobs::ExitRequest::Restart => return Some(ExitReason::Restart),
        jobs::ExitRequest::Running => {}
    }

    if wayland_state.should_exit() {
        return Some(ExitReason::Quit);
    }

    // Drive the session-lock state machine (lock/unlock requests,
    // grant/denial events, per-output lock surfaces)
    session_lock::process_session_lock(surface_manager, wayland_state, tree);

    // Run deferred owner disposals (public dispose_owner). Safe
    // here: no user closure is on the stack.
    reactive::owner::flush_pending_disposals();

    // Process dynamic surface commands
    if !process_surface_commands(surface_manager, wayland_state, tree) {
        return Some(ExitReason::Quit);
    }

    // Initialize GPU for any pending surfaces (newly created dynamic surfaces)
    surface_manager.init_pending_gpu(gpu_context, wayland_state, tree);

    // Lazily create the shared renderer once the first surface has a
    // GPU state (apps may start with zero surfaces and spawn them
    // dynamically, e.g. one bar per output).
    if renderer.is_none()
        && let Some(wgpu_surface) = surface_manager.first_gpu_surface()
    {
        *renderer = Some(Renderer::new(
            wgpu_surface.device().clone(),
            wgpu_surface.queue().clone(),
            wgpu_surface.format(),
        ));
    }

    // Flush background-thread signal writes once per frame (queued via WriteSignal).
    // Must run before take_wake_request() so that signal changes from bg writes
    // are processed into jobs before we check the wake request.
    reactive::flush_bg_writes();

    // Take the wake request once for all surfaces (not per-surface)
    let woken = take_wake_request();

    // One instant for every surface this pass draws, taken here rather than at
    // the top of the iteration so it dates the frame and not the work ahead of
    // it. A driver that named one is obeyed.
    let frame_at = frame_at.unwrap_or_else(std::time::Instant::now);

    // Render each surface (no renderer yet means no surface has a
    // GPU state — nothing can be rendered this iteration)
    if let Some(renderer) = renderer.as_mut() {
        let surface_ids: Vec<SurfaceId> = surface_manager.ids().collect();

        // Live surface roots: the ownership domain for job
        // scheduling. Computed after surface commands/session-lock
        // processing so closed surfaces are already gone.
        let active_roots: rustc_hash::FxHashSet<WidgetId> = surface_ids
            .iter()
            .filter_map(|sid| surface_manager.get(*sid).map(|s| s.widget_id))
            .collect();

        // Resolve job ownership, then run the orphan lane: jobs
        // whose widget has no live surface (deferred Unregister
        // cleanup from closed surfaces, mostly) must not wait for
        // a render pass that will never come.
        jobs::distribute_jobs(tree, &active_roots);
        let orphans = jobs::drain_orphan_jobs();
        if !orphans.is_empty() {
            let mut scratch_layout_roots = Vec::new();
            process_jobs(&orphans, tree, &mut scratch_layout_roots);
        }
        jobs::recycle_job_buffer(orphans);

        // Drop layout roots of surfaces that no longer exist.
        layout_roots.retain(|root, _| active_roots.contains(root));

        for id in surface_ids {
            let Some(surface) = surface_manager.get_mut(id) else {
                continue;
            };
            let layout_roots = layout_roots.entry(surface.widget_id).or_default();
            let mut ctx = FrameContext {
                id,
                surface,
                wayland_state,
                renderer,
                tree,
            };
            render_surface(&mut ctx, layout_roots, woken, &active_roots, frame_at);
        }
    }

    // Hand the seat what the widgets changed — after the render
    // pass, so a copy or a cursor set by this iteration's event
    // handling still goes out in this iteration, and outside it, so
    // it goes out whether or not any surface had a frame to draw.
    sync_platform_state(wayland_state);

    // Everything this iteration queued goes out at once.
    if !wayland_state.flush() {
        return Some(ExitReason::Error(platform::PlatformError::ConnectionLost));
    }
    None
}

impl Drop for App {
    fn drop(&mut self) {
        // Dispose the root owner first — cascades cleanup through the entire
        // reactive graph (signals, effects, cleanup callbacks).
        if let Some(root_id) = self.root_owner_id {
            reactive::dispose_owner_now(root_id);
        }

        // Clear the tree BEFORE resetting jobs. Dropping widgets triggers
        // ChildrenSource::drop() which pushes Unregister jobs. If we reset
        // jobs first, these late Unregister jobs survive into the next App
        // and destroy the new tree's widgets (which reuse the same IDs).
        self.tree.clear();

        // Reset all thread-local and static state so the next App can start clean.
        reactive::reset_reactive();
        jobs::reset_jobs();
        ingress::reset_ingress();
        surface::reset_surface_commands();
        surface::reset_popups();
        widget_ref::reset_widget_refs();
        reactive::focus::reset_pending_focus();
        session_lock::reset_session_lock();
        FONTS_CONSUMED.with(|f| f.set(false));
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod exclusive_zone_resync_tests {
    use super::*;
    use crate::platform::Anchor;
    use crate::surface::{ExclusiveZone, Margin, SurfaceExtent};

    /// The reservation has to follow the size that was just *requested*. The
    /// compositor only tells us the new size a round trip later, so resolving
    /// against what it last said reserves for the size the surface is leaving.
    #[test]
    fn a_fixed_axis_resolves_against_the_requested_size() {
        // A bar that was 32 tall and has just asked for 48.
        let live_height = Some(32);
        let height = surface::requested_extent(SurfaceExtent::Fixed(48), live_height);
        assert_eq!(height, 48, "the request wins over the stale configure");

        let zone =
            ExclusiveZone::Auto.resolve(Anchor::TOP, Margin::from([6, 0, 0, 0]), 800, height);
        assert_eq!(zone, 48 + 6);
    }

    /// A content axis has no number of its own yet — `initial()` is 1px, which
    /// must never be what a running surface asks for.
    #[test]
    fn a_content_axis_holds_the_live_size_until_it_is_measured() {
        assert_eq!(
            surface::requested_extent(SurfaceExtent::Content, Some(240)),
            240
        );
        // Before the first configure there is nothing better to say, and the
        // placeholder is what creation asks for too — the content-measure pass
        // runs on a surface's first frames regardless, so this is a size it is
        // on its way out of rather than one it can be stuck at. What must not
        // happen is reporting the placeholder as though the compositor had
        // agreed to it, which is why `live` is the *configured* size.
        assert_eq!(
            surface::requested_extent(SurfaceExtent::Content, None),
            SurfaceExtent::Content.initial()
        );
        assert_eq!(
            surface::requested_extent(SurfaceExtent::Content, Some(1)),
            1,
            "and once configured at 1px, 1px is the honest answer"
        );
    }

    /// The anchor is what an `Auto` reservation reads to decide which axis it
    /// follows and which edge's margin counts, so a bar becoming a side dock has
    /// to reserve on the other axis. It used to keep reserving its old one:
    /// `SetAnchor` was the one command that recorded nothing on the config.
    #[test]
    fn a_reservation_follows_the_anchor_it_was_given() {
        let margin = Margin::from([6, 20, 9, 20]);

        // A 800x32 bar at the top reserves its height plus the top margin.
        let bar = ExclusiveZone::Auto.resolve(Anchor::TOP, margin, 800, 32);
        assert_eq!(bar, 32 + 6);

        // The same surface, re-anchored as a 48-wide dock on the left, has to
        // reserve its width plus the left margin instead.
        let dock = ExclusiveZone::Auto.resolve(Anchor::LEFT, margin, 48, 600);
        assert_eq!(dock, 48 + 20);
        assert_ne!(dock, bar, "the axis genuinely changes with the anchor");
    }

    /// Handing an axis back to `content()` at runtime must not ask for 1px, and
    /// must ask for a layout — nothing else brings the content-measure pass
    /// round for a surface whose widgets did not change.
    #[test]
    fn handing_an_axis_to_content_keeps_the_size_and_asks_for_a_measure() {
        let live = Some((300u32, 40u32));
        let anchored = |w: SurfaceExtent, h: SurfaceExtent| SurfaceConfig {
            anchor: Anchor::TOP,
            width: w,
            height: h,
            ..SurfaceConfig::new()
        };

        let (w, h, measure) =
            surface::resize_request(&anchored(SurfaceExtent::Content, 40.into()), live);
        assert_eq!((w, h), (300, 40), "no 1px collapse");
        assert!(measure, "the measure pass has to be woken");

        let (w, h, measure) = surface::resize_request(&anchored(200.into(), 60.into()), live);
        assert_eq!((w, h), (200, 60));
        assert!(!measure, "two fixed axes need no measure");
    }

    /// A `content()` axis the compositor owns is measured and then thrown away.
    /// Asking for the measure anyway re-lays out the whole subtree on every
    /// resize and every re-anchoring, to produce a number nothing can use.
    #[test]
    fn a_stretched_content_axis_asks_for_no_measure() {
        let live = Some((1920u32, 32u32));
        let bar = |anchor: Anchor| SurfaceConfig {
            anchor,
            width: SurfaceExtent::Content,
            height: 32.into(),
            ..SurfaceConfig::new()
        };

        let stretched = bar(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        let (_, _, measure) = surface::resize_request(&stretched, live);
        assert!(!measure, "the width is the compositor's either way");
        assert!(
            !surface::needs_content_measure(&stretched),
            "and the frame loop's measure pass asks the same question, or it \
             re-measures a discarded axis on every frame that lays anything out"
        );

        let ours = bar(Anchor::TOP | Anchor::LEFT);
        let (_, _, measure) = surface::resize_request(&ours, live);
        assert!(
            measure,
            "anchored to one edge it is ours, and has to be measured"
        );
        assert!(surface::needs_content_measure(&ours));
    }

    /// And until it *is* measured there is nothing to reserve for. The confirmed
    /// size of a stretched axis is the compositor's, not a measurement: a bar
    /// declared `width(content()).anchor(TOP | LEFT | RIGHT)` is confirmed at the
    /// screen's 1920, and re-anchoring it into a side dock turns that into a
    /// reservation for the whole screen — every other window pushed off it — for
    /// the frame between the command and the measure.
    #[test]
    fn a_reservation_waits_for_the_measure_rather_than_guessing() {
        // A left dock: anchored to one horizontal edge and both vertical ones, so
        // the height is the compositor's and the reservation follows the width.
        let dock = SurfaceConfig {
            anchor: Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
            width: SurfaceExtent::Content,
            height: 32.into(),
            exclusive_zone: surface::ExclusiveZone::Auto,
            ..SurfaceConfig::new()
        };

        // What the stretched anchor left behind, and what the content is worth.
        let stale = 1920;
        let measured = 240;

        assert!(
            surface::needs_content_measure(&dock),
            "the width is ours now, so it is waiting on a measure"
        );
        assert_eq!(
            dock.exclusive_zone
                .resolve(dock.anchor, dock.margin, measured, 32),
            240,
            "the measure is what it reserves for"
        );
        assert_eq!(
            dock.exclusive_zone
                .resolve(dock.anchor, dock.margin, stale, 32),
            1920,
            "and the confirmed size is the whole screen, which is why publishing \
             from it has to wait"
        );
    }

    /// Only `Auto` waits, because only `Auto` reads a size. The other three are
    /// constants, and nothing republishes a policy that is not `Auto` — so a
    /// deferred one is a lost one: `set_exclusive_zone(Fixed(40))` on a surface
    /// with a `content()` axis would never reach the compositor at all.
    #[test]
    fn only_a_reservation_that_reads_a_size_waits_for_one() {
        let toast = |zone| SurfaceConfig {
            anchor: Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
            width: SurfaceExtent::Content,
            height: SurfaceExtent::Content,
            exclusive_zone: zone,
            ..SurfaceConfig::new()
        };

        assert!(
            surface::needs_content_measure(&toast(surface::ExclusiveZone::Auto)),
            "the premise: this is the surface whose measure is pending"
        );

        // Whatever size these are handed, they answer the same thing — so there
        // is nothing for them to wait for.
        for zone in [
            surface::ExclusiveZone::Fixed(40),
            surface::ExclusiveZone::None,
            surface::ExclusiveZone::Ignore,
        ] {
            let config = toast(zone);
            let unmeasured = zone.resolve(config.anchor, config.margin, 1, 1);
            let measured = zone.resolve(config.anchor, config.margin, 240, 60);
            assert_eq!(
                unmeasured, measured,
                "{zone:?} does not depend on the size, so deferring it only loses it"
            );
        }
    }

    /// Which axes are the compositor's follows from the anchor, and layer-shell
    /// is strict about it: omitting a dimension *without* opposite-edge
    /// anchoring is a protocol error, so a bar re-anchored to one edge has to
    /// stop asking for zero on the axis it now owns — at the next commit, which
    /// is every frame, the compositor would close the connection.
    #[test]
    fn the_size_asked_for_follows_the_anchor() {
        let bar = |anchor: Anchor| SurfaceConfig {
            anchor,
            width: 800.into(),
            height: 32.into(),
            ..SurfaceConfig::new()
        };
        let live = Some((1920u32, 32u32));

        let stretched =
            surface::resize_request(&bar(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT), live);
        assert_eq!(
            (stretched.0, stretched.1),
            (0, 32),
            "anchored to both horizontal edges, the width is the compositor's"
        );

        let pinned = surface::resize_request(&bar(Anchor::TOP | Anchor::LEFT), live);
        assert_eq!(
            (pinned.0, pinned.1),
            (800, 32),
            "anchored to one, it is ours again and must be sent"
        );

        let dock = surface::resize_request(&bar(Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM), live);
        assert_eq!(
            (dock.0, dock.1),
            (800, 0),
            "and the other axis, the other way"
        );
    }
}

#[cfg(test)]
mod font_registry_tests {
    /// An app that reloads its config calls load_font again on every run.
    /// Without dedup each run kept another copy of the same bytes.
    #[test]
    fn loading_the_same_font_twice_registers_it_once() {
        let font = vec![7u8; 64];
        super::load_font(font.clone());
        let after_first = super::CUSTOM_FONTS.with(|f| f.borrow().len());
        super::load_font(font);
        let after_second = super::CUSTOM_FONTS.with(|f| f.borrow().len());
        assert_eq!(after_first, after_second);

        super::load_font(vec![9u8; 64]);
        assert_eq!(
            super::CUSTOM_FONTS.with(|f| f.borrow().len()),
            after_second + 1,
            "a different font must still register"
        );
    }
}

#[cfg(test)]
mod dispatch_declares_the_moment {
    use super::*;
    use crate::layout::Size;
    use crate::widgets::widget::{Event, EventResponse};

    /// A widget that records what time the tree said it was when it was handed
    /// an event.
    struct Spy(std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>);

    impl widgets::Widget for Spy {
        fn layout(
            &mut self,
            tree: &mut Tree,
            id: WidgetId,
            constraints: crate::layout::Constraints,
        ) -> Size {
            let size = Size::new(constraints.max_width, constraints.max_height);
            tree.cache_layout(id, constraints, size);
            size
        }

        fn paint(&self, _tree: &Tree, _id: WidgetId, _ctx: &mut crate::renderer::PaintContext) {}

        fn event(&mut self, tree: &mut Tree, _id: WidgetId, _event: &Event) -> EventResponse {
            self.0.set(Some(tree.event_instant()));
            EventResponse::Handled
        }
    }

    /// The queue carries each event's moment and `dispatch_events` is what puts
    /// it where a widget can read it. Without that one line every widget falls
    /// back to the clock, which is what they all did before this existed — and
    /// the tests of the widgets themselves cannot tell, because they declare
    /// the instant by hand.
    ///
    /// So this one does not: it goes through the dispatcher, as the loop does.
    #[test]
    fn a_widget_is_told_when_the_event_it_is_handed_happened() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Spy(seen.clone())));
        tree.set_origin(root, 0.0, 0.0);

        // An hour ago: no clock this process could read would answer with it,
        // so the widget can only have got it from the event.
        let happened = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        let events = [(happened, Event::MouseMove { x: 10.0, y: 10.0 })];

        dispatch_events(&events, root, &mut tree, &Default::default());

        assert_eq!(
            seen.get(),
            Some(happened),
            "the widget has to be told the moment the queue carried, not the \
             moment the dispatch ran"
        );
        // And once the dispatch is over the moment is gone: what a later
        // reader gets is the clock, not the last event's time.
        let after = tree.event_instant();
        assert!(
            after > happened,
            "the moment has to be cleared when the dispatch ends, got {after:?} \
             for an event from an hour ago"
        );
    }
}

/// A frame is a description of what the compositor said, and the pacing gate is
/// a decision taken from that description. Neither needs a compositor to exist,
/// and until the Wayland types came out of the frame path neither could be
/// named here: `Frame` held a live `WlSurface`, and a test has no display to
/// get one from.
#[cfg(test)]
mod a_frame_is_a_description_not_a_connection {
    use super::*;

    fn frame(callback_pending: bool, force: bool) -> Frame {
        Frame {
            events: Vec::new(),
            scale_factor: 1.0,
            width: 200,
            height: 50,
            frame_callback_pending: callback_pending,
            force_render_surface: force,
        }
    }

    fn geometry(needs_resize: bool, scale_changed: bool) -> Geometry {
        Geometry {
            physical_width: 200,
            physical_height: 50,
            needs_resize,
            scale_changed,
        }
    }

    #[test]
    fn a_frame_can_be_described_without_a_display() {
        let f = frame(false, false);
        assert_eq!((f.width, f.height), (200, 50));
    }

    #[test]
    fn a_callback_still_in_flight_paces_a_frame_out() {
        assert!(paced_out(&frame(true, false), &geometry(false, false)));
    }

    /// Each on its own, because any one of them alone has to open the gate.
    #[test]
    fn a_first_frame_a_resize_and_a_scale_change_each_bypass_the_gate() {
        assert!(!paced_out(&frame(true, true), &geometry(false, false)));
        assert!(!paced_out(&frame(true, false), &geometry(true, false)));
        assert!(!paced_out(&frame(true, false), &geometry(false, true)));
    }

    #[test]
    fn no_callback_in_flight_paces_nothing_out() {
        assert!(!paced_out(&frame(false, false), &geometry(false, false)));
    }
}

/// The frame path asks a compositor for things, and this is the asking written
/// down. `WaylandState` is one answer to it; a recorder that keeps what it was
/// asked is another, and having two is the whole point — one implementation is
/// a rename, two is a seam.
#[cfg(test)]
mod a_second_host_can_answer_for_a_compositor {
    use super::*;
    use crate::platform::Anchor;
    use crate::surface::{ExclusiveZone, Margin, SurfaceConfig};

    /// Answers nothing and remembers what it was asked. Overrides `open_frame`,
    /// which is required, and the two methods these tests read; inherits the
    /// other sixteen, which is the point of the defaults.
    #[derive(Default)]
    struct Recorder {
        configured: Option<(u32, u32)>,
        exclusive_zones: Vec<i32>,
    }

    impl Surface for Recorder {
        fn open_frame(&mut self) -> Option<Frame> {
            None
        }

        fn configured_size(&self) -> Option<(u32, u32)> {
            self.configured
        }

        fn set_exclusive_zone(&mut self, zone: i32) {
            self.exclusive_zones.push(zone);
        }
    }

    fn bar() -> SurfaceConfig {
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .exclusive_zone(ExclusiveZone::Auto)
    }

    /// The first thing anything has ever asserted about what a surface *asks
    /// the compositor for*, rather than about what it draws.
    #[test]
    fn a_bar_reserving_automatically_asks_for_the_height_it_declared() {
        let mut surface = Recorder {
            configured: Some((800, 32)),
            ..Default::default()
        };
        resync_exclusive_zone(&mut surface, &bar(), None);
        assert_eq!(surface.exclusive_zones, vec![32]);
    }

    /// The margin on the anchored edge is part of the reservation: a bar held
    /// eight pixels off the top edge occupies forty.
    #[test]
    fn a_margin_on_the_anchored_edge_is_reserved_too() {
        let mut surface = Recorder {
            configured: Some((800, 32)),
            ..Default::default()
        };
        resync_exclusive_zone(&mut surface, &bar().margin(Margin::all(8)), None);
        assert_eq!(surface.exclusive_zones, vec![40]);
    }

    /// A policy that is not `Auto` sends nothing from here — it was sent once,
    /// when the surface was configured.
    #[test]
    fn a_fixed_reservation_is_not_republished_every_frame() {
        let mut surface = Recorder::default();
        resync_exclusive_zone(&mut surface, &bar().exclusive_zone(48u32), None);
        assert!(surface.exclusive_zones.is_empty());
    }
}

/// How long an iteration is allowed to sleep is a decision, and it was four
/// conditions inline in `App::run` — reachable only by running a compositor.
#[cfg(test)]
mod what_the_loop_waits_for {
    use super::*;
    use std::time::{Duration, Instant};

    const FRAME: Duration = Duration::from_millis(16);

    fn idle() -> Pending {
        Pending {
            force_render: false,
            jobs: false,
            wake_requested: false,
            deadline: None,
        }
    }

    /// Nothing queued and nothing scheduled: sleep until the compositor or a
    /// ping says otherwise. Anything else would spin.
    #[test]
    fn an_idle_loop_blocks() {
        assert_eq!(wait_for(&idle(), Instant::now()), None);
    }

    /// Each on its own, because any one of them means work is waiting and a
    /// block would strand it. `wake_requested` is the subtle one: it lands
    /// *after* the iteration consumed the flag, and blocking on it would
    /// suppress every later ping.
    #[test]
    fn any_pending_work_polls_at_frame_rate() {
        for pending in [
            Pending {
                jobs: true,
                ..idle()
            },
            Pending {
                force_render: true,
                ..idle()
            },
            Pending {
                wake_requested: true,
                ..idle()
            },
        ] {
            assert_eq!(wait_for(&pending, Instant::now()), Some(FRAME));
        }
    }

    /// A caret blinks twice a second. Sleeping exactly that long wakes once,
    /// where treating a schedule as pending work would repaint nothing 113
    /// frames out of 114.
    #[test]
    fn a_deadline_is_slept_to_exactly() {
        let now = Instant::now();
        let pending = Pending {
            deadline: Some(now + Duration::from_millis(500)),
            ..idle()
        };
        assert_eq!(wait_for(&pending, now), Some(Duration::from_millis(500)));
    }

    /// A deadline already gone is not a negative sleep.
    #[test]
    fn a_deadline_in_the_past_does_not_wait() {
        let now = Instant::now();
        let pending = Pending {
            deadline: Some(now - Duration::from_secs(1)),
            ..idle()
        };
        assert_eq!(wait_for(&pending, now), Some(Duration::ZERO));
    }

    /// Pending work outranks a deadline: the deadline would be the longer
    /// sleep, and the queued job would sit through it.
    #[test]
    fn pending_work_outranks_a_later_deadline() {
        let now = Instant::now();
        let pending = Pending {
            jobs: true,
            deadline: Some(now + Duration::from_secs(10)),
            ..idle()
        };
        assert_eq!(wait_for(&pending, now), Some(FRAME));
    }
}

/// The frame path, driven with nothing behind it.
///
/// What a test can reach from outside the crate is [`crate::testing::Headless`],
/// and `tests/headless_app.rs` uses it. This module keeps only what needs the
/// internals: `resolve_geometry`'s answer, which a driver deliberately does not
/// expose.
#[cfg(test)]
mod a_frame_lands_where_the_surface_points {
    use super::*;
    use crate::renderer::{GpuContext, RenderTarget};
    use crate::surface::SurfaceConfig;
    use crate::widgets::container;

    const W: u32 = 100;
    const H: u32 = 32;

    /// A surface with one frame's worth of facts and nothing else.
    struct OneSurface {
        width: u32,
        height: u32,
    }

    impl Surface for OneSurface {
        fn open_frame(&mut self) -> Option<Frame> {
            Some(Frame {
                events: Vec::new(),
                scale_factor: 1.0,
                width: self.width,
                height: self.height,
                frame_callback_pending: false,
                force_render_surface: true,
            })
        }
    }

    struct OnePlatform(OneSurface);

    /// The handle, as the two real implementations spell it: a newtype over
    /// what it borrows, so the trait does not have to be implemented for
    /// references as well.
    struct OneHandle<'a>(&'a mut OneSurface);

    impl Surface for OneHandle<'_> {
        fn open_frame(&mut self) -> Option<Frame> {
            self.0.open_frame()
        }
    }

    impl Platform for OnePlatform {
        type Surface<'a> = OneHandle<'a>;

        fn surface(&mut self, _id: SurfaceId) -> Option<OneHandle<'_>> {
            Some(OneHandle(&mut self.0))
        }
    }

    fn gpu() -> Option<GpuContext> {
        match GpuContext::try_new() {
            Some(gpu) => Some(gpu),
            None if std::env::var_os("GUIDO_GPU_REQUIRED").is_some() => {
                panic!("GUIDO_GPU_REQUIRED is set and no GPU adapter was found")
            }
            None => {
                eprintln!("no GPU adapter; skipping");
                None
            }
        }
    }

    /// A target smaller than its frame is made to fit, and then reports that it
    /// fits.
    ///
    /// The second half is the whole of it: a target that quietly declined would
    /// pass the first assertion of every frame and report a resize on all of
    /// them, repainting the entire tree for ever.
    #[test]
    fn a_target_the_wrong_size_is_resized_once_and_not_again() {
        let Some(gpu) = gpu() else { return };

        let mut tree = Tree::new();
        let (widget, owner) = reactive::with_owner(|| Box::new(container()) as Box<dyn Widget>);
        let mut surface = surface_manager::ManagedSurface::new(
            SurfaceId::next(),
            SurfaceConfig::new(),
            widget,
            owner,
            &mut tree,
        );
        surface.wgpu_surface = Some(RenderTarget::offscreen(&gpu, 8, 8));
        let mut renderer = Renderer::new(
            gpu.device.clone(),
            gpu.queue.clone(),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let mut platform = OnePlatform(OneSurface {
            width: W,
            height: H,
        });
        let id = surface.id;
        let mut ctx = FrameContext {
            id,
            surface: &mut surface,
            wayland_state: &mut platform,
            renderer: &mut renderer,
            tree: &mut tree,
        };
        let frame = ctx.wayland_state.surface(id).unwrap().open_frame().unwrap();

        let first = resolve_geometry(&mut ctx, &frame);
        assert!(first.needs_resize, "an 8x8 target for a 100x32 frame");
        assert_eq!(
            (
                ctx.surface.wgpu_surface.as_ref().unwrap().width(),
                ctx.surface.wgpu_surface.as_ref().unwrap().height()
            ),
            (W, H)
        );

        let again = resolve_geometry(&mut ctx, &frame);
        assert!(!again.needs_resize, "the target already fits");
    }
}
