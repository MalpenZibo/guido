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
