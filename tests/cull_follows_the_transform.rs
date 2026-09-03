//! Where a widget draws, not where it was laid out.
//!
//! A cull rect is handed to a child translated by the child's laid-out origin.
//! A child's own `translate`, `rotate` or `scale` is applied later, inside its
//! own `paint`, so the rect describes where the child *would* have drawn. Both
//! things that narrow children to that rect — the binary-search window and the
//! per-child cull beside it — then answer about the wrong rows.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::renderer::RenderNode;

const ROW_WIDTH: f32 = 120.0;
const ROW_HEIGHT: f32 = 24.0;
const PITCH: f32 = 32.0;
const VIEWPORT: f32 = 200.0;
/// Enough to carry a whole column clear of the viewport it sits in.
const LIFT: f32 = 300.0;
/// Enough to carry row fifteen from below the fold into the middle of it.
const ROW_LIFT: f32 = 400.0;
/// The odd one out, told from its twenty neighbours by width — a row clamps a
/// taller child to its own height, so a distinguishing *height* would not
/// survive layout and would match nothing at all.
const MARKED_WIDTH: f32 = 60.0;

/// Where each leaf of a given width ended up, in the root's coordinates.
///
/// By position rather than by count, because a count cannot tell a window that
/// narrowed to the right number of the wrong rows from one that narrowed to the
/// right rows. Leaves only, because a column takes its width from the rows
/// inside it and would otherwise be counted as one of them.
fn tops_of_width(node: &RenderNode, width: f32, offset: f32, out: &mut Vec<f32>) {
    let here = offset + node.local_transform.ty();
    if node.children.is_empty() && (node.bounds.width - width).abs() < 0.01 {
        out.push(here);
    }
    for child in &node.children {
        tops_of_width(child, width, here, out);
    }
}

fn tops(view: impl Widget + 'static, width: f32) -> Vec<f32> {
    let mut harness = Harness::laid_out(view, 400.0, VIEWPORT);
    let mut out = Vec::new();
    tops_of_width(&harness.paint(), width, 0.0, &mut out);
    out.sort_by(f32::total_cmp);
    out
}

fn rows(n: usize) -> Vec<guido::widgets::Container> {
    (0..n)
        .map(|_| {
            container()
                .width(ROW_WIDTH)
                .height(ROW_HEIGHT)
                .background(Color::BLUE)
        })
        .collect()
}

fn scroller(children: Vec<guido::widgets::Container>) -> guido::widgets::Container {
    container()
        .width(200.0)
        .height(VIEWPORT)
        .scroll(Scroll::vertical())
        .layout(Flex::column().spacing(PITCH - ROW_HEIGHT))
        .children(children)
}

/// The rows a lifted column shows are the rows the lift brings into view.
#[test]
fn a_translated_column_paints_the_rows_the_translate_brings_into_view() {
    let lifted = tops(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(PITCH - ROW_HEIGHT))
                    .translate(Translate::new(0.0, -LIFT))
                    .children(rows(20)),
            ),
        ROW_WIDTH,
    );

    assert!(!lifted.is_empty(), "a lifted column painted nothing at all");
    let stray: Vec<_> = lifted
        .iter()
        .copied()
        .filter(|top| *top + ROW_HEIGHT < -PITCH || *top > VIEWPORT + PITCH)
        .collect();
    assert!(
        stray.is_empty(),
        "rows were painted well outside the viewport, so the rect that chose \
         them did not follow the translate: tops {lifted:?}, strays {stray:?}"
    );
}

/// And the untransformed case still narrows, so the fix cannot be to widen the
/// rect until nothing is ever culled.
#[test]
fn an_untranslated_column_still_narrows_to_its_viewport() {
    let plain = tops(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(PITCH - ROW_HEIGHT))
                    .children(rows(20)),
            ),
        ROW_WIDTH,
    );

    assert!(
        plain.len() < 20,
        "the plain column stopped being narrowed: {} of 20 rows painted",
        plain.len()
    );
}

/// A row that its own transform brings into view is painted, on the first frame
/// and on the frames after it.
///
/// Both frames, because the two mechanisms take turns: on a first paint every
/// child is dirty, so the per-child cull drops nothing and only the window can
/// lose the row; from the second frame the row is clean and the cull can.
#[test]
fn a_row_its_own_transform_lifts_into_view_survives_both_frames() {
    let mut rows = rows(20);
    rows[15] = container()
        .width(MARKED_WIDTH)
        .height(ROW_HEIGHT)
        .background(Color::rgb(1.0, 0.0, 0.0))
        .translate(Translate::new(0.0, -ROW_LIFT));

    let mut harness = Harness::laid_out(scroller(rows), 400.0, VIEWPORT);
    let mut root = RenderNode::new(harness.root.as_u64());

    for frame in 1..=2 {
        root.clear();
        root.bounds = guido::widgets::Rect::new(0.0, 0.0, 400.0, VIEWPORT);
        let (tree, id) = (&mut harness.tree, harness.root);
        tree.with_widget_mut(id, |w, id, t| {
            let mut ctx = guido::renderer::PaintContext::new(&mut root);
            w.paint(t, id, &mut ctx);
        });

        let mut found = Vec::new();
        tops_of_width(&root, MARKED_WIDTH, 0.0, &mut found);
        assert_eq!(
            found.len(),
            1,
            "frame {frame}: the lifted row was dropped by the rect that chose \
             what to paint, though its own transform puts it in the viewport"
        );
        let expected = 15.0 * PITCH - ROW_LIFT;
        assert!(
            (found[0] - expected).abs() < 0.01,
            "frame {frame}: the lifted row was painted at {} rather than {expected}",
            found[0]
        );

        for child in &root.children {
            guido::cache_paint_results(&mut harness.tree, child);
        }
        harness.tree.clear_needs_paint(harness.root);
    }
}

/// A row that is not itself transformed, but holds something that is.
///
/// The reach has to gather upward. A parent narrows its children by their own
/// bounds grown by their own reach, so a row reporting nothing is culled where
/// it was laid out — and the box inside it that a transform brought on screen
/// goes with it. Flutter and Blink both union descendant paint bounds up the
/// tree for this reason.
#[test]
fn a_row_whose_child_is_lifted_into_view_is_kept() {
    let mut rows = rows(20);
    rows[15] = container().width(ROW_WIDTH).height(ROW_HEIGHT).child(
        container()
            .width(MARKED_WIDTH)
            .height(ROW_HEIGHT)
            .background(Color::rgb(1.0, 0.0, 0.0))
            .translate(Translate::new(0.0, -ROW_LIFT)),
    );

    let lifted = tops(scroller(rows), MARKED_WIDTH);

    assert_eq!(
        lifted.len(),
        1,
        "the row was culled at its laid-out bounds, taking with it the box its \
         own child's transform had brought into the viewport"
    );
}

/// A subtree scaled to nothing is culled entirely, not painted entirely.
///
/// `scale(0.0)` has no inverse, so there is no rect describing where its
/// content went. The answer is the empty rect — none of it is visible — and not
/// "no rect", which means nothing is narrowed at all. Collapsing by scale is
/// how a menu closes here, and the difference is every row of it painted on
/// every frame that dirties it.
#[test]
fn a_subtree_scaled_to_nothing_paints_nothing() {
    let painted = tops(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(PITCH - ROW_HEIGHT))
                    .scale(Scale::uniform(0.0))
                    .children(rows(20)),
            ),
        ROW_WIDTH,
    );

    // One, not none: the window keeps a child either side of what it found, for
    // the boundary rounding that has nothing to do with transforms. All twenty
    // is what it paints when the rect is dropped rather than emptied, which is
    // what the subtree gets if a non-invertible transform means "no rect".
    assert!(
        painted.len() <= 1,
        "a collapsed subtree painted {} rows nobody can see",
        painted.len()
    );
}

/// The window widens on the *near* side too, not only the far one.
///
/// A row whose bounds sit before the visible span can still reach into it — a
/// shadow hangs below the box that cast it. The search grows its span by the
/// widest reach among the children at both ends, and only the far end is
/// obvious: everything below the fold is a far-end case. This is the other one.
///
/// The reach here is an elevation rather than a transform, which is the point:
/// what the window widens by is how far a child paints outside itself, whatever
/// put it there.
#[test]
fn the_window_widens_on_the_near_side_as_well() {
    let mut rows: Vec<_> = (0..20)
        .map(|_| {
            container()
                .width(ROW_WIDTH)
                .height(ROW_HEIGHT)
                .background(Color::BLUE)
        })
        .collect();
    // Row seven ends at y=248, just above the 300..500 the lift brings into
    // view, and its shadow hangs 100px below that — into the middle of it.
    rows[7] = container()
        .width(MARKED_WIDTH)
        .height(ROW_HEIGHT)
        .background(Color::rgb(1.0, 0.0, 0.0))
        .elevation(24.0);

    let lifted = tops(
        container()
            .width(200.0)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(PITCH - ROW_HEIGHT))
                    .translate(Translate::new(0.0, -LIFT))
                    .children(rows),
            ),
        MARKED_WIDTH,
    );

    assert_eq!(
        lifted.len(),
        1,
        "a row whose shadow reaches down into the viewport was cut by the near \
         edge of the search, which was not widened by what its children reach"
    );
}
