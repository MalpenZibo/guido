//! What reaches paint inside a scroller, and what does not.
//!
//! `paint_children` narrows a parent's children to the ones the cull rect can
//! reach, with a binary search over their bounds. That window is the only
//! thing that bounds the work unconditionally: the per-child cull beside it is
//! guarded by `!tree.needs_paint(child_id)`, so on a first paint — and after
//! anything that dirties the subtree — it drops nothing and every child is
//! painted in full.
//!
//! So the window is worth asserting on directly, over a first paint, where the
//! cull cannot stand in for it. Neither pass needs a compositor or a GPU.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::widget_prelude::{Constraints, Layout, Tree, WidgetId};

const ROWS: usize = 200;
const ROW_WIDTH: f32 = 120.0;
const ROW_HEIGHT: f32 = 24.0;
const SPACING: f32 = 8.0;
const VIEWPORT: f32 = 200.0;

/// Eight places whose `y` runs 0, 400, 400 … 40 and whose `x` runs
/// 0, 300, 300 … 0 — ordered along neither axis, and unordered in the same
/// shape on each of them separately. In a 100 px viewport the first and the
/// last are visible.
///
/// Both coordinates carry their weight. Searching the `y` for `y < 100` lands
/// on index 1 and drops the last row, which is what catches a wrong vertical
/// answer; the `x` catches a wrong horizontal one, because a search for
/// `x < 200` lands on index 1 in exactly the same way. With every row at one
/// `x` the horizontal branch could answer anything and no row would move —
/// which is what `cargo mutants` found by turning its `&=` into `|=` and
/// watching nothing object.
const SCATTERED: [(f32, f32); 8] = [
    (0.0, 0.0),
    (300.0, 400.0),
    (300.0, 400.0),
    (300.0, 400.0),
    (300.0, 400.0),
    (300.0, 400.0),
    (300.0, 400.0),
    (0.0, 40.0),
];

/// The rows that reached paint, counted by size so that the count means the
/// same thing however deep they sit — which is the whole question here.
fn painted_rows(widget: impl Widget + 'static, viewport: f32) -> usize {
    Harness::laid_out(widget, 400.0, viewport)
        .painted_rects()
        .iter()
        .filter(|r| (r.width - ROW_WIDTH).abs() < 0.01 && (r.height - ROW_HEIGHT).abs() < 0.01)
        .count()
}

fn rows(n: usize) -> Vec<guido::widgets::Container> {
    (0..n)
        .map(|_| {
            container()
                .width(ROW_WIDTH)
                .height(ROW_HEIGHT)
                .background(Color::rgb(0.25, 0.25, 0.35))
        })
        .collect()
}

/// The rows are the same rows and the viewport is the same viewport, so the
/// window has to reach the same number of them however deep they sit.
///
/// After `examples/bench_list.rs`, which is written the wrapped way — as are
/// `examples/scroll_example.rs` and `examples/perf_stress_test.rs`.
#[test]
fn a_wrapped_list_paints_the_rows_an_unwrapped_one_paints() {
    let direct = painted_rows(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .layout(Flex::column().spacing(SPACING))
            .children(rows(ROWS)),
        VIEWPORT,
    );

    let wrapped = painted_rows(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(SPACING))
                    .children(rows(ROWS)),
            ),
        VIEWPORT,
    );

    // 200 px of viewport over a 32 px pitch is seven rows, plus the one either
    // side the window carries for a child whose transform moves what it draws.
    assert!(
        direct <= 12,
        "the direct form is no longer windowed: {direct} of {ROWS} rows painted"
    );
    assert_eq!(
        wrapped, direct,
        "wrapping the rows cost the list its window: {wrapped} rows painted where the same rows \
         as direct children paint {direct}"
    );
}

/// A window over bounds that are not ordered along the axis it searches does
/// not answer about the viewport. `partition_point` needs a partitioned slice,
/// and nothing used to check that the children were one — it was inferred from
/// `scroll_axis` alone, and a binary search over an unpartitioned slice returns
/// whichever index it happened to land on.
///
/// So the container has to be able to say *no*: children ordered along neither
/// axis are all painted, because there is no window to be had and a wrong one
/// drops something visible.
#[test]
fn a_scroller_whose_children_are_ordered_along_neither_axis_paints_all_of_them() {
    // Eight rows whose `y` runs 0, 400, 400 … 40 and whose `x` runs
    // 0, 300, 300 … 0 — ordered along neither axis, and unordered in the same
    // shape on both. Two are visible in a 100 px viewport: the first and the
    // last.
    //
    // Both coordinates carry their weight. Searching the `y` for `y < 100`
    // lands on index 1 and drops the last row, which is what this catches if
    // the vertical answer is wrong; the `x` is what catches a wrong horizontal
    // one, because a search for `x < 200` lands on index 1 in just the same
    // way. With every row at one `x` the horizontal branch could answer
    // anything and no row would move.
    let painted = painted_rows(
        container()
            .width(200.0)
            .height(100.0)
            .scroll(Scroll::vertical())
            .layout(Scatter(SCATTERED.to_vec()))
            .children(rows(SCATTERED.len())),
        100.0,
    );

    assert_eq!(
        painted,
        SCATTERED.len(),
        "a scroller binary-searched bounds that are not partitioned along its axis and dropped a \
         child sitting in the viewport: {painted} of {} painted",
        SCATTERED.len()
    );
}

/// Places its children exactly where it is told, so a test can hand a container
/// children ordered along neither axis — and unordered along each of them
/// separately, which is what watches both halves of the answer. `Flex` orders
/// them along one axis by construction and `ZStack` puts them all at the
/// origin, where every predicate a search asks is constant and the old code
/// came out right by accident; neither can pose the question.
struct Scatter(Vec<(f32, f32)>);

impl Layout for Scatter {
    fn layout(
        &mut self,
        tree: &mut Tree,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let mut size = Size::zero();
        for (&child, &(x, y)) in children.iter().zip(&self.0) {
            let child_size = tree
                .with_widget_mut(child, |w, id, t| w.layout(t, id, constraints))
                .unwrap_or_default();
            tree.set_origin(child, origin.0 + x, origin.1 + y);
            size.width = size.width.max(x + child_size.width);
            size.height = size.height.max(y + child_size.height);
        }
        size
    }
}

/// The counter has to be able to tell a container that narrowed from one that
/// could not, because "1 offered, 1 iterated" is what the wrapped form used to
/// report and it reads as a healthy window over a list of one.
///
/// Behind the feature, because that is where the counters exist at all; the
/// lavapipe job runs the suite with every feature on.
#[cfg(feature = "render-stats")]
#[test]
fn the_counter_separates_a_narrowed_window_from_one_that_could_not_narrow() {
    use guido::render_stats::{get_stats, reset_stats};

    reset_stats();
    painted_rows(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .layout(Flex::column().spacing(SPACING))
            .children(rows(ROWS)),
        VIEWPORT,
    );
    let narrowed = get_stats();
    assert_eq!(
        narrowed.window_children_total, ROWS as u64,
        "the window was not offered the rows"
    );
    assert!(
        narrowed.window_children_iterated < ROWS as u64,
        "the window let every row through: {} of {ROWS}",
        narrowed.window_children_iterated
    );
    assert_eq!(
        narrowed.window_declined_containers, 0,
        "a column of rows is ordered along an axis and nothing should have declined"
    );

    reset_stats();
    painted_rows(
        container()
            .width(200.0)
            .height(100.0)
            .scroll(Scroll::vertical())
            .layout(Scatter(SCATTERED.to_vec()))
            .children(rows(8)),
        100.0,
    );
    let declined = get_stats();
    assert_eq!(
        declined.window_declined_containers, 1,
        "a container that could not narrow went uncounted"
    );
    assert_eq!(
        declined.window_declined_children, 8,
        "the children it had to examine one at a time went uncounted"
    );
}
