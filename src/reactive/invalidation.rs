use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bitflags::bitflags;
use smithay_client_toolkit::reexports::calloop::ping::Ping;

bitflags! {
    /// Flags indicating what aspects of rendering need to be updated
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ChangeFlags: u8 {
        /// Widget needs layout recalculation (size/position may change)
        const NEEDS_LAYOUT = 0b01;
        /// Widget needs repainting (visual appearance changed)
        const NEEDS_PAINT  = 0b10;
    }
}

/// Unique identifier for a widget in the tree
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WidgetId(u64);

static NEXT_WIDGET_ID: AtomicU64 = AtomicU64::new(1);

impl WidgetId {
    /// Generate a new unique widget ID
    pub fn next() -> Self {
        WidgetId(NEXT_WIDGET_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Request that this widget be re-laid out (and repainted)
    pub fn request_layout(&self) {
        APP_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.change_flags |= ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT;
            state.dirty_widgets.insert(*self);
        });
        request_frame();
    }

    /// Request that this widget be repainted (without layout)
    pub fn request_paint(&self) {
        APP_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.change_flags |= ChangeFlags::NEEDS_PAINT;
            state.dirty_widgets.insert(*self);
        });
        request_frame();
    }
}

/// Application state for tracking what needs updating
pub struct AppState {
    /// Global change flags
    pub change_flags: ChangeFlags,
    /// Set of widgets that have changed
    pub dirty_widgets: HashSet<WidgetId>,
    /// Whether animations are currently active
    pub has_animations: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            change_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            dirty_widgets: HashSet::new(),
            has_animations: false,
        }
    }

    pub fn needs_layout(&self) -> bool {
        self.change_flags.contains(ChangeFlags::NEEDS_LAYOUT)
    }

    pub fn needs_paint(&self) -> bool {
        self.change_flags.contains(ChangeFlags::NEEDS_PAINT)
    }

    pub fn clear_layout_flag(&mut self) {
        self.change_flags.remove(ChangeFlags::NEEDS_LAYOUT);
    }

    pub fn clear_paint_flag(&mut self) {
        self.change_flags.remove(ChangeFlags::NEEDS_PAINT);
    }

    pub fn clear_dirty_widgets(&mut self) {
        self.dirty_widgets.clear();
    }
}

thread_local! {
    static APP_STATE: RefCell<AppState> = RefCell::new(AppState::new());
    static WIDGET_TREE: RefCell<WidgetTree> = RefCell::new(WidgetTree::new());
    /// Per-widget dirty flags for layout
    static WIDGET_DIRTY_FLAGS: RefCell<HashMap<WidgetId, bool>> = RefCell::new(HashMap::new());
    /// Registered relayout boundary check functions (widget_id -> is_boundary)
    static RELAYOUT_BOUNDARIES: RefCell<HashMap<WidgetId, bool>> = RefCell::new(HashMap::new());
}

/// Global widget tree for parent tracking and dirty propagation
pub struct WidgetTree {
    /// Map from widget ID to parent widget ID
    parents: HashMap<WidgetId, WidgetId>,
    /// Set of widgets that are roots for partial layout (relayout boundaries)
    layout_roots: HashSet<WidgetId>,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self {
            parents: HashMap::new(),
            layout_roots: HashSet::new(),
        }
    }

    /// Set the parent of a widget
    pub fn set_parent(&mut self, child: WidgetId, parent: WidgetId) {
        self.parents.insert(child, parent);
    }

    /// Get the parent of a widget
    pub fn get_parent(&self, widget: WidgetId) -> Option<WidgetId> {
        self.parents.get(&widget).copied()
    }

    /// Remove a widget from the tree (when it's dropped)
    pub fn remove(&mut self, widget: WidgetId) {
        self.parents.remove(&widget);
        self.layout_roots.remove(&widget);
    }

    /// Mark a widget as a layout root (needs layout, is a relayout boundary)
    pub fn add_layout_root(&mut self, widget: WidgetId) {
        self.layout_roots.insert(widget);
    }

    /// Take all layout roots (clears the set)
    pub fn take_layout_roots(&mut self) -> Vec<WidgetId> {
        self.layout_roots.drain().collect()
    }

    /// Check if a widget is a layout root
    pub fn is_layout_root(&self, widget: WidgetId) -> bool {
        self.layout_roots.contains(&widget)
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Set the parent of a widget in the global tree
pub fn set_widget_parent(child: WidgetId, parent: WidgetId) {
    WIDGET_TREE.with(|tree| {
        tree.borrow_mut().set_parent(child, parent);
    });
}

/// Get the parent of a widget from the global tree
pub fn get_widget_parent(widget: WidgetId) -> Option<WidgetId> {
    WIDGET_TREE.with(|tree| tree.borrow().get_parent(widget))
}

/// Remove a widget from the global tree
pub fn remove_widget_from_tree(widget: WidgetId) {
    WIDGET_TREE.with(|tree| {
        tree.borrow_mut().remove(widget);
    });
}

/// Add a widget to the layout roots set
pub fn add_layout_root(widget: WidgetId) {
    WIDGET_TREE.with(|tree| {
        tree.borrow_mut().add_layout_root(widget);
    });
}

/// Take all layout roots (for partial layout)
pub fn take_layout_roots() -> Vec<WidgetId> {
    WIDGET_TREE.with(|tree| tree.borrow_mut().take_layout_roots())
}

/// Set the needs_layout flag for a widget
pub fn set_needs_layout_flag(widget_id: WidgetId, value: bool) {
    WIDGET_DIRTY_FLAGS.with(|flags| {
        if value {
            flags.borrow_mut().insert(widget_id, true);
        } else {
            flags.borrow_mut().remove(&widget_id);
        }
    });
}

/// Get the needs_layout flag for a widget
pub fn get_needs_layout_flag(widget_id: WidgetId) -> bool {
    WIDGET_DIRTY_FLAGS.with(|flags| flags.borrow().get(&widget_id).copied().unwrap_or(false))
}

/// Clear all dirty flags (call at start of frame)
pub fn clear_all_dirty_flags() {
    WIDGET_DIRTY_FLAGS.with(|flags| flags.borrow_mut().clear());
}

/// Register whether a widget is a relayout boundary
pub fn register_relayout_boundary(widget_id: WidgetId, is_boundary: bool) {
    RELAYOUT_BOUNDARIES.with(|boundaries| {
        boundaries.borrow_mut().insert(widget_id, is_boundary);
    });
}

/// Check if a widget is a relayout boundary
fn is_relayout_boundary(widget_id: WidgetId) -> bool {
    RELAYOUT_BOUNDARIES.with(|boundaries| {
        boundaries
            .borrow()
            .get(&widget_id)
            .copied()
            .unwrap_or(false)
    })
}

/// Unregister a widget's relayout boundary status (on drop)
pub fn unregister_relayout_boundary(widget_id: WidgetId) {
    RELAYOUT_BOUNDARIES.with(|boundaries| {
        boundaries.borrow_mut().remove(&widget_id);
    });
    WIDGET_DIRTY_FLAGS.with(|flags| {
        flags.borrow_mut().remove(&widget_id);
    });
}

// Layout tracking context for building widget -> signal dependencies
thread_local! {
    /// Current widget being laid out (for dependency tracking)
    static CURRENT_LAYOUT_WIDGET: RefCell<Option<WidgetId>> = const { RefCell::new(None) };
    /// Map from signal ID to set of widgets that depend on it for layout
    static LAYOUT_SUBSCRIBERS: RefCell<HashMap<usize, HashSet<WidgetId>>> = RefCell::new(HashMap::new());
}

/// Start layout tracking for a widget.
/// While tracking is active, any signal reads will be recorded as layout dependencies.
pub fn start_layout_tracking(widget_id: WidgetId) {
    CURRENT_LAYOUT_WIDGET.with(|current| {
        *current.borrow_mut() = Some(widget_id);
    });
}

/// Finish layout tracking and clear the current widget.
pub fn finish_layout_tracking() {
    CURRENT_LAYOUT_WIDGET.with(|current| {
        *current.borrow_mut() = None;
    });
}

/// Record that the current layout widget (if any) depends on the given signal.
/// Called from Signal::get() when layout tracking is active.
pub fn record_layout_read(signal_id: usize) {
    CURRENT_LAYOUT_WIDGET.with(|current| {
        if let Some(widget_id) = *current.borrow() {
            LAYOUT_SUBSCRIBERS.with(|subs| {
                subs.borrow_mut()
                    .entry(signal_id)
                    .or_default()
                    .insert(widget_id);
            });
        }
    });
}

/// Notify all layout subscribers of a signal that it has changed.
/// Called from Signal::set() and Signal::update().
pub fn notify_layout_subscribers(signal_id: usize) {
    // Collect widget IDs first to avoid holding the borrow during mark_needs_layout
    let widgets_to_mark: Vec<WidgetId> = LAYOUT_SUBSCRIBERS.with(|subs| {
        subs.borrow()
            .get(&signal_id)
            .map(|widgets| widgets.iter().copied().collect())
            .unwrap_or_default()
    });

    // Mark each widget as needing layout and propagate up to ancestors
    for widget_id in widgets_to_mark {
        mark_needs_layout(widget_id);
    }
}

/// Clear layout subscribers for a specific signal (when signal is disposed)
pub fn clear_layout_subscribers(signal_id: usize) {
    LAYOUT_SUBSCRIBERS.with(|subs| {
        subs.borrow_mut().remove(&signal_id);
    });
}

/// Mark a widget as needing layout.
/// Bubbles up through parents, stopping at relayout boundaries.
/// Adds the stopping point to layout_roots for partial re-layout.
pub fn mark_needs_layout(widget_id: WidgetId) {
    WIDGET_TREE.with(|tree| {
        let tree = tree.borrow();
        let mut current = widget_id;

        loop {
            // Mark this widget as needing layout
            WIDGET_DIRTY_FLAGS.with(|flags| {
                flags.borrow_mut().insert(current, true);
            });

            // If this widget is a relayout boundary, it's a layout root
            if is_relayout_boundary(current) {
                drop(tree);
                WIDGET_TREE.with(|t| {
                    t.borrow_mut().add_layout_root(current);
                });
                break;
            }

            // Otherwise, bubble up to parent
            match tree.get_parent(current) {
                Some(parent) => current = parent,
                None => {
                    // Reached root, add it as layout root
                    drop(tree);
                    WIDGET_TREE.with(|t| {
                        t.borrow_mut().add_layout_root(current);
                    });
                    break;
                }
            }
        }
    });

    // Also set global layout flag
    APP_STATE.with(|state| {
        state.borrow_mut().change_flags |= ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT;
    });

    request_frame();
}

/// Global flag to indicate a frame is requested
static FRAME_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Global wakeup handle for signaling the event loop
static WAKEUP_PING: OnceLock<Ping> = OnceLock::new();

/// Initialize the wakeup mechanism (called from App::run())
pub fn init_wakeup(ping: Ping) {
    let _ = WAKEUP_PING.set(ping);
}

/// Request that the main event loop process a frame
pub fn request_frame() {
    // Only ping on first request - avoids redundant syscalls when multiple signals update
    let was_requested = FRAME_REQUESTED.swap(true, Ordering::Relaxed);
    if !was_requested {
        // Wake up the event loop immediately
        if let Some(ping) = WAKEUP_PING.get() {
            ping.ping();
        }
    }
}

/// Check if a frame has been requested and clear the flag
pub fn take_frame_request() -> bool {
    FRAME_REQUESTED.swap(false, Ordering::Relaxed)
}

/// Request a frame for animation purposes
pub fn request_animation_frame() {
    APP_STATE.with(|state| {
        state.borrow_mut().has_animations = true;
    });
    request_frame();
}

/// Clear the animation flag (call after animation completes)
pub fn clear_animation_flag() {
    APP_STATE.with(|state| {
        state.borrow_mut().has_animations = false;
    });
}

/// Check if animations are active
pub fn has_animations() -> bool {
    APP_STATE.with(|state| state.borrow().has_animations)
}

/// Access the app state for rendering decisions
pub fn with_app_state<F, R>(f: F) -> R
where
    F: FnOnce(&AppState) -> R,
{
    APP_STATE.with(|state| f(&state.borrow()))
}

/// Mutably access the app state
pub fn with_app_state_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut AppState) -> R,
{
    APP_STATE.with(|state| f(&mut state.borrow_mut()))
}

/// Mark that layout is needed (global)
pub fn request_layout() {
    APP_STATE.with(|state| {
        state.borrow_mut().change_flags |= ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT;
    });
    request_frame();
}

/// Mark that paint is needed (global)
pub fn request_paint() {
    APP_STATE.with(|state| {
        state.borrow_mut().change_flags |= ChangeFlags::NEEDS_PAINT;
    });
    request_frame();
}
