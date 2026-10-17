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

use std::time::Duration;

use guido::prelude::*;
use guido::renderer::{Fill, Join, Path, Stroke};
use guido::widget_prelude::*;

/// The surface, in logical pixels.
pub const VIEWPORT: (u32, u32) = (600, 800);

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
    container()
        .width(VIEWPORT.0 as f32)
        .height(VIEWPORT.1 as f32)
        .scroll(Scroll::vertical())
        .background(Color::rgb(0.10, 0.10, 0.14))
        .child(
            container()
                .layout(Flex::column())
                .padding(8.0)
                .children((0..rows).map(move |row| status(row, tick))),
        )
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
