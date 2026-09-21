//! A clipped subtree at rest, beside something that will not stop painting.
//!
//! The shape of a status bar rather than a list: a clock ticks, so the surface
//! paints, and every clipped container beside it is walked again although
//! nothing in it moved. `benches/scroll_list` cannot show what #442 buys
//! because it holds nothing at rest under a clip — every node under its
//! scroller moves on every frame. This holds the other case, and only that
//! case, so the number it produces is about one thing.
//!
//! Two panels, because there are two ways to be clipped and they are not the
//! same workload. [`Panel::Hidden`] is an `Overflow::Hidden` box, which clips
//! and does not cull: every row it holds is painted and, on main, flattened
//! again on every frame. [`Panel::Resting`] is an ordinary scroller that is
//! simply not being scrolled, which is what a scrollable spends almost all of
//! its life doing — it culls to its viewport, so the subtree at stake is
//! smaller, and it is by far the more common shape in a real application.
//!
//! What ticks here is a small scroller rather than a clock, because the script
//! both benchmarks play is a list of scroll deltas and a scroller is what
//! answers them. It sits in the middle of the surface, under the pointer the
//! script aims at, with a clipped panel above and below it.

use guido::prelude::*;

/// The surface, in logical pixels.
pub const VIEWPORT: (u32, u32) = (600, 800);

/// The strip that is scrolled, and so repaints, every frame.
const TICKER_HEIGHT: f32 = 200.0;
/// Each of the two panels above and below it.
const PANEL_HEIGHT: f32 = 300.0;
/// Rows inside the ticker: enough that the script never reaches the bottom of
/// it. The gesture travels `frames_per_phase * 624` pixels before it turns
/// round — 37440 at sixty — and a ticker shorter than that pins, stops
/// painting, and leaves the panels beside it with no frames to be measured in.
/// They cost nothing to have: the paint window narrows to what is visible, so
/// only the handful of rows inside the strip is ever painted or flattened.
const TICKER_ROWS: usize = 2000;

const ROW_HEIGHT: f32 = 24.0;
const ROW_GAP: f32 = 4.0;
const PAD: f32 = 8.0;

/// Which way the panels are clipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// `Overflow::Hidden`: clips, and does not cull.
    Hidden,
    /// A scroller that is never scrolled — what a scrollable is almost always
    /// doing. It culls to its viewport, so less of it is at stake per frame.
    Resting,
}

impl Panel {
    /// The word this scenario is named by on the command line and in the table.
    pub fn name(self) -> &'static str {
        match self {
            Panel::Hidden => "overflow-hidden",
            Panel::Resting => "resting-scroller",
        }
    }

    /// Every scenario, in the order the benchmark prints them.
    pub const ALL: [Panel; 2] = [Panel::Resting, Panel::Hidden];
}

/// A clipped panel whose contents never change, over a ticker that is scrolled
/// every frame, over a second such panel.
///
/// `row_count` is the rows in *each* panel.
pub fn scene(row_count: usize, panels: Panel) -> Container {
    container()
        .layout(Flex::column())
        .child(panel(row_count, panels))
        .child(ticker())
        .child(panel(row_count, panels))
}

/// A clip over more rows than it can show. Nothing here ever moves or
/// repaints: it is the subtree #442 is about.
fn panel(row_count: usize, kind: Panel) -> Container {
    let rows = container()
        .layout(Flex::column().spacing(ROW_GAP))
        .padding(PAD)
        .children((0..row_count).map(|_| row(Color::rgb(0.22, 0.22, 0.30))));
    let panel = container()
        .width(VIEWPORT.0 as f32)
        .height(PANEL_HEIGHT)
        .background(Color::rgb(0.10, 0.10, 0.14));
    match kind {
        Panel::Hidden => panel.overflow(Overflow::Hidden).child(rows),
        Panel::Resting => panel.scroll(Scroll::vertical()).child(rows),
    }
}

/// The scroller the script drives, and the whole reason a frame is painted at
/// all. Small, so that what it costs does not drown what the panels save.
fn ticker() -> Container {
    container()
        .width(VIEWPORT.0 as f32)
        .height(TICKER_HEIGHT)
        .scroll(Scroll::vertical())
        .background(Color::rgb(0.14, 0.14, 0.19))
        .child(
            container()
                .layout(Flex::column().spacing(ROW_GAP))
                .padding(PAD)
                .children((0..TICKER_ROWS).map(|_| row(Color::rgb(0.30, 0.30, 0.40)))),
        )
}

/// One row: three boxes, and no text.
///
/// Text would put the machine's installed fonts into the paint phase, and what
/// is being compared here is flatten. Three commands rather than one so that a
/// replayed subtree is carrying something.
fn row(fill: Color) -> Container {
    container()
        .layout(Flex::row().spacing(ROW_GAP))
        .height(ROW_HEIGHT)
        .child(
            container()
                .width(ROW_HEIGHT)
                .height(ROW_HEIGHT)
                .corners(4.0)
                .background(fill),
        )
        .child(
            container()
                .width(fill_width())
                .height(ROW_HEIGHT)
                .corners(4.0)
                .background(fill),
        )
        .child(
            container()
                .width(ROW_HEIGHT)
                .height(ROW_HEIGHT)
                .corners(4.0)
                .background(fill),
        )
}

fn fill_width() -> impl Into<Length> {
    fill()
}
