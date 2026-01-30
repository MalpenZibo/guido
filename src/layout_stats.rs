//! Layout statistics tracking for debugging and performance analysis.
//!
//! Enable layout stats by compiling with the `layout-stats` feature:
//! ```bash
//! cargo run --example scroll_mixed_content --features layout-stats
//! ```
//!
//! Stats are printed every second when enabled, showing:
//! - Total layout calls
//! - Layouts skipped (cache hit)
//! - Layouts executed (cache miss)
//! - Skip rate percentage
//! - Breakdown of why layouts were executed

/// Reasons why a layout was executed (can be multiple).
/// Note: Animations and property changes flow through the reactive system via mark_needs_layout(),
/// so animation-triggered and signal-triggered layouts appear under reactive_changed.
#[derive(Default, Clone, Copy)]
pub struct LayoutReasons {
    pub constraints_changed: bool,
    pub reactive_changed: bool,
}

#[cfg(feature = "layout-stats")]
mod inner {
    use super::LayoutReasons;
    use std::cell::RefCell;
    use std::time::Instant;

    thread_local! {
        static STATS: RefCell<LayoutStats> = RefCell::new(LayoutStats::new());
    }

    struct LayoutStats {
        /// Total layout() calls
        total_calls: u64,
        /// Layouts that were skipped (cache hit)
        skipped: u64,
        /// Layouts that were executed (cache miss)
        executed: u64,
        /// Primary (first) reason - mutually exclusive
        primary_constraints: u64,
        primary_reactive: u64,
        /// Last time stats were printed
        last_print: Instant,
        /// Frame counter
        frames: u64,
    }

    impl LayoutStats {
        fn new() -> Self {
            Self {
                total_calls: 0,
                skipped: 0,
                executed: 0,
                primary_constraints: 0,
                primary_reactive: 0,
                last_print: Instant::now(),
                frames: 0,
            }
        }

        fn reset(&mut self) {
            self.total_calls = 0;
            self.skipped = 0;
            self.executed = 0;
            self.primary_constraints = 0;
            self.primary_reactive = 0;
            self.frames = 0;
            self.last_print = Instant::now();
        }
    }

    /// Record a layout call that was skipped (cache hit).
    #[inline]
    pub fn record_layout_skipped() {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.total_calls += 1;
            stats.skipped += 1;
        });
    }

    /// Record a layout call that was executed (cache miss) with reasons.
    /// Tracks the primary (first) reason.
    #[inline]
    pub fn record_layout_executed_with_reasons(reasons: LayoutReasons) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.total_calls += 1;
            stats.executed += 1;

            // Track primary (first) reason - order matches evaluation in Container::layout
            // Note: Animation and signal-triggered layouts appear under reactive_changed
            if reasons.constraints_changed {
                stats.primary_constraints += 1;
            } else if reasons.reactive_changed {
                stats.primary_reactive += 1;
            }
        });
    }

    /// Called at the end of each frame to potentially print stats.
    /// Prints stats every second when enabled.
    pub fn end_frame() {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.frames += 1;

            let elapsed = stats.last_print.elapsed();
            if elapsed.as_secs() >= 1 {
                let skip_rate = if stats.total_calls > 0 {
                    (stats.skipped as f64 / stats.total_calls as f64) * 100.0
                } else {
                    0.0
                };

                eprintln!(
                    "[Layout Stats] frames={} calls={} skipped={} executed={} skip_rate={:.1}%",
                    stats.frames, stats.total_calls, stats.skipped, stats.executed, skip_rate
                );
                if stats.executed > 0 {
                    eprintln!(
                        "  primary: constraints={} reactive={}",
                        stats.primary_constraints, stats.primary_reactive
                    );
                }

                stats.reset();
            }
        });
    }
}

#[cfg(feature = "layout-stats")]
pub use inner::*;

// No-op implementations when feature is disabled - these get completely inlined away
#[cfg(not(feature = "layout-stats"))]
#[inline(always)]
pub fn record_layout_skipped() {}

#[cfg(not(feature = "layout-stats"))]
#[inline(always)]
pub fn record_layout_executed_with_reasons(_reasons: LayoutReasons) {}

#[cfg(not(feature = "layout-stats"))]
#[inline(always)]
pub fn end_frame() {}
