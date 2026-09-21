//! A scrolling workload that runs the same way twice.
//!
//! Two binaries compile this file. `benches/scroll_list/main.rs` plays the
//! script once, at as many rows as you ask for, and prints the table;
//! `tests/scroll_benchmark_is_repeatable.rs` plays it twice at a fraction of
//! that and asserts the two runs agree. The second is why this is a file of its
//! own rather than the benchmark's `main`: a benchmark nothing watches is a
//! number nobody can check.
//!
//! What repeats and what does not is the whole design, and it is written down
//! on [`Counts`], on [`Run`] and on [`PhaseCost`], beside the fields it decides.

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::render_stats::{self, PhaseTiming, StatsSnapshot};
use guido::testing::Headless;

/// The gap between frames. Named rather than slept through: every instant in
/// the run is computed from the first, so the script plays at the same speed on
/// a loaded machine as on an idle one.
const FRAME: Duration = Duration::from_millis(16);

/// Where the wheel points — the middle of the viewport, clear of the scrollbar
/// down the right edge.
const POINTER: (f32, f32) = (280.0, 400.0);

/// The gesture, as one pixel delta per frame.
///
/// Four phases and the pauses between them: a slow drag down, a flick down, the
/// flick undone, the drag undone. It is symmetric, so the list ends where it
/// started.
///
/// It travels `frames_per_phase * 624` pixels before it turns round — 37440 at
/// the benchmark's sixty — and a list shorter than that pins at the bottom for
/// the rest of the flick. Nothing then moves, so nothing paints, and the phase
/// costs are taken over the frames that did: at 200 rows and sixty frames a
/// phase, 82 of 248 frames. It stays repeatable, which is why no assertion here
/// catches it; [`Counts::frames_pinned`] counts those frames and [`report`]
/// says so, rather than leaving a person to read an average of a stationary
/// list.
///
/// The zero-delta frames are not padding. A frame with nothing to do is not
/// painted, and a script with none of them would never once exercise the path
/// that decides so.
pub fn script(frames_per_phase: usize) -> Vec<f32> {
    let phase = |delta: f32| std::iter::repeat_n(delta, frames_per_phase);
    let pause = || std::iter::repeat_n(0.0, 2);

    phase(24.0)
        .chain(pause())
        .chain(phase(600.0))
        .chain(pause())
        .chain(phase(-600.0))
        .chain(pause())
        .chain(phase(-24.0))
        .chain(pause())
        .collect()
}

/// What the renderer did, as opposed to what it cost — and only the part of it
/// this application decides by itself.
///
/// Every field here is a consequence of the script and the tree, so two runs of
/// one script on one revision produce identical ones, on any machine and
/// however loaded. That is the property the hand-scrolled runs lacked, where
/// two attempts at `examples/bench_list` polled 136714 frames and 83209. It is
/// also what makes the benchmark answer the question it exists for: point it at
/// two revisions and every number that moved is a number the code moved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub frames_painted: u64,
    /// Frames the loop did not paint, however far it got before deciding — see
    /// [`Run::frames_skipped`] for why the two ways are not counted apart here.
    pub frames_not_painted: u64,
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
    pub damage_partial: u64,
    pub damage_full: u64,
    pub window_children_total: u64,
    pub window_children_iterated: u64,
    /// A container that had a rect to narrow to and could not, and the children
    /// it therefore examined in full. `render_stats` calls this the number
    /// worth watching, and a list that scrolls is where it would show.
    pub window_declined_containers: u64,
    pub window_declined_children: u64,
    /// Scripted deltas that moved nothing: the scroller had reached an end and
    /// stood still. Zero is a gesture that stayed inside the list, which is the
    /// workload the benchmark is for — see [`script`].
    pub frames_pinned: u64,
}

impl Counts {
    fn add(&mut self, snapshot: &StatsSnapshot) {
        self.frames_painted += snapshot.frames_painted;
        self.frames_not_painted += 1 - snapshot.frames_painted;
        self.layout_total_calls += snapshot.layout_total_calls;
        self.layout_skipped += snapshot.layout_skipped;
        self.layout_executed += snapshot.layout_executed;
        self.layout_primary_constraints += snapshot.layout_primary_constraints;
        self.layout_primary_reactive += snapshot.layout_primary_reactive;
        self.paint_children_cached += snapshot.paint_children_cached;
        self.paint_children_painted += snapshot.paint_children_painted;
        self.paint_children_culled += snapshot.paint_children_culled;
        self.flatten_nodes_cached += snapshot.flatten_nodes_cached;
        self.flatten_nodes_flattened += snapshot.flatten_nodes_flattened;
        self.damage_partial += snapshot.damage_partial;
        self.damage_full += snapshot.damage_full;
        self.window_children_total += snapshot.window_children_total;
        self.window_children_iterated += snapshot.window_children_iterated;
        self.window_declined_containers += snapshot.window_declined_containers;
        self.window_declined_children += snapshot.window_declined_children;
    }

    /// A delta was asked for and nothing moved.
    fn pinned(&mut self) {
        self.frames_pinned += 1;
    }
}

/// One phase's cost, one sample per painted frame, in microseconds.
///
/// The samples are kept rather than folded into a running average, because the
/// average is what hid the thing worth finding: a 21ms flatten was seen once by
/// hand and never reproduced, and one frame in two hundred moves an average by
/// nothing at all.
///
/// Nothing here repeats. A microsecond is the machine's as much as the code's,
/// so no test reads one — they are for a person, on a quiet machine, comparing
/// two revisions.
#[derive(Debug, Default, Clone)]
pub struct PhaseCost {
    pub samples: Vec<f64>,
}

impl PhaseCost {
    pub fn avg_us(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    pub fn max_us(&self) -> f64 {
        self.samples.iter().copied().fold(0.0, f64::max)
    }

    /// The worst frame in twenty, which is what a dropped frame feels like —
    /// an average hides it and the maximum is one frame's bad luck.
    pub fn p95_us(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)]
    }
}

/// One play of the script.
#[derive(Debug, Default, Clone)]
pub struct Run {
    /// The adapter that drew every frame — the label the GPU column needs and
    /// the CPU columns do not.
    pub adapter: String,
    pub counts: Counts,
    /// Of the frames that did not paint, the ones where the loop was woken,
    /// asked the tree, and found nothing to repaint.
    ///
    /// Outside [`Counts`], with the two below, because the split is not this
    /// application's to make. The wake flag is a process-wide atomic by design
    /// — anything may ask for a frame from any thread — so in a test binary
    /// running four applications at once, whether a still frame is skipped or
    /// never asked about is decided partly by the others. Two runs that agreed
    /// on every paint, flatten and window count came back 5 and 3 here. In the
    /// benchmark, which is one application in a process of its own, the split
    /// is exact and worth printing.
    pub frames_skipped: u64,
    /// And the ones where nothing woke the loop to ask at all.
    pub frames_idle: u64,
    /// The damage those frames reported, which is none. It moves with them,
    /// and for the same reason: `end_frame` is handed `DamageRegion::None` on
    /// the path that skips a paint and is not called at all on the path that
    /// was never woken, so this is the skipped count under another name. It is
    /// printed beside them rather than in the table of counts, which would be
    /// claiming for it a repeatability it does not have.
    pub damage_none: u64,
    /// One snapshot per painted frame, in order. The phases are read off these
    /// rather than accumulated four times over.
    painted: Vec<StatsSnapshot>,
}

impl Run {
    /// One phase's cost across the painted frames.
    ///
    /// Each snapshot covers a single frame, so its `avg` is that frame's
    /// measurement and nothing has been averaged yet.
    pub fn phase(&self, of: fn(&StatsSnapshot) -> &PhaseTiming) -> PhaseCost {
        PhaseCost {
            samples: self.painted.iter().map(|s| of(s).avg_us).collect(),
        }
    }
}

/// Play `script` over the widget `build` makes, one frame per delta, and return
/// what it did and what it cost.
///
/// The first frame is stepped and thrown away. It paints every row there is,
/// because nothing has been painted yet, and it is not part of the gesture —
/// leaving it in would make the maximum of every phase the cost of arriving
/// rather than the worst frame of the scroll.
pub fn play<W, F>(app: &mut Headless, viewport: (u32, u32), build: F, script: &[f32]) -> Run
where
    W: Widget + 'static,
    F: FnOnce() -> W,
{
    let (width, height) = viewport;
    let id = app.surface(
        SurfaceConfig::new()
            .width(width)
            .height(height)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .namespace("guido-scroll-bench"),
        build,
    );
    app.configure(id, width, height, 1.0);

    let start = Instant::now();
    app.step_at(start);

    let mut run = Run {
        adapter: app.adapter_name().to_string(),
        ..Default::default()
    };

    for (frame, delta) in script.iter().enumerate() {
        let at = start + FRAME * (frame as u32 + 1);

        // The counters are scoped to this one frame, which is what the table
        // below is made of.
        render_stats::reset_stats();

        if *delta != 0.0 {
            app.event_at(
                id,
                Event::scroll(POINTER.0, POINTER.1, 0.0, *delta, ScrollSource::Wheel),
                at,
            );
        }
        app.step_at(at);

        let snapshot = render_stats::get_stats();
        assert!(
            snapshot.frames_painted + snapshot.frames_skipped <= 1,
            "frame {frame} accounted for more than one frame",
        );

        run.counts.add(&snapshot);
        // A delta that painted nothing is a delta the scroller refused: it had
        // reached an end, and `apply_scroll` returns false rather than asking
        // for a frame.
        if *delta != 0.0 && snapshot.frames_painted == 0 {
            run.counts.pinned();
        }
        run.frames_skipped += snapshot.frames_skipped;
        run.frames_idle += 1 - snapshot.frames_painted - snapshot.frames_skipped;
        run.damage_none += snapshot.damage_none;
        // A frame that did not paint ran no phase, and its zeroes are not a
        // measurement.
        if snapshot.frames_painted == 1 {
            run.painted.push(snapshot);
        }
    }

    run
}

/// The run as a person reads it: the counts that repeat, the CPU phases that
/// are the answer, and the GPU phase under the adapter that produced it.
pub fn report(run: &Run, rows: usize) -> String {
    let mut out = String::new();
    let counts = &run.counts;

    out.push_str(&format!(
        "rows={rows} frames={} painted={} | skipped={} idle={} damage_none={} \
         (these three do not repeat)\n",
        counts.frames_painted + counts.frames_not_painted,
        counts.frames_painted,
        run.frames_skipped,
        run.frames_idle,
        run.damage_none,
    ));

    if counts.frames_pinned > 0 {
        out.push_str(&format!(
            "\n{} of the scripted deltas moved nothing: the list reached an end \
             and stood still for them, and the phase costs below are taken over \
             the frames that did move. More rows, or fewer frames per phase.\n",
            counts.frames_pinned
        ));
    }

    out.push_str("\ncounts (identical on every run of this revision)\n");
    for (name, value) in [
        ("layout.calls", counts.layout_total_calls),
        ("layout.skipped", counts.layout_skipped),
        ("layout.executed", counts.layout_executed),
        ("layout.by_constraints", counts.layout_primary_constraints),
        ("layout.by_reactive", counts.layout_primary_reactive),
        ("paint.children_cached", counts.paint_children_cached),
        ("paint.children_painted", counts.paint_children_painted),
        ("paint.children_culled", counts.paint_children_culled),
        ("paint.window_offered", counts.window_children_total),
        ("paint.window_iterated", counts.window_children_iterated),
        ("paint.window_declined", counts.window_declined_containers),
        ("paint.declined_children", counts.window_declined_children),
        ("flatten.nodes_cached", counts.flatten_nodes_cached),
        ("flatten.nodes_flattened", counts.flatten_nodes_flattened),
        ("damage.partial", counts.damage_partial),
        ("damage.full", counts.damage_full),
        ("script.deltas_pinned", counts.frames_pinned),
    ] {
        out.push_str(&format!("  {name:<26} {value}\n"));
    }

    out.push_str("\ncpu phases, microseconds per painted frame\n");
    out.push_str(&format!(
        "  {:<10} {:>10} {:>10} {:>10}\n",
        "", "avg", "p95", "max"
    ));
    for (name, phase) in [
        ("paint", run.phase(|s| &s.paint_timing)),
        ("flatten", run.phase(|s| &s.flatten_timing)),
        ("cache", run.phase(|s| &s.cache_paint_timing)),
    ] {
        out.push_str(&format!(
            "  {name:<10} {:>10.0} {:>10.0} {:>10.0}\n",
            phase.avg_us(),
            phase.p95_us(),
            phase.max_us()
        ));
    }

    let gpu = run.phase(|s| &s.gpu_render_timing);
    out.push_str(&format!(
        "\ngpu phase, microseconds per painted frame, on adapter {}\n\
         not comparable with any other adapter, and not part of the numbers above\n",
        run.adapter
    ));
    out.push_str(&format!(
        "  {:<10} {:>10.0} {:>10.0} {:>10.0}\n",
        "gpu",
        gpu.avg_us(),
        gpu.p95_us(),
        gpu.max_us()
    ));

    out
}
