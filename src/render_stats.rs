//! Render statistics tracking for debugging and performance analysis.
//!
//! Enable render stats by compiling with the `render-stats` feature:
//! ```bash
//! cargo run --example render_stats_test --features render-stats
//! ```
//!
//! Every pass accumulates into a thread-local; `report_if_due` is what prints
//! and clears, once a second, and only the driver calls it — `App::run` does,
//! a harness stepping the same loop does not. A consumer that wants one
//! frame's numbers scopes them with `reset_stats` and reads `get_stats`.
//!
//! The report shows:
//! - Frame counts (painted vs skipped)
//! - Layout calls, skip rate, and execution reasons
//! - Paint child cache hits/misses
//! - Flatten cache hits/misses
//! - Damage region distribution
//! - Per-phase timing (paint, flatten, GPU render, cache)
//! - What the frames asked of the allocator, when the binary installed
//!   [`crate::heap::CountingAllocator`]

/// Reasons why a layout was executed (can be multiple).
/// Note: Animations and property changes flow through the reactive system via mark_needs_layout(),
/// so animation-triggered and signal-triggered layouts appear under reactive_changed.
#[derive(Default, Clone, Copy)]
pub struct LayoutReasons {
    pub constraints_changed: bool,
    pub reactive_changed: bool,
}

/// Render pipeline phase for timing measurements.
#[derive(Debug, Clone, Copy)]
pub enum Phase {
    Paint,
    Flatten,
    GpuRender,
    CachePaintResults,
}

/// Per-phase timing statistics (microseconds).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PhaseTiming {
    pub avg_us: f64,
    pub min_us: f64,
    pub max_us: f64,
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
    pub paint_children_culled: u64,
    pub flatten_nodes_cached: u64,
    pub flatten_nodes_flattened: u64,
    pub damage_none: u64,
    pub damage_partial: u64,
    pub damage_full: u64,
    // Timing
    pub paint_timing: PhaseTiming,
    pub flatten_timing: PhaseTiming,
    pub gpu_render_timing: PhaseTiming,
    pub cache_paint_timing: PhaseTiming,
    // The paint window: children offered to it, children it let through, and
    // the ones it could not narrow because nothing ordered them.
    pub window_children_total: u64,
    pub window_children_iterated: u64,
    pub window_declined_children: u64,
    pub window_declined_containers: u64,
    /// How many times these frames went to the allocator. Zero unless the
    /// binary installed [`crate::heap::CountingAllocator`]: the library may
    /// not.
    ///
    /// Sampled at [`reset_stats`] and at each [`end_frame`], so a consumer
    /// that scopes one frame is told what that whole frame asked for and not
    /// only what the renderer's part of it did — dispatching the events that
    /// provoked a frame allocates too, and a change that moved its cost from
    /// paint into input would otherwise read as free.
    pub allocations: u64,
    /// The bytes behind [`allocations`](Self::allocations). A reallocation
    /// asks for the difference between the old size and the new.
    pub bytes_allocated: u64,
}

/// Zero-cost timing macro. Wraps a block with `Instant::now()` / `.elapsed()`
/// when `render-stats` is enabled; expands to just the body when disabled.
#[cfg(feature = "render-stats")]
#[macro_export]
macro_rules! time_phase {
    ($phase:expr, $body:expr) => {{
        let _t = std::time::Instant::now();
        let result = $body;
        $crate::render_stats::record_phase_duration($phase, _t.elapsed());
        result
    }};
}

#[cfg(not(feature = "render-stats"))]
#[macro_export]
macro_rules! time_phase {
    ($phase:expr, $body:expr) => {
        $body
    };
}

#[cfg(feature = "render-stats")]
mod inner {
    use super::{LayoutReasons, Phase, PhaseTiming};
    use crate::tree::DamageRegion;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    thread_local! {
        static STATS: RefCell<RenderStats> = RefCell::new(RenderStats::new());
    }

    /// Per-phase duration accumulator.
    struct PhaseAccum {
        total: Duration,
        min: Duration,
        max: Duration,
        count: u64,
    }

    impl PhaseAccum {
        fn new() -> Self {
            Self {
                total: Duration::ZERO,
                min: Duration::MAX,
                max: Duration::ZERO,
                count: 0,
            }
        }

        fn record(&mut self, d: Duration) {
            self.total += d;
            self.min = self.min.min(d);
            self.max = self.max.max(d);
            self.count += 1;
        }

        fn to_timing(&self) -> PhaseTiming {
            if self.count == 0 {
                return PhaseTiming::default();
            }
            PhaseTiming {
                avg_us: self.total.as_micros() as f64 / self.count as f64,
                min_us: self.min.as_micros() as f64,
                max_us: self.max.as_micros() as f64,
            }
        }
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
        paint_children_culled: u64,
        // Flatten cache
        flatten_nodes_cached: u64,
        flatten_nodes_flattened: u64,
        // Damage regions
        damage_none: u64,
        damage_partial: u64,
        damage_full: u64,
        // Phase timing
        paint_phase: PhaseAccum,
        flatten_phase: PhaseAccum,
        gpu_render_phase: PhaseAccum,
        cache_paint_phase: PhaseAccum,
        // Paint window
        window_children_total: u64,
        window_children_iterated: u64,
        window_declined_children: u64,
        window_declined_containers: u64,
        // The heap: what the frames asked for, and where the allocator's
        // process-wide totals stood when the last frame was folded in.
        allocations: u64,
        bytes_allocated: u64,
        heap_mark: (u64, u64),
        // Report timing
        last_print: Instant,
    }

    /// The allocator's totals, now. Both are monotonic, so a later reading
    /// minus an earlier one is what happened in between.
    fn heap_now() -> (u64, u64) {
        (crate::heap::allocations(), crate::heap::bytes_allocated())
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
                paint_children_culled: 0,
                flatten_nodes_cached: 0,
                flatten_nodes_flattened: 0,
                damage_none: 0,
                damage_partial: 0,
                damage_full: 0,
                paint_phase: PhaseAccum::new(),
                flatten_phase: PhaseAccum::new(),
                gpu_render_phase: PhaseAccum::new(),
                cache_paint_phase: PhaseAccum::new(),
                window_children_total: 0,
                window_children_iterated: 0,
                window_declined_children: 0,
                window_declined_containers: 0,
                allocations: 0,
                bytes_allocated: 0,
                heap_mark: heap_now(),
                last_print: Instant::now(),
            }
        }

        /// Fold what the allocator has been asked for since the last sample
        /// into the total, and mark where it now stands.
        ///
        /// The samples are taken at [`reset_stats`] and at the end of every
        /// frame, so a consumer that scopes one frame with `reset_stats` — a
        /// benchmark does — is told what that frame asked for, and a driver
        /// reporting once a second is told what the second asked for.
        fn sample_heap(&mut self) {
            let (allocations, bytes) = heap_now();
            self.allocations += allocations - self.heap_mark.0;
            self.bytes_allocated += bytes - self.heap_mark.1;
            self.heap_mark = (allocations, bytes);
        }

        /// Every counter back to nothing and the second back to now, which is
        /// what `new` already says. Spelled this way rather than field by
        /// field so a counter added above cannot be forgotten here and leak
        /// across the report that was supposed to clear it.
        fn reset(&mut self) {
            *self = Self::new();
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

    /// Record a clean off-screen child that was culled (skipped paint).
    #[inline]
    pub fn record_paint_child_culled() {
        STATS.with(|s| {
            s.borrow_mut().paint_children_culled += 1;
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

    /// Record a render pipeline phase duration.
    #[inline]
    pub fn record_phase_duration(phase: Phase, duration: Duration) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            match phase {
                Phase::Paint => stats.paint_phase.record(duration),
                Phase::Flatten => stats.flatten_phase.record(duration),
                Phase::GpuRender => stats.gpu_render_phase.record(duration),
                Phase::CachePaintResults => stats.cache_paint_phase.record(duration),
            }
        });
    }

    /// Record a container that narrowed its children to the visible rect.
    #[inline]
    pub fn record_paint_window(total_children: u64, iterated: u64) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.window_children_total += total_children;
            stats.window_children_iterated += iterated;
        });
    }

    /// Record a container that had a rect to narrow to and could not: its
    /// children are ordered along no axis, so every one of them is examined.
    ///
    /// This is the number worth watching. A container in this state looks
    /// healthy from the outside — it draws the right thing — and pays for the
    /// whole of its list on every frame that repaints it.
    #[inline]
    pub fn record_paint_window_declined(total_children: u64) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.window_declined_children += total_children;
            stats.window_declined_containers += 1;
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
                paint_children_culled: stats.paint_children_culled,
                flatten_nodes_cached: stats.flatten_nodes_cached,
                flatten_nodes_flattened: stats.flatten_nodes_flattened,
                damage_none: stats.damage_none,
                damage_partial: stats.damage_partial,
                damage_full: stats.damage_full,
                paint_timing: stats.paint_phase.to_timing(),
                flatten_timing: stats.flatten_phase.to_timing(),
                gpu_render_timing: stats.gpu_render_phase.to_timing(),
                cache_paint_timing: stats.cache_paint_phase.to_timing(),
                window_children_total: stats.window_children_total,
                window_children_iterated: stats.window_children_iterated,
                window_declined_children: stats.window_declined_children,
                window_declined_containers: stats.window_declined_containers,
                allocations: stats.allocations,
                bytes_allocated: stats.bytes_allocated,
            }
        })
    }

    /// Reset all stats to zero, and start the second over (for test
    /// isolation, and for a consumer that wants one frame's numbers).
    pub fn reset_stats() {
        STATS.with(|s| {
            s.borrow_mut().reset();
        });
    }

    /// Move the report clock back, so a test can stand where a second's worth
    /// of frames would have put it without spending the second.
    #[cfg(test)]
    pub fn backdate_report_clock(by: Duration) {
        STATS.with(|s| {
            s.borrow_mut().last_print -= by;
        });
    }

    /// Record what a frame damaged. Accumulation only: whether any of this is
    /// ever printed is [`report_if_due`]'s question, and the driver's.
    pub fn end_frame(damage: &DamageRegion) {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();
            stats.sample_heap();

            match damage {
                DamageRegion::None => stats.damage_none += 1,
                DamageRegion::Partial(_) => stats.damage_partial += 1,
                DamageRegion::Full => stats.damage_full += 1,
            }
        });
    }

    /// Print the accumulated stats and clear them, once a second.
    ///
    /// The driver calls this, not the frame path: `App::run` wants the running
    /// commentary, and a harness or a benchmark stepping the same loop wants
    /// its counters left where it can read them.
    pub fn report_if_due() {
        STATS.with(|s| {
            let mut stats = s.borrow_mut();

            if stats.last_print.elapsed().as_secs() < 1 {
                return;
            }

            let total_frames = stats.frames_painted + stats.frames_skipped;

            let layout_skip_rate = if stats.layout_total_calls > 0 {
                (stats.layout_skipped as f64 / stats.layout_total_calls as f64) * 100.0
            } else {
                0.0
            };

            let paint_total = stats.paint_children_cached
                + stats.paint_children_painted
                + stats.paint_children_culled;
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
                "  paint: children={} cached={} painted={} culled={} cache_rate={:.1}%",
                paint_total,
                stats.paint_children_cached,
                stats.paint_children_painted,
                stats.paint_children_culled,
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

            // Timing output
            let pt = stats.paint_phase.to_timing();
            let ft = stats.flatten_phase.to_timing();
            let gt = stats.gpu_render_phase.to_timing();
            let ct = stats.cache_paint_phase.to_timing();
            eprintln!(
                "  timing (avg/min/max us): paint={:.0}/{:.0}/{:.0} flatten={:.0}/{:.0}/{:.0} gpu={:.0}/{:.0}/{:.0} cache={:.0}/{:.0}/{:.0}",
                pt.avg_us, pt.min_us, pt.max_us,
                ft.avg_us, ft.min_us, ft.max_us,
                gt.avg_us, gt.min_us, gt.max_us,
                ct.avg_us, ct.min_us, ct.max_us,
            );

            // The heap, for a binary that installed the counting allocator.
            // Nothing did, and there is no line: a permanent `heap: 0` would
            // be reporting the absence of an allocator as a frame that
            // allocated nothing.
            //
            // The first of these lines is charged everything the process
            // allocated before its first frame — the mark is taken when this
            // thread's counters are built, and only a frame moves it. It is the
            // second line onwards that describes a second's frames. A consumer
            // that wants one frame's number scopes it with `reset_stats`, which
            // is what the benchmarks do.
            if stats.allocations > 0 {
                eprintln!(
                    "  heap: allocations={} bytes={}",
                    stats.allocations, stats.bytes_allocated
                );
            }

            // The paint window, and what it could not narrow.
            if stats.window_children_total > 0 || stats.window_declined_children > 0 {
                eprintln!(
                    "  window: offered={} narrowed_to={} | declined={} children in {} containers",
                    stats.window_children_total,
                    stats.window_children_iterated,
                    stats.window_declined_children,
                    stats.window_declined_containers
                );
            }

            stats.reset();
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
pub fn record_paint_child_culled() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_flatten_cached() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_flatten_full() {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_phase_duration(_phase: Phase, _duration: std::time::Duration) {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_paint_window(_total_children: u64, _iterated: u64) {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn record_paint_window_declined(_total_children: u64) {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn end_frame(_damage: &crate::tree::DamageRegion) {}

#[cfg(not(feature = "render-stats"))]
#[inline(always)]
pub fn report_if_due() {}

#[cfg(test)]
#[cfg(feature = "render-stats")]
mod tests {
    use super::*;
    use crate::tree::DamageRegion;
    use crate::widgets::Rect;
    use std::time::Duration;

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
        record_paint_child_culled();
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
        assert_eq!(s.paint_children_culled, 0);
        assert_eq!(s.flatten_nodes_cached, 0);
        assert_eq!(s.flatten_nodes_flattened, 0);
        assert_eq!(s.damage_none, 0);
        assert_eq!(s.damage_partial, 0);
        assert_eq!(s.damage_full, 0);
    }

    #[test]
    fn test_phase_duration_recording() {
        setup();

        record_phase_duration(Phase::Paint, Duration::from_micros(100));
        record_phase_duration(Phase::Paint, Duration::from_micros(200));
        record_phase_duration(Phase::Paint, Duration::from_micros(150));

        record_phase_duration(Phase::Flatten, Duration::from_micros(50));

        let s = get_stats();
        assert!((s.paint_timing.avg_us - 150.0).abs() < 1.0);
        assert!((s.paint_timing.min_us - 100.0).abs() < 1.0);
        assert!((s.paint_timing.max_us - 200.0).abs() < 1.0);
        assert!((s.flatten_timing.avg_us - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_paint_window() {
        setup();
        record_paint_window(10000, 52);
        record_paint_window(10000, 48);
        let s = get_stats();
        assert_eq!(s.window_children_total, 20000);
        assert_eq!(s.window_children_iterated, 100);
    }

    /// `end_frame` accumulates and returns. It used to ask a clock nobody
    /// else had started whether a second was up, and clear the counters where
    /// it stood — so a consumer reading one frame's numbers raced it.
    #[test]
    fn end_frame_keeps_the_counters_a_second_later() {
        setup();
        record_frame_painted();
        backdate_report_clock(Duration::from_secs(1));

        end_frame(&DamageRegion::Full);

        let s = get_stats();
        assert_eq!(s.frames_painted, 1, "end_frame cleared a counter");
        assert_eq!(s.damage_full, 1, "end_frame did not record the damage");
    }

    /// The cadence is `report_if_due`'s now, and so is the clearing: it keeps
    /// what it has not printed yet, and clears what it has.
    #[test]
    fn report_if_due_clears_once_the_second_is_up() {
        setup();
        record_frame_painted();

        report_if_due();
        assert_eq!(
            get_stats().frames_painted,
            1,
            "reported before the second was up"
        );

        backdate_report_clock(Duration::from_secs(1));
        report_if_due();
        assert_eq!(
            get_stats(),
            StatsSnapshot::default(),
            "the second was up and the counters were not cleared"
        );
    }

    /// A container that could not narrow is counted apart from one whose
    /// window happened to cover everything. Both used to read as a window.
    #[test]
    fn test_paint_window_declined() {
        setup();
        record_paint_window(10000, 10000);
        record_paint_window_declined(10000);
        let s = get_stats();
        assert_eq!(s.window_children_iterated, 10000);
        assert_eq!(s.window_declined_children, 10000);
        assert_eq!(s.window_declined_containers, 1);
    }
}
