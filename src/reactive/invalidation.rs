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

    /// Get the raw u64 value of this widget ID
    pub fn as_u64(self) -> u64 {
        self.0
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
}

// Layout tracking context for building widget -> signal dependencies
thread_local! {
    /// Stack of widgets being laid out (for dependency tracking during nested layouts)
    static LAYOUT_WIDGET_STACK: RefCell<Vec<WidgetId>> = const { RefCell::new(Vec::new()) };
    /// Map from signal ID to set of widgets that depend on it for layout
    static LAYOUT_SUBSCRIBERS: RefCell<HashMap<usize, HashSet<WidgetId>>> = RefCell::new(HashMap::new());
}

/// Start layout tracking for a widget.
/// Pushes the widget onto the tracking stack. Signal reads during layout will be
/// recorded as dependencies of the topmost widget on the stack.
pub fn start_layout_tracking(widget_id: WidgetId) {
    LAYOUT_WIDGET_STACK.with(|stack| {
        stack.borrow_mut().push(widget_id);
    });
}

/// Finish layout tracking for the current widget.
/// Pops the widget from the tracking stack, restoring the parent's tracking context.
pub fn finish_layout_tracking() {
    LAYOUT_WIDGET_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

/// Record that the current layout widget (if any) depends on the given signal.
/// Called from Signal::get() when layout tracking is active.
pub fn record_layout_read(signal_id: usize) {
    LAYOUT_WIDGET_STACK.with(|stack| {
        if let Some(&widget_id) = stack.borrow().last() {
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
/// Bubbles up through parents until reaching a relayout boundary, which is
/// added to the layout queue. If no boundary is found, bubbles to root.
///
/// OPTIMIZATION: Stops early if a widget in the path is already dirty,
/// since the path to the boundary would already be marked.
pub fn mark_needs_layout(widget_id: WidgetId) {
    // Use the arena's mark_needs_layout which stops at boundaries
    super::layout_arena::arena_mark_needs_layout(widget_id);

    // Set NEEDS_PAINT so we repaint after layout changes
    // Note: We don't set NEEDS_LAYOUT because partial layout uses layout_roots
    APP_STATE.with(|state| {
        state.borrow_mut().change_flags |= ChangeFlags::NEEDS_PAINT;
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
