pub mod animation;
pub mod backdrop;
mod blur;
pub mod compositor;
pub mod image_metadata;
mod ingress;
mod jobs;
pub mod keyboard;
pub mod layout;
pub mod outputs;
pub mod reactive;
pub mod render_stats;
pub mod session_lock;
pub mod surface;
mod surface_manager;
pub mod transform;
pub mod transform_origin;
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

pub mod prelude {
    pub use crate::animation::{SpringConfig, TimingFunction, Transition, TransitionConfig};
    pub use crate::backdrop::{BackdropBlur, BackdropSources};
    pub use crate::compositor::{CompositorEffects, compositor_effects};
    pub use crate::keyboard::keyboard_modifiers;
    pub use crate::layout::{
        Axis, Constraints, CrossAlignment, Flex, IntoF32, Length, MainAlignment, Size, ZStack,
        at_least, at_most, fill, fraction,
    };
    pub use crate::outputs::{OutputId, OutputInfo, outputs, surface_output};
    pub use crate::platform::{Anchor, KeyboardInteractivity, Layer};
    pub use crate::reactive::{
        Callback, CursorIcon, Memo, OptionSignalExt, RwSignal, Service, Signal, Trigger,
        WriteSignal, create_derived, create_effect, create_memo, create_service, create_signal,
        create_stored, create_trigger, expect_context, has_context, on_cleanup, provide_context,
        provide_signal_context, set_cursor, use_context, with_context,
    };
    pub use crate::renderer::{PaintContext, Shadow, measure_text};
    pub use crate::session_lock::{
        LockState, lock_session, lock_state, session_locked, unlock_session,
    };
    pub use crate::surface::{
        ExclusiveZone, PopupAnchor, PopupConfig, PopupGravity, PopupHandle, SurfaceConfig,
        SurfaceExtent, SurfaceHandle, SurfaceId, content, spawn_popup, spawn_surface,
        surface_handle,
    };
    pub use crate::transform::Transform;
    pub use crate::transform_origin::{HorizontalAnchor, TransformOrigin, VerticalAnchor};
    pub use crate::widget_ref::{WidgetRef, create_widget_ref};
    pub use crate::widgets::{
        AnyWidget, Border, Color, Container, ContentFit, CornerRadii, Event, EventResponse,
        FontFamily, FontWeight, GradientDirection, Image, ImageSource, IntoChildren, Key,
        LinearGradient, Modifiers, MouseButton, Overflow, Padding, Rect, ScrollAxis, ScrollSource,
        ScrollbarBuilder, ScrollbarVisibility, Selection, StateStyle, Text, TextInput, TextShadow,
        TextStroke, TextStyle, Widget, container, image, keyed, text, text_input,
    };
    pub use crate::{
        App, ExitReason, SignalFields, component, default_font_family, load_font, quit_app,
        restart_app, set_default_font_family,
    };
}

use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;

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
fn close_surface_now(
    id: SurfaceId,
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut platform::WaylandState,
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

/// Process dynamic surface commands (create, close, property changes).
/// Returns false if all surfaces have been closed and the app should exit.
fn process_surface_commands(
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut platform::WaylandState,
    qh: &QueueHandle<platform::WaylandState>,
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
                wayland_state.create_surface_with_id(qh, id, &config);

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
                    wayland_state.exit = true;
                    return false;
                }
            }
            SurfaceCommand::SetLayer { id, layer } => {
                wayland_state.set_surface_layer(id, layer);
            }
            SurfaceCommand::SetKeyboardInteractivity { id, mode } => {
                wayland_state.set_surface_keyboard_interactivity(id, mode);
            }
            SurfaceCommand::SetAnchor { id, anchor } => {
                wayland_state.set_surface_anchor(id, anchor);
            }
            SurfaceCommand::SetSize { id, width, height } => {
                wayland_state.set_surface_size(id, width, height);
            }
            SurfaceCommand::SetExclusiveZone { id, zone } => {
                wayland_state.set_surface_exclusive_zone(id, zone);
            }
            SurfaceCommand::SetMargin {
                id,
                top,
                right,
                bottom,
                left,
            } => {
                wayland_state.set_surface_margin(id, top, right, bottom, left);
            }
            SurfaceCommand::SetInputRegion { id, rects } => {
                wayland_state.set_surface_input_region(id, rects.as_deref());
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

                if wayland_state.create_popup_surface_with_id(
                    qh,
                    id,
                    parent,
                    &config,
                    (config.width, height),
                ) {
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
/// Caching is an `Rc::clone` of the node already sitting in the frame's
/// render tree — O(1) per node instead of the previous deep subtree clone.
///
/// Returns whether the subtree is partial (this node or any descendant had
/// children culled by cull_rect). Partial-ness propagates UP: an ancestor
/// whose subtree embeds an incomplete paint must not be cached either, or a
/// later cache reuse would permanently hide the culled grandchildren.
///
/// Also clears needs_paint flags and the per-frame `repainted` marker for
/// freshly painted widgets (skipping already-clean subtrees entirely).
fn cache_paint_results(tree: &mut Tree, node: &std::rc::Rc<renderer::RenderNode>) -> bool {
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
        if !subtree_partial {
            // Complete paint — safe to cache for future reuse.
            tree.cache_paint(widget_id, std::rc::Rc::clone(node));
        }
        // else: partial subtree — don't cache. Keep the previous complete
        // cache (if any) so it can be reused when the widget becomes fully
        // visible. If there's no previous cache, the widget will get a full
        // paint next frame anyway (cached_paint None → full paint).
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
    events: &[widgets::Event],
    root: WidgetId,
    tree: &mut Tree,
    active_roots: &rustc_hash::FxHashSet<WidgetId>,
) {
    for event in events {
        reactive::diagnostics::snapshot_zone(|| {
            tree.with_widget_mut(root, |widget, id, tree| {
                widget.event(tree, id, event);
            });
        });
    }

    jobs::distribute_jobs(tree, active_roots);
}

/// Push anything the widgets changed back out to the compositor: the
/// clipboard after a copy, the primary selection after a select-to-copy, the
/// cursor shape after a hover.
fn sync_platform_state(
    wayland_state: &mut platform::WaylandState,
    qh: &QueueHandle<platform::WaylandState>,
) {
    if let Some(text) = take_clipboard_change() {
        wayland_state.set_clipboard(text, qh);
    }
    if let Some(text) = reactive::take_primary_change() {
        wayland_state.set_primary(text, qh);
    }
    if let Some(cursor) = take_cursor_change() {
        wayland_state.set_cursor(cursor, qh);
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

/// Deferred work that is queued and still waiting for the loop to drain it.
///
/// The wakeup contract says a producer that queues work must also guarantee a
/// wakeup that survives until its consumer runs. This is that contract as a
/// check instead of as prose — prose does not fail a test run.
///
/// Why an empty result is the only correct answer at the blocking point: each
/// of these is drained unconditionally, once per iteration. Anything still
/// queued was therefore produced *after* its own drain, and the only thing
/// that can bring the loop back to drain it is a wake request — which would
/// have kept it from blocking in the first place. So a non-empty queue here
/// means nobody asked to be woken, and the app is about to go deaf until an
/// unrelated compositor event happens along. That is precisely the shape of
/// the lost-wakeup bug this contract exists to prevent.
///
/// Returns the name of the offender, because the point is the message.
fn queued_but_unwoken() -> Option<&'static str> {
    [
        ("widget jobs", has_pending_jobs()),
        ("a wake request", jobs::wake_request_pending()),
        ("background signal writes", reactive::bg_writes_pending()),
        ("owner disposals", reactive::owner::disposals_pending()),
        ("surface commands", surface::surface_commands_pending()),
        ("a selection change", reactive::selection_change_pending()),
        ("a cursor change", reactive::cursor_change_pending()),
    ]
    .into_iter()
    .find_map(|(name, pending)| pending.then_some(name))
}

/// What this surface tells us about itself, read once at the top of a frame.
///
/// Copying it frees the borrow on `wayland_state`, which the phases below
/// need mutably — and nothing the compositor sends can change it before this
/// frame ends anyway.
struct Frame {
    events: Vec<widgets::Event>,
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
    wl_surface: WlSurface,
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
fn open_frame(ctx: &mut FrameContext) -> Option<Frame> {
    let surface = &*ctx.surface;
    let wayland_surface = ctx.wayland_state.get_surface_mut(ctx.id)?;
    if !wayland_surface.configured {
        return None;
    }

    // Taken before the GPU check, not after: a surface still building its GPU
    // state drops this frame *and* the input queued for it. Holding the input
    // instead would deliver a burst of stale events — a pointer position from
    // several frames ago among them — on the first frame that renders.
    let events = wayland_surface.take_events();
    let fully_initialized =
        wayland_surface.first_frame_presented && wayland_surface.scale_factor_received;
    let frame = Frame {
        events,
        scale_factor: wayland_surface.scale_factor,
        width: wayland_surface.width,
        height: wayland_surface.height,
        frame_callback_pending: wayland_surface.frame_callback_pending,
        force_render_surface: !fully_initialized,
        wl_surface: wayland_surface.wl_surface.clone(),
    };

    surface.is_gpu_ready().then_some(frame)
}

/// Resolve the physical size and bring the swapchain in line with it.
///
/// Runs after input rather than with the rest of the snapshot: dispatching
/// events cannot change the surface size, but resizing the swapchain before
/// the widgets have been told anything would reorder the frame for no gain.
fn resolve_geometry(ctx: &mut FrameContext, frame: &Frame) -> Geometry {
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
fn layout_pass(
    ctx: &mut FrameContext,
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
    let qh = ctx.qh;

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
    if has_pending_layouts && let Some(popup_width) = wayland_state.popup_auto_width(id) {
        let natural = measure_popup_height(tree, surface.widget_id, popup_width, id);
        tree.with_widget_mut(surface.widget_id, |widget, wid, tree| {
            widget.layout(tree, wid, constraints);
        });
        wayland_state.reposition_popup_if_changed(id, natural, qh);
    } else if (has_pending_layouts || frame.force_render_surface)
        && (surface.config.width.is_content() || surface.config.height.is_content())
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
        if (nw, nh) != (frame.width, frame.height) {
            wayland_state.set_surface_size(id, nw, nh);
            // Auto reservations follow automatic resizes too; every
            // other policy never moves
            if surface.config.exclusive_zone == surface::ExclusiveZone::Auto {
                let zone = surface.config.exclusive_zone.resolve(
                    surface.config.anchor,
                    surface.config.margin,
                    nw,
                    nh,
                );
                wayland_state.set_surface_exclusive_zone(id, zone);
            }
        }
    }

    // Update widget ref signals with current bounds after layout
    widget_ref::update_widget_refs(tree);
}

/// Paint, flatten, hand the frame to the GPU, and re-arm the pacing gate.
///
/// The order at the end is not free-form: the frame callback and the damage
/// are both requested BEFORE presenting, because presenting is what commits
/// the surface. Anything set afterwards would ride an empty second commit
/// the compositor cannot use.
fn paint_and_present(ctx: &mut FrameContext, frame: &Frame, geometry: &Geometry) {
    let id = ctx.id;
    let surface = &mut *ctx.surface;
    let wayland_state = &mut *ctx.wayland_state;
    let renderer = &mut *ctx.renderer;
    let tree = &mut *ctx.tree;
    let qh = ctx.qh;

    // Force full repaint on resize, scale change, or during initialization
    if frame.force_render_surface || geometry.needs_resize || geometry.scale_changed {
        tree.mark_subtree_needs_paint(surface.widget_id);
    }

    // Collect this surface's blur region from registered widgets (post
    // layout, so bounds are current).
    let blur_rects = blur::collect_for_surface(tree, surface.widget_id);

    // Skip frame if nothing needs paint
    if !tree.needs_paint(surface.widget_id) {
        // A blur-region change without a repaint (e.g. the compositor's
        // blur capability arriving) still needs its own commit.
        wayland_state.sync_blur_region(id, blur_rects, qh, true);
        render_stats::record_frame_skipped();
        render_stats::end_frame(&DamageRegion::None);
        return;
    }

    // Set the blur region now so it rides the buffer commit performed
    // inside present() — region and content change in the same frame.
    wayland_state.sync_blur_region(id, blur_rects, qh, false);

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
    time_phase!(render_stats::Phase::Flatten, {
        flatten_root_into(
            &surface.root_node,
            &mut surface.flattened_commands,
            &mut surface.command_layers,
        );
    });

    // Re-arm the frame callback BEFORE presenting so the request rides
    // the same commit as the buffer (present() commits internally). The
    // callback's arrival is the signal that this surface may render its
    // next frame. Previously the callback was requested exactly once at
    // startup and never again — the only pacing was the event loop
    // blocking inside the Fifo swapchain.
    frame.wl_surface.frame(qh, frame.wl_surface.clone());

    // Report damage BEFORE presenting: presenting attaches the buffer and
    // commits the frame.wl_surface (inside wgpu/the driver's Wayland path), so
    // pending damage set now is part of that commit. The previous code
    // damaged + committed AFTER present, attaching the damage to an empty
    // second commit the compositor could not use.
    let damage = tree.take_damage(surface.widget_id);
    match damage {
        DamageRegion::None => {
            // Shouldn't happen since we're rendering, but report full damage to be safe
            frame.wl_surface.damage_buffer(
                0,
                0,
                geometry.physical_width as i32,
                geometry.physical_height as i32,
            );
        }
        DamageRegion::Partial(rect) => {
            let scale = frame.scale_factor;
            frame.wl_surface.damage_buffer(
                (rect.x * scale) as i32,
                (rect.y * scale) as i32,
                (rect.width * scale).ceil() as i32,
                (rect.height * scale).ceil() as i32,
            );
        }
        DamageRegion::Full => {
            frame.wl_surface.damage_buffer(
                0,
                0,
                geometry.physical_width as i32,
                geometry.physical_height as i32,
            );
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

    // The frame callback requested before present is now committed;
    // rendering for this surface is gated until it fires.
    if let Some(ws) = wayland_state.get_surface_mut(id) {
        ws.frame_callback_pending = true;
    }
}

/// The borrows a frame needs from start to finish, in one place.
///
/// They are distinct objects in the caller, so bundling them costs nothing —
/// and it spares every phase below the parameter list that had already grown
/// past what anyone reads. Each phase reborrows what it uses, so its body
/// reads as if it still owned the arguments.
struct FrameContext<'a> {
    id: SurfaceId,
    surface: &'a mut surface_manager::ManagedSurface,
    wayland_state: &'a mut platform::WaylandState,
    renderer: &'a mut Renderer,
    tree: &'a mut Tree,
    qh: &'a QueueHandle<platform::WaylandState>,
}

/// One surface, one frame, in the order the phases have to run.
fn render_surface(
    ctx: &mut FrameContext,
    layout_roots: &mut Vec<WidgetId>,
    woken: bool,
    active_roots: &rustc_hash::FxHashSet<WidgetId>,
) {
    let Some(frame) = open_frame(ctx) else {
        return;
    };

    let root = ctx.surface.widget_id;
    dispatch_events(&frame.events, root, ctx.tree, active_roots);
    sync_platform_state(ctx.wayland_state, ctx.qh);

    let geometry = resolve_geometry(ctx, &frame);

    // Frame pacing: while a `wl_surface.frame` callback is in flight the
    // compositor has not shown the previous frame, so another one would only
    // queue behind it.
    //
    // This returns BEFORE draining jobs, and that is the whole point:
    // animation continuations have to stay queued. Advancing them on every
    // loop iteration would re-ping the wakeup each time and spin the loop
    // flat out between frame callbacks. Input was dispatched above, so it is
    // never delayed by the gate; initialisation and resizes bypass it.
    if frame.frame_callback_pending
        && !frame.force_render_surface
        && !geometry.needs_resize
        && !geometry.scale_changed
    {
        render_stats::record_frame_skipped();
        return;
    }

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
        return;
    }

    layout_pass(ctx, &frame, &geometry, has_pending_layouts, layout_roots);
    paint_and_present(ctx, &frame, &geometry);
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
        // Create root owner scope — all signals/effects created in setup are owned
        self.root_owner_id = Some(reactive::create_root_owner());
        // Under the root owner, so it outlives every component scope.
        reactive::init_focus();
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

        // The loop is created before the connection because the keyboard is
        // built during the first dispatch, and it needs the loop handle to arm
        // its key-repeat timer.
        let mut event_loop: EventLoop<platform::WaylandState> =
            EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();

        let (connection, mut event_queue, mut wayland_state, qh) =
            match create_wayland_app(loop_handle.clone()) {
                Ok(app) => app,
                Err(e) => {
                    log::error!("Cannot start: {e}");
                    return ExitReason::Error(e);
                }
            };

        // Create surfaces from add_surface() calls
        for def in &self.surface_definitions {
            wayland_state.create_surface_with_id(&qh, def.id, &def.config);
        }

        // Wait for all surfaces to configure
        while !wayland_state.all_surfaces_configured() && !wayland_state.exit {
            if let Err(e) = event_queue.blocking_dispatch(&mut wayland_state) {
                log::error!("Wayland dispatch failed during configure: {e}");
                return ExitReason::Error(platform::PlatformError::ConnectionLost);
            }
        }

        if wayland_state.exit {
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
                &connection,
                &wayland_surface.wl_surface,
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
                    wgpu_surface.device.clone(),
                    wgpu_surface.queue.clone(),
                    wgpu_surface.config.format,
                );
                renderer = Some(r);
            }

            surface_manager.add(managed);
        }

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

        // Insert Wayland source - this handles all Wayland protocol events
        WaylandSource::new(connection.clone(), event_queue)
            .insert(loop_handle.clone())
            .expect("Failed to insert Wayland source");

        // Main loop - event-driven, blocks until Wayland event or signal update
        loop {
            // Check if all surfaces are fully initialized
            let any_surface_needs_init = wayland_state.any_surface_needs_render();
            let force_render = any_surface_needs_init;

            // Anything whose deadline has arrived becomes ordinary pending work.
            jobs::promote_due_jobs();

            // Check if we need to actively poll: jobs pushed during the
            // previous frame, or a wake request that landed after this
            // iteration consumed the flag (blocking on a set flag would
            // suppress all later pings — see wake_request_pending).
            let has_pending = has_pending_jobs();
            let needs_polling = has_pending || force_render || jobs::wake_request_pending();

            // Dispatch events from calloop:
            // - If polling needed (animations/callbacks/init), use timeout
            // - If something is due later (a blinking caret), sleep until then
            // - Otherwise block until event (Wayland or ping wakeup)
            let timeout = if needs_polling {
                Some(std::time::Duration::from_millis(16)) // ~60fps for animations
            } else if let Some(deadline) = jobs::next_deadline() {
                // Not polling — waiting. A caret blinks twice a second, so this
                // sleeps ~530 ms and wakes once, where treating the schedule as
                // pending work would spin at 60 fps to repaint nothing 113 times
                // out of 114.
                Some(deadline.saturating_duration_since(std::time::Instant::now()))
            } else {
                // About to block with nothing left to wake us but the
                // compositor. Every queue must be empty by now — see
                // `queued_but_unwoken`.
                debug_assert!(
                    queued_but_unwoken().is_none(),
                    "the loop is about to block indefinitely with {} still queued — \
                     whatever produced it owes a wakeup. See the Event Loop Wakeup \
                     Contract in docs/ARCHITECTURE.md.",
                    queued_but_unwoken().unwrap_or_default()
                );
                None // Block indefinitely until event
            };

            if let Err(e) = event_loop.dispatch(timeout, &mut wayland_state) {
                // The connection died (compositor exited, protocol error).
                // Exit cleanly instead of panicking the process.
                log::error!("Event loop dispatch failed: {e}");
                return ExitReason::Error(platform::PlatformError::ConnectionLost);
            }

            // Reset ping coalescing: the first wake_loop from here on
            // sends a fresh ping so the next dispatch can't block on
            // work queued during this iteration.
            jobs::mark_loop_awake();

            // Check for programmatic exit/restart requests
            match get_exit_request() {
                jobs::ExitRequest::Quit => break,
                jobs::ExitRequest::Restart => return ExitReason::Restart,
                jobs::ExitRequest::Running => {}
            }

            if wayland_state.exit {
                break;
            }

            // Drive the session-lock state machine (lock/unlock requests,
            // grant/denial events, per-output lock surfaces)
            session_lock::process_session_lock(
                &mut surface_manager,
                &mut wayland_state,
                &qh,
                &mut self.tree,
            );

            // Run deferred owner disposals (public dispose_owner). Safe
            // here: no user closure is on the stack.
            reactive::owner::flush_pending_disposals();

            // Process dynamic surface commands
            if !process_surface_commands(
                &mut surface_manager,
                &mut wayland_state,
                &qh,
                &mut self.tree,
            ) {
                break;
            }

            // Initialize GPU for any pending surfaces (newly created dynamic surfaces)
            surface_manager.init_pending_gpu(
                &gpu_context,
                &connection,
                &wayland_state,
                &mut self.tree,
            );

            // Lazily create the shared renderer once the first surface has a
            // GPU state (apps may start with zero surfaces and spawn them
            // dynamically, e.g. one bar per output).
            if renderer.is_none()
                && let Some(wgpu_surface) = surface_manager.first_gpu_surface()
            {
                renderer = Some(Renderer::new(
                    wgpu_surface.device.clone(),
                    wgpu_surface.queue.clone(),
                    wgpu_surface.config.format,
                ));
            }

            // Flush background-thread signal writes once per frame (queued via WriteSignal).
            // Must run before take_wake_request() so that signal changes from bg writes
            // are processed into jobs before we check the wake request.
            reactive::flush_bg_writes();

            // Take the wake request once for all surfaces (not per-surface)
            let woken = take_wake_request();

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
                jobs::distribute_jobs(&self.tree, &active_roots);
                let orphans = jobs::drain_orphan_jobs();
                if !orphans.is_empty() {
                    let mut scratch_layout_roots = Vec::new();
                    process_jobs(&orphans, &mut self.tree, &mut scratch_layout_roots);
                }
                jobs::recycle_job_buffer(orphans);

                // Drop layout roots of surfaces that no longer exist.
                self.layout_roots
                    .retain(|root, _| active_roots.contains(root));

                for id in surface_ids {
                    let Some(surface) = surface_manager.get_mut(id) else {
                        continue;
                    };
                    let layout_roots = self.layout_roots.entry(surface.widget_id).or_default();
                    let mut ctx = FrameContext {
                        id,
                        surface,
                        wayland_state: &mut wayland_state,
                        renderer,
                        tree: &mut self.tree,
                        qh: &qh,
                    };
                    render_surface(&mut ctx, layout_roots, woken, &active_roots);
                }
            }

            // Flush the connection once for all surfaces
            if let Err(e) = connection.flush() {
                log::error!("Wayland connection flush failed: {e}");
                return ExitReason::Error(platform::PlatformError::ConnectionLost);
            }
        }

        ExitReason::Quit
    }
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
        outputs::reset_outputs();
        compositor::reset_compositor_effects();
        keyboard::reset_keyboard_modifiers();
        blur::reset_blur();
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
mod wakeup_contract_coverage {
    use super::*;

    /// A probe that never reports anything would make the check in
    /// `queued_but_unwoken` silently useless, so each one is pinned to the
    /// drain it speaks for: queue, see it, drain, see it gone.
    #[test]
    fn each_probe_tracks_its_own_queue() {
        // Clipboard and primary share one probe.
        let _ = reactive::take_clipboard_change();
        let _ = reactive::take_primary_change();
        assert!(!reactive::selection_change_pending());

        reactive::clipboard_copy("queued");
        assert!(
            reactive::selection_change_pending(),
            "a copy must be visible to the wakeup check"
        );
        let _ = reactive::take_clipboard_change();
        assert!(!reactive::selection_change_pending());

        reactive::primary_copy("queued");
        assert!(
            reactive::selection_change_pending(),
            "a primary-selection copy must be visible too"
        );
        let _ = reactive::take_primary_change();
        assert!(!reactive::selection_change_pending());

        // Cursor.
        let _ = reactive::take_cursor_change();
        assert!(!reactive::cursor_change_pending());
        reactive::set_cursor(reactive::CursorIcon::Text);
        assert!(
            reactive::cursor_change_pending(),
            "a cursor change must be visible to the wakeup check"
        );
        let _ = reactive::take_cursor_change();
        assert!(!reactive::cursor_change_pending());
    }
}
