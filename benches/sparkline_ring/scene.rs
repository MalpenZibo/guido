//! A scroller of status rows, each a ring and a sparkline that keep moving.
//!
//! What a status bar's meters are made of: a ring whose progress eases to a
//! new value, and a line chart with the area under it filled, both fed by a
//! timer. The timer ticks every [`TICK`] — four frames of the script — and on
//! each tick every row gets a new reading: its ring starts easing towards it,
//! and its chart shifts one sample along, which is a new path to tessellate.
//! Between ticks the rings are still easing, and nothing is re-tessellated.
//!
//! The scroller is what the script moves, so on every frame some rows move
//! under the viewport, and a path that only moved draws the mesh it has.
//!
//! Above the scroller, standing still, is one row with a dashed outline whose
//! dashes march along it every frame — marching ants. A new offset is a new
//! mesh, so that one path is tessellated every frame, the case #755 asked to
//! have measured. It steps by a fraction of a pixel that no whole number of
//! frames in the script brings back round to a period, so no frame finds its
//! mesh cached from an earlier one: what it costs is the worst case.

use std::time::Duration;

use guido::prelude::*;
use guido::renderer::{Fill, Join, Path, Stroke};
use guido::widget_prelude::*;

/// The marching outline's pattern, and how far it marches each frame: 0.37
/// pixels against a period of 12 comes back to where it began only after 1200
/// frames, more than the script plays.
const ANTS: [f32; 2] = [8.0, 4.0];
const ANTS_STEP: f32 = 0.37;
const ANTS_HEIGHT: f32 = 40.0;

/// The gap between the script's frames, which is how often the ants march.
const FRAME: Duration = Duration::from_millis(16);

/// The surface, in logical pixels: the marching outline's row, and under it a
/// scroller 800 pixels tall.
pub const VIEWPORT: (u32, u32) = (600, 840);

const ROW_HEIGHT: f32 = 44.0;
const RING: f32 = 32.0;
const CHART_WIDTH: f32 = 480.0;
const CHART_HEIGHT: f32 = 32.0;
const SAMPLES: usize = 48;

/// How often a reading arrives. The frames are 16 milliseconds apart, so this
/// is every fourth frame.
pub const TICK: Duration = Duration::from_millis(64);

/// A reading, from 0 to 1: deterministic in the tick and the row, so two runs
/// draw the same paths.
fn reading(tick: u32, row: usize, sample: usize) -> f32 {
    let t = (tick as usize + sample) as f32 * 0.37 + row as f32 * 1.3;
    0.5 + 0.3 * t.sin() + 0.15 * (t * 2.9).cos()
}

/// `rows` rows in a scroller the size of the surface.
pub fn scene(rows: usize) -> Container {
    let tick = create_signal(0u32);
    // The handle is dropped and the timer kept: it lives as long as the owner
    // building the surface, which is the run.
    let _ = set_interval(TICK, move || tick.update(|t| *t += 1));
    let march = create_signal(0u32);
    let _ = set_interval(FRAME, move || march.update(|m| *m += 1));
    container()
        .layout(Flex::column())
        .background(Color::rgb(0.10, 0.10, 0.14))
        .child(Ants { march })
        .child(
            container()
                .width(VIEWPORT.0 as f32)
                .height(VIEWPORT.1 as f32 - ANTS_HEIGHT)
                .scroll(Scroll::vertical())
                .child(
                    container()
                        .layout(Flex::column())
                        .padding(8.0)
                        .children((0..rows).map(move |row| status(row, tick))),
                ),
        )
}

/// A rounded outline the width of the surface whose dashes march along it,
/// one step a frame.
struct Ants {
    march: RwSignal<u32>,
}

impl Widget for Ants {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        constraints.constrain(Size::new(VIEWPORT.0 as f32, ANTS_HEIGHT))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let (x0, y0, x1, y1, r) = (4.0, 4.0, VIEWPORT.0 as f32 - 4.0, ANTS_HEIGHT - 4.0, 8.0);
        let outline = Path::new()
            .move_to((x0 + r, y0))
            .line_to((x1 - r, y0))
            .quad_to((x1, y0), (x1, y0 + r))
            .line_to((x1, y1 - r))
            .quad_to((x1, y1), (x1 - r, y1))
            .line_to((x0 + r, y1))
            .quad_to((x0, y1), (x0, y1 - r))
            .line_to((x0, y0 + r))
            .quad_to((x0, y0), (x0 + r, y0))
            .close();
        let offset = -(self.march.get() as f32) * ANTS_STEP;
        ctx.draw_path(
            &outline,
            None,
            Stroke::new(1.5, Color::rgb(0.95, 0.75, 0.25)).dashes(&ANTS, offset),
        );
    }
}

fn status(row: usize, tick: RwSignal<u32>) -> Container {
    container()
        .height(ROW_HEIGHT)
        .padding([6.0, 4.0])
        .layout(
            Flex::row()
                .spacing(12.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(
            arc()
                .size(RING)
                .stroke(3.0)
                .track(Color::WHITE.with_alpha(0.16))
                .color(Color::rgb(0.40, 0.85, 0.50))
                .caps(Cap::Round)
                .progress((move || reading(tick.get(), row, 0)).transition(200.0)),
        )
        .child(Chart { row, tick })
}

/// The last [`SAMPLES`] readings of one row, as a filled area under a line.
struct Chart {
    row: usize,
    tick: RwSignal<u32>,
}

impl Widget for Chart {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        constraints.constrain(Size::new(CHART_WIDTH, CHART_HEIGHT))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Read while painting, so a new reading repaints this chart and
        // nothing else.
        let tick = self.tick.get();
        let step = CHART_WIDTH / (SAMPLES - 1) as f32;
        let line = (0..SAMPLES).fold(Path::new(), |path, i| {
            let point = (
                i as f32 * step,
                CHART_HEIGHT * (1.0 - reading(tick, self.row, i)),
            );
            if i == 0 {
                path.move_to(point)
            } else {
                path.line_to(point)
            }
        });
        let area = line
            .clone()
            .line_to((CHART_WIDTH, CHART_HEIGHT))
            .line_to((0.0, CHART_HEIGHT))
            .close();
        ctx.draw_path(
            &area,
            Fill::color(Color::rgb(0.45, 0.80, 1.0).with_alpha(0.18)),
            None,
        );
        ctx.draw_path(
            &line,
            None,
            Stroke::new(1.6, Color::rgb(0.45, 0.80, 1.0)).joins(Join::Round),
        );
    }
}
