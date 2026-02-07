//! Render statistics tracking for debugging and performance analysis.
//!
//! Enable render stats by compiling with the `render-stats` feature:
//! ```bash
//! cargo run --example render_stats_test --features render-stats
//! ```
//!
//! Stats are printed every second when enabled, showing:
//! - Frame counts (painted vs skipped)
//! - Layout calls, skip rate, and execution reasons
//! - Paint child cache hits/misses
//! - Flatten cache hits/misses
//! - Damage region distribution

/// Reasons why a layout was executed (can be multiple).
/// Note: Animations and property changes flow through the reactive system via mark_needs_layout(),
/// so animation-triggered and signal-triggered layouts appear under reactive_changed.
#[derive(Default, Clone, Copy)]
pub struct LayoutReasons {
    pub constraints_changed: bool,
    pub reactive_changed: bool,
}

/// Snapshot of accumulated render statistics.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StatsSnapshot {
    pub frames_painted: u64,
    pub frames_skipped: u64,
    pub layout_total_calls: u64,
    pub layout_skipped: u64,
    pub layout_executed: u64,
    pub layout_primary_constraints: u64,
    pub layout_primary_reactive: u64,
    pub paint_children_cached: u64,
    pub paint_children_painted: u64,
    pub flatten_nodes_cached: u64,
    pub flatten_nodes_flattened: u64,
    pub damage_none: u64,
    pub damage_partial: u64,
    pub damage_full: u64,
}

#[cfg(feature = "render-stats")]
mod inner {
    use super::LayoutReasons;
    use crate::tree::DamageRegion;
    use std::cell::RefCell;
    use std::time::Instant;

    thread_local! {
        static STATS: RefCell<RenderStats> = RefCell::new(RenderStats::new());
    }

    struct RenderStats {
        // Layout
        layout_total_calls: u64,
        layout_skipped: u64,
        layout_executed: u64,
        layout_primary_constraints: u64,
        layout_primary_reactive: u64,
        // Frame-level
        frames_painted: u64,
        frames_skipped: u64,
        // Paint child cache
        paint_children_cached: u64,
        paint_children_painted: u64,
        // Flatten cache
        flatten_nodes_cached: u64,
        flatten_nodes_flattened: u64,
        // Damage regions
        damage_none: u64,
        damage_partial: u64,
        damage_full: u64,
        // Timing
        last_print: Instant,
    }

    impl RenderStats {
        fn new() -> Self {
            Self {
                layout_total_calls: 0,
                layout_skipped: 0,
                layout_executed: 0,
                layout_primary_constraints: 0,
                layout_primary_reactive: 0,
                frames_painted: 0,
                frames_skipped: 0,
                paint_children_cached: 0,
                paint_children_painted: 0,
                flatten_nodes_cached: 0,
                flatten_nodes_flattened: 0,
                damage_none: 0,
                damage_partial: 0,
                damage_full: 0,
                last_print: Instant::now(),
            }
        }

        fn reset(&mut self) {
            self.layout_total_calls = 0;
            self.layout_skipped = 0;
            self.layout_executed = 0;
            self.layout_primary_constraints = 0;
            self.layout_primary_reactive = 0;
            self.frames_painted = 0;
            self.frames_skipped = 0;
            self.paint_children_cached = 0;
            self.paint_children_painted = 0;
            self.flatten_nodes_cached = 0;
            self.flatten_nodes_flattened = 0;
            self.damage_none = 0;
            self.damage_partial = 0;
            self.damage_full = 0;
            self.last_print = Instant::now();
        }
    }

    /// Record a layout call that was skipped (cache hit).
    #[inline]
    pub fn record_layout_skipped() {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.layout_total_calls += 1;
            stats.layout_skipped += 1;
        });
    }

    /// Record a layout call that was executed (cache miss) with reasons.
    #[inline]
    pub fn record_layout_executed_with_reasons(reasons: LayoutReasons) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.layout_total_calls += 1;
            stats.layout_executed += 1;

            if reasons.constraints_changed {
                stats.layout_primary_constraints += 1;
            } else if reasons.reactive_changed {
                stats.layout_primary_reactive += 1;
            }
        });
    }

    /// Record a frame that was fully painted.
    #[inline]
    pub fn record_frame_painted() {
        STATS.with(|s| {
            s.borrow_mut().frames_painted += 1;
        });
    }

    /// Record a frame that was skipped (nothing needed paint).
    #[inline]
    pub fn record_frame_skipped() {
        STATS.with(|s| {
            s.borrow_mut().frames_skipped += 1;
        });
    }

    /// Record a child that reused its cached paint result.
    #[inline]
    pub fn record_paint_child_cached() {
        STATS.with(|s| {
            s.borrow_mut().paint_children_cached += 1;
        });
    }

    /// Record a child that was fully repainted.
    #[inline]
    pub fn record_paint_child_painted() {
        STATS.with(|s| {
            s.borrow_mut().paint_children_painted += 1;
        });
    }

    /// Record a flatten node that reused cached commands.
    #[inline]
    pub fn record_flatten_cached() {
        STATS.with(|s| {
            s.borrow_mut().flatten_nodes_cached += 1;
        });
    }

    /// Record a flatten node that was fully flattened.
    #[inline]
    pub fn record_flatten_full() {
        STATS.with(|s| {
            s.borrow_mut().flatten_nodes_flattened += 1;
        });
    }

    /// Return a snapshot of the current stats (for testing).
    pub fn get_stats() -> super::StatsSnapshot {
        STATS.with(|s| {
            let stats = s.borrow();
            super::StatsSnapshot {
                frames_painted: stats.frames_painted,
                frames_skipped: stats.frames_skipped,
                layout_total_calls: stats.layout_total_calls,
                layout_skipped: stats.layout_skipped,
                layout_executed: stats.layout_executed,
                layout_primary_constraints: stats.layout_primary_constraints,
                layout_primary_reactive: stats.layout_primary_reactive,
                paint_children_cached: stats.paint_children_cached,
                paint_children_painted: stats.paint_children_painted,
                flatten_nodes_cached: stats.flatten_nodes_cached,
                flatten_nodes_flattened: stats.flatten_nodes_flattened,
                damage_none: stats.damage_none,
                damage_partial: stats.damage_partial,
                damage_full: stats.damage_full,
            }
        })
    }

    /// Reset all stats to zero (for test isolation).
    pub fn reset_stats() {
        STATS.with(|s| {
            s.borrow_mut().reset();
        });
    }

    /// Called at the end of each frame to potentially print stats.
    /// Accepts the damage region for this frame.
    pub fn end_frame(damage: &DamageRegion) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();

            match damage {
                DamageRegion::None => stats.damage_none += 1,
                DamageRegion::Partial(_) => stats.damage_partial += 1,
                DamageRegion::Full => stats.damage_full += 1,
            }

            let elapsed = stats.last_print.elapsed();
            if elapsed.as_secs() >= 1 {
                let total_frames = stats.frames_painted + stats.frames_skipped;

                let layout_skip_rate = if stats.layout_total_calls > 0 {
                    (stats.layout_skipped as f64 / stats.layout_total_calls as f64) * 100.0
                } else {
                    0.0
                };

                let paint_total = stats.paint_children_cached + stats.paint_children_painted;
                let paint_cache_rate = if paint_total > 0 {
                    (stats.paint_children_cached as f64 / paint_total as f64) * 100.0
                } else {
                    0.0
                };

                let flatten_total = stats.flatten_nodes_cached + stats.flatten_nodes_flattened;
                let flatten_cache_rate = if flatten_total > 0 {
                    (stats.flatten_nodes_cached as f64 / flatten_total as f64) * 100.0
                } else {
                    0.0
                };

                eprintln!(
                    "[Render Stats] frames={} painted={} skipped={}",
                    total_frames, stats.frames_painted, stats.frames_skipped
                );
                eprintln!(
                    "  layout: calls={} skipped={} executed={} skip_rate={:.1}%",
                    stats.layout_total_calls,
                    stats.layout_skipped,
                    stats.layout_executed,
                    layout_skip_rate
                );
                if stats.layout_executed > 0 {
                    eprintln!(
                        "    primary: constraints={} reactive={}",
                        stats.layout_primary_constraints, stats.layout_primary_reactive
                    );
                }
                eprintln!(
                    "  paint: children={} cached={} painted={} cache_rate={:.1}%",
                    paint_total,
                    stats.paint_children_cached,
                    stats.paint_children_painted,
                    paint_cache_rate
                );
                eprintln!(
                    "  flatten: nodes={} cached={} flattened={} cache_rate={:.1}%",
                    flatten_total,
                    stats.flatten_nodes_cached,
                    stats.flatten_nodes_flattened,
                    flatten_cache_rate
                );
                eprintln!(
                    "  damage: none={} partial={} full={}",
                    stats.damage_none, stats.damage_partial, stats.damage_full
                );

                stats.reset();
            }
        });
    }
}

#[cfg(feature = "render-stats")]
pub use inner::*;

// No-op implementations when feature is disabled - these get completely inlined away

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn get_stats() -> StatsSnapshot {
    StatsSnapshot::default()
}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn reset_stats() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_layout_skipped() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_layout_executed_with_reasons(_reasons: LayoutReasons) {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_frame_painted() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_frame_skipped() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_paint_child_cached() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_paint_child_painted() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_flatten_cached() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_flatten_full() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn end_frame(_damage: &crate::tree::DamageRegion) {}

#[cfg(test)]
#[cfg(feature = "render-stats")]
mod tests {
    use super::*;
    use crate::tree::DamageRegion;
    use crate::widgets::Rect;

    /// Reset stats before each test to ensure isolation
    /// (tests share the thread-local when run on the same thread).
    fn setup() {
        reset_stats();
    }

    #[test]
    fn test_frame_painted_counter() {
        setup();
        record_frame_painted();
        record_frame_painted();
        record_frame_painted();
        let s = get_stats();
        assert_eq!(s.frames_painted, 3);
        assert_eq!(s.frames_skipped, 0);
    }

    #[test]
    fn test_frame_skipped_counter() {
        setup();
        record_frame_skipped();
        record_frame_skipped();
        let s = get_stats();
        assert_eq!(s.frames_skipped, 2);
        assert_eq!(s.frames_painted, 0);
    }

    #[test]
    fn test_layout_skipped() {
        setup();
        record_layout_skipped();
        record_layout_skipped();
        record_layout_skipped();
        let s = get_stats();
        assert_eq!(s.layout_total_calls, 3);
        assert_eq!(s.layout_skipped, 3);
        assert_eq!(s.layout_executed, 0);
    }

    #[test]
    fn test_layout_executed_constraints_changed() {
        setup();
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: true,
            reactive_changed: false,
        });
        let s = get_stats();
        assert_eq!(s.layout_total_calls, 1);
        assert_eq!(s.layout_executed, 1);
        assert_eq!(s.layout_primary_constraints, 1);
        assert_eq!(s.layout_primary_reactive, 0);
    }

    #[test]
    fn test_layout_executed_reactive_changed() {
        setup();
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: false,
            reactive_changed: true,
        });
        let s = get_stats();
        assert_eq!(s.layout_executed, 1);
        assert_eq!(s.layout_primary_reactive, 1);
        assert_eq!(s.layout_primary_constraints, 0);
    }

    #[test]
    fn test_layout_constraints_takes_priority_over_reactive() {
        setup();
        // When both are true, constraints_changed is the primary reason
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: true,
            reactive_changed: true,
        });
        let s = get_stats();
        assert_eq!(s.layout_primary_constraints, 1);
        assert_eq!(s.layout_primary_reactive, 0);
    }

    #[test]
    fn test_paint_child_counters() {
        setup();
        record_paint_child_cached();
        record_paint_child_cached();
        record_paint_child_cached();
        record_paint_child_painted();
        let s = get_stats();
        assert_eq!(s.paint_children_cached, 3);
        assert_eq!(s.paint_children_painted, 1);
    }

    #[test]
    fn test_flatten_counters() {
        setup();
        record_flatten_cached();
        record_flatten_full();
        record_flatten_full();
        let s = get_stats();
        assert_eq!(s.flatten_nodes_cached, 1);
        assert_eq!(s.flatten_nodes_flattened, 2);
    }

    #[test]
    fn test_damage_region_tracking() {
        setup();
        end_frame(&DamageRegion::None);
        end_frame(&DamageRegion::None);
        end_frame(&DamageRegion::Partial(Rect::new(0.0, 0.0, 100.0, 50.0)));
        end_frame(&DamageRegion::Full);
        let s = get_stats();
        assert_eq!(s.damage_none, 2);
        assert_eq!(s.damage_partial, 1);
        assert_eq!(s.damage_full, 1);
    }

    #[test]
    fn test_mixed_layout_skip_rate() {
        setup();
        // 3 skipped, 2 executed = 60% skip rate
        record_layout_skipped();
        record_layout_skipped();
        record_layout_skipped();
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: true,
            reactive_changed: false,
        });
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: false,
            reactive_changed: true,
        });
        let s = get_stats();
        assert_eq!(s.layout_total_calls, 5);
        assert_eq!(s.layout_skipped, 3);
        assert_eq!(s.layout_executed, 2);
        assert_eq!(s.layout_primary_constraints, 1);
        assert_eq!(s.layout_primary_reactive, 1);
    }

    #[test]
    fn test_reset_clears_all_counters() {
        setup();
        record_frame_painted();
        record_frame_skipped();
        record_layout_skipped();
        record_layout_executed_with_reasons(LayoutReasons {
            constraints_changed: true,
            reactive_changed: false,
        });
        record_paint_child_cached();
        record_paint_child_painted();
        record_flatten_cached();
        record_flatten_full();
        end_frame(&DamageRegion::Full);

        // Verify something was recorded
        let s = get_stats();
        assert_ne!(s, StatsSnapshot::default());

        // Reset and verify all zeros
        reset_stats();
        let s = get_stats();
        assert_eq!(s.frames_painted, 0);
        assert_eq!(s.frames_skipped, 0);
        assert_eq!(s.layout_total_calls, 0);
        assert_eq!(s.layout_skipped, 0);
        assert_eq!(s.layout_executed, 0);
        assert_eq!(s.paint_children_cached, 0);
        assert_eq!(s.paint_children_painted, 0);
        assert_eq!(s.flatten_nodes_cached, 0);
        assert_eq!(s.flatten_nodes_flattened, 0);
        assert_eq!(s.damage_none, 0);
        assert_eq!(s.damage_partial, 0);
        assert_eq!(s.damage_full, 0);
    }
}
