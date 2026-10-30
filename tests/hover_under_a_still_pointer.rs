//! What moves under a still pointer moves the hover with it, once the frame
//! that moved it has been drawn (#842, #844).
//!
//! Hover is worked out on pointer events, and a wheel, a glide, a layout or a
//! child built in another's place changes what is under the pointer without
//! one. Once a frame is drawn, the hover and the cursor are worked out again
//! where the pointer is: the row that went hears that it lost the hover, and
//! the row now under the pointer that it has it, without waiting for the
//! pointer to move.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use guido::layout::Flex;
use guido::prelude::*;
use guido::renderer::RenderNode;

mod common;
use common::Harness;

/// The font the label whose size flickers is measured in, so that its widths
/// are the same on every machine.
#[cfg(feature = "testing")]
const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");

/// What each row's `on_hover` said, in the order it said it.
type Heard = Rc<RefCell<Vec<(usize, bool)>>>;

/// Four 100×50 rows, each saying when it is hovered and counting the pointer
/// moves it hears.
fn rows(heard: &Heard, moves: &Rc<Cell<u32>>) -> impl Iterator<Item = Box<dyn Widget>> {
    let (heard, moves) = (heard.clone(), moves.clone());
    (0..4).map(move |n| {
        let heard = heard.clone();
        let moves = moves.clone();
        container()
            .width(100.0)
            .height(50.0)
            .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
            .on_pointer_move(move |_, _| moves.set(moves.get() + 1))
            .into_any()
    })
}

/// A 100×100 scroller over the four rows, with the pointer resting on row 0.
fn list() -> (Harness, Heard, Rc<Cell<u32>>) {
    let heard: Heard = Rc::default();
    let moves = Rc::new(Cell::new(0));
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical())
        .child(
            container()
                .layout(Flex::column())
                .children(rows(&heard, &moves)),
        );
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 25.0));
    assert_eq!(heard.take(), [(0, true)], "the pointer rests on row 0");
    (h, heard, moves)
}

fn content_at(node: &RenderNode) -> f32 {
    node.children[0].local_transform.ty()
}

/// One wheel step of 50 under the still pointer moves the hover from row 0,
/// scrolled away, to row 1, now under it, once the frame is drawn.
#[test]
fn a_wheel_step_moves_the_hover_to_the_row_under_the_pointer() {
    let (mut h, heard, _) = list();
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    assert!(
        heard.borrow().is_empty(),
        "nothing is worked out before the frame"
    );
    assert_eq!(content_at(&h.frame()), -50.0, "the step moved the content");
    assert_eq!(heard.take(), [(0, false), (1, true)]);
}

/// A step that moves nothing, against the end of the list, changes nothing
/// under the pointer: it asks for nothing to be worked out again, and says
/// nothing.
#[test]
fn a_wheel_step_against_the_end_leaves_the_hover() {
    let (mut h, heard, _) = list();
    h.send(Event::scroll(50.0, 25.0, 0.0, -50.0, ScrollSource::Wheel));
    let root = h.root;
    assert!(!h.tree.recheck_hover(root), "nothing to work out again");
    assert_eq!(
        content_at(&h.frame()),
        0.0,
        "the list was already at its top"
    );
    assert!(heard.take().is_empty(), "nothing moved, so nothing changed");
}

/// The pointer did not move, so what hears the pointer's moves hears nothing,
/// though the hover moved.
#[test]
fn the_hover_moving_is_no_pointer_move() {
    let (mut h, heard, moves) = list();
    let before = moves.get();
    assert!(before > 0, "the pointer's own move was heard");
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    h.frame();
    assert_eq!(heard.take(), [(0, false), (1, true)], "the hover moved");
    assert_eq!(moves.get(), before, "and no move was heard");
}

/// With a button held, the frame leaves the hover where it is: a move then
/// would carry on as a drag. The release works it out, with no frame and no
/// move of its own.
#[test]
fn a_held_button_keeps_the_hover_until_it_is_released() {
    let (mut h, heard, _) = list();
    h.send(Event::mouse_down(50.0, 25.0, MouseButton::Right));
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    assert_eq!(content_at(&h.frame()), -50.0, "the step moved the content");
    assert!(
        heard.take().is_empty(),
        "nothing is worked out with a button held"
    );
    h.send(Event::mouse_up(50.0, 25.0, MouseButton::Right));
    assert_eq!(heard.take(), [(0, false), (1, true)], "the release did it");
}

/// A scroll a callback takes, moving no content, moves nothing under the
/// pointer: the frame after it works nothing out again, and what hears the
/// pointer's moves hears none.
#[test]
fn a_scroll_that_moves_no_content_moves_nothing_under_the_pointer() {
    let moves = Rc::new(Cell::new(0u32));
    let counted = moves.clone();
    // Inside a box, so that the pointer record keeps where the pointer is.
    let view = container().child(
        container()
            .width(100.0)
            .height(100.0)
            .on_scroll(|_, _, _| {})
            .on_pointer_move(move |_, _| counted.set(counted.get() + 1)),
    );
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 25.0));
    let before = moves.get();
    assert!(before > 0, "the pointer's move was heard");
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    let root = h.root;
    assert!(!h.tree.recheck_hover(root), "nothing to work out again");
    assert_eq!(
        moves.get(),
        before,
        "a scroll that moved nothing is no move"
    );
}

/// A finger lifted after scrolling the list is over nothing: its release
/// ends no mouse chord, so nothing is worked out where it was, and the leave
/// that follows says the finger is gone.
#[test]
fn a_finger_lifted_after_a_scroll_hovers_nothing() {
    let (mut h, heard, _) = list();
    h.send(Event::MouseLeave);
    heard.take();
    h.send(Event::finger_move(50.0, 75.0));
    h.send(Event::finger_down(50.0, 75.0));
    for y in [65.0, 55.0, 45.0, 35.0, 25.0] {
        h.send(Event::finger_move(50.0, y));
    }
    assert!(content_at(&h.paint()) < 0.0, "the finger scrolled the list");
    heard.take();
    // What the platform queues for a lift: the release, then the leave.
    h.send(Event::mouse_up(50.0, 25.0, MouseButton::Left));
    assert!(!h.tree.chord_ended(), "a finger's chord is no mouse's");
    h.send(Event::MouseLeave);
    assert!(
        heard.take().iter().all(|&(_, hovered)| !hovered),
        "nothing was hovered by the lift"
    );
}

/// A scrollbar drag goes on wherever the pointer goes, and the rows hear
/// none of it while the button holds; the release works the hover out where
/// the pointer let go.
#[test]
fn a_scrollbar_drag_moves_the_hover_at_its_release() {
    let (mut h, heard, _) = list();
    // The handle, 48 of the 96 track down the right edge.
    h.send(Event::mouse_move(95.0, 10.0));
    assert_eq!(heard.take(), [(0, false)], "on the scrollbar, off the rows");
    h.send(Event::mouse_down(95.0, 10.0, MouseButton::Left));
    // Off the scrollbar, over the rows, still dragging.
    h.send(Event::mouse_move(50.0, 34.0));
    assert_eq!(
        content_at(&h.paint()),
        -50.0,
        "the drag scrolled half the list"
    );
    assert!(heard.borrow().is_empty(), "nothing while the drag holds");
    h.send(Event::mouse_up(50.0, 34.0, MouseButton::Left));
    assert_eq!(heard.take(), [(1, true)], "row 1 is under it now");
}

/// A drag that brings the content back where it started moved nothing in
/// the end, but it took every move from the rows: the one under the pointer
/// where it lets go hears its hover at the release.
#[test]
fn a_scrollbar_drag_that_ends_where_it_began_hovers_the_row_it_ends_over() {
    let (mut h, heard, _) = list();
    h.send(Event::mouse_move(95.0, 10.0));
    assert_eq!(heard.take(), [(0, false)], "onto the scrollbar");
    h.send(Event::mouse_down(95.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(50.0, 34.0));
    assert_eq!(content_at(&h.frame()), -50.0, "the drag moved the content");
    h.send(Event::mouse_move(50.0, 10.0));
    assert_eq!(content_at(&h.frame()), 0.0, "and brought it back");
    h.send(Event::mouse_up(50.0, 10.0, MouseButton::Left));
    h.frame();
    assert_eq!(heard.take(), [(0, true)]);
}

/// The same across: four columns 50 wide, dragged by the handle along the
/// bottom edge.
#[test]
fn a_horizontal_scrollbar_drag_that_ends_where_it_began_hovers_the_column_it_ends_over() {
    let heard: Heard = Rc::default();
    let columns = (0..4).map({
        let heard = heard.clone();
        move |n| {
            let heard = heard.clone();
            container()
                .width(50.0)
                .height(100.0)
                .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
                .into_any()
        }
    });
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::horizontal())
        .child(container().layout(Flex::row()).children(columns));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    let content_across = |node: &RenderNode| node.children[0].local_transform.tx();
    h.send(Event::mouse_move(25.0, 50.0));
    h.send(Event::mouse_move(10.0, 95.0));
    assert_eq!(
        heard.take(),
        [(0, true), (0, false)],
        "onto the scrollbar, over column 0"
    );
    h.send(Event::mouse_down(10.0, 95.0, MouseButton::Left));
    h.send(Event::mouse_move(34.0, 50.0));
    assert_eq!(
        content_across(&h.frame()),
        -50.0,
        "the drag moved the content"
    );
    h.send(Event::mouse_move(10.0, 50.0));
    assert_eq!(content_across(&h.frame()), 0.0, "and brought it back");
    h.send(Event::mouse_up(10.0, 50.0, MouseButton::Left));
    h.frame();
    assert_eq!(heard.take(), [(0, true)]);
}

/// A press on the track jumps the handle under the pointer; the release
/// works its hover out there.
#[test]
fn a_track_press_hovers_the_handle_it_jumped_under_the_pointer_at_its_release() {
    let handle: Heard = Rc::default();
    let handle_heard = handle.clone();
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical().handle(move |c: Container| {
            let heard = handle_heard.clone();
            c.on_hover(move |hovered| heard.borrow_mut().push((0, hovered)))
        }))
        .child(container().width(100.0).height(400.0));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(95.0, 90.0));
    assert!(handle.take().is_empty(), "on the track, below the handle");
    h.send(Event::mouse_down(95.0, 90.0, MouseButton::Left));
    assert!(content_at(&h.paint()) < 0.0, "the press jumped the content");
    h.send(Event::mouse_up(95.0, 90.0, MouseButton::Left));
    assert_eq!(handle.take(), [(0, true)], "the handle is under it now");
}

/// A 100×100 scroller over ten 100×20 rows, with the pointer resting on row
/// 0: a step of 80 carries it further than the rows an event is offered to
/// around the point.
fn short_rows() -> (Harness, Heard) {
    let heard: Heard = Rc::default();
    let rows = (0..10).map({
        let heard = heard.clone();
        move |n| {
            let heard = heard.clone();
            container()
                .width(100.0)
                .height(20.0)
                .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
                .into_any()
        }
    });
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical())
        .child(container().layout(Flex::column()).children(rows));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 10.0));
    assert_eq!(heard.take(), [(0, true)], "the pointer rests on row 0");
    (h, heard)
}

/// A button held while the wheel carries the hovered row far off: the
/// release works the hover out, and the row it went with hears it went,
/// though the release was offered only where it is now.
#[test]
fn a_release_after_a_long_scroll_takes_the_hover_off_the_row_it_carried() {
    let (mut h, heard) = short_rows();
    h.send(Event::mouse_down(50.0, 10.0, MouseButton::Right));
    h.send(Event::scroll(50.0, 10.0, 0.0, 80.0, ScrollSource::Wheel));
    assert_eq!(content_at(&h.frame()), -80.0, "the step moved the content");
    assert!(heard.borrow().is_empty(), "nothing with the button down");
    h.send(Event::mouse_up(50.0, 10.0, MouseButton::Right));
    assert_eq!(heard.take(), [(0, false), (4, true)], "the release did it");
    h.send(Event::MouseLeave);
    assert_eq!(
        heard.take(),
        [(4, false)],
        "and nothing else is left hovered"
    );
}

/// A click between a long wheel step and its frame: the press works no hover
/// out, and its release, which does, still owes the row the step carried off
/// the move that takes its hover away.
#[test]
fn a_click_after_a_long_scroll_takes_the_hover_off_the_row_it_carried() {
    let (mut h, heard) = short_rows();
    h.send(Event::scroll(50.0, 10.0, 0.0, 80.0, ScrollSource::Wheel));
    h.send(Event::mouse_down(50.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_up(50.0, 10.0, MouseButton::Left));
    assert_eq!(content_at(&h.frame()), -80.0, "the step moved the content");
    assert_eq!(heard.take(), [(0, false), (4, true)]);
    h.send(Event::MouseLeave);
    assert_eq!(
        heard.take(),
        [(4, false)],
        "and nothing else is left hovered"
    );
}

/// A press after a long wheel step, dragged out of the list over a header
/// above it and let go there: the press's route is what it reached, and the
/// rows the step carried off are not on it, so nothing in the list is left
/// hovered once the pointer has gone.
#[test]
fn a_drag_out_of_a_scrolled_list_leaves_no_row_hovered() {
    let heard: Heard = Rc::default();
    let rows = (0..10).map({
        let heard = heard.clone();
        move |n| {
            let heard = heard.clone();
            container()
                .width(100.0)
                .height(20.0)
                .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
                .into_any()
        }
    });
    let view = container()
        .layout(Flex::column())
        .child(container().width(100.0).height(50.0))
        .child(
            container()
                .width(100.0)
                .height(100.0)
                .scroll(Scroll::vertical())
                .child(container().layout(Flex::column()).children(rows)),
        );
    let mut h = Harness::laid_out(view, 100.0, 150.0);
    h.send(Event::mouse_move(50.0, 60.0));
    h.send(Event::scroll(50.0, 60.0, 0.0, 80.0, ScrollSource::Wheel));
    h.send(Event::mouse_down(50.0, 60.0, MouseButton::Left));
    h.send(Event::mouse_move(50.0, 5.0));
    h.send(Event::mouse_up(50.0, 5.0, MouseButton::Left));
    h.frame();
    h.send(Event::MouseLeave);
    let mut hovered = std::collections::BTreeMap::new();
    for (row, now) in heard.take() {
        hovered.insert(row, now);
    }
    let left: Vec<usize> = hovered
        .into_iter()
        .filter_map(|(row, now)| now.then_some(row))
        .collect();
    assert!(left.is_empty(), "rows left hovered: {left:?}");
}

/// Two wheel steps before a frame — a fast spin — still take the hover off
/// the row the first one scrolled away, though the second was offered where
/// that row no longer is: a scroll works no hover out, and leaves what the
/// hover is owed alone.
#[test]
fn two_wheel_steps_before_a_frame_take_the_hover_off_the_first_row() {
    let (mut h, heard) = short_rows();
    h.send(Event::scroll(50.0, 10.0, 0.0, 40.0, ScrollSource::Wheel));
    h.send(Event::scroll(50.0, 10.0, 0.0, 40.0, ScrollSource::Wheel));
    assert_eq!(content_at(&h.frame()), -80.0, "two steps of forty");
    assert_eq!(heard.take(), [(0, false), (4, true)]);
    h.send(Event::MouseLeave);
    assert_eq!(
        heard.take(),
        [(4, false)],
        "and nothing else is left hovered"
    );
}

/// The keyboard scrolls a list under a still pointer too: an arrow moves it a
/// line, forty, and the hover goes to the row under the pointer then.
#[test]
fn a_key_that_scrolls_moves_the_hover() {
    let heard: Heard = Rc::default();
    let rows = (0..4).map(|n| {
        let heard = heard.clone();
        container()
            .width(100.0)
            .height(50.0)
            .focusable(n == 0)
            .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
            .into_any()
    });
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical())
        .child(container().layout(Flex::column()).children(rows));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 25.0));
    // A press on row 0 gives it the focus, and the pointer rests there.
    h.click(50.0, 25.0);
    assert_eq!(heard.take(), [(0, true)], "the pointer rests on row 0");
    h.send(Event::KeyDown {
        key: Key::Down,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(content_at(&h.frame()), -40.0, "the arrow scrolled a line");
    assert_eq!(heard.take(), [(0, false), (1, true)]);
}

/// Through the loop, which is where the frame is.
#[cfg(feature = "testing")]
mod through_the_loop {
    use super::*;
    use common::headless;
    use guido::layout::ZStack;
    use guido::testing::Headless;
    use std::time::{Duration, Instant};

    /// The pointer arrives at `(x, y)` on `id`, and the frame that follows is
    /// drawn.
    fn rest_at(app: &mut Headless, id: SurfaceId, x: f32, y: f32) {
        let now = Instant::now();
        app.event_at(id, Event::mouse_enter(x, y), now);
        app.event_at(id, Event::mouse_move(x, y), now);
        app.step();
    }

    /// A child removed under the pointer with no exit hears nothing as it
    /// goes, and the one built in its place hears that it is hovered once the
    /// frame is drawn, without waiting for the pointer to move (#844).
    #[test]
    fn a_child_built_under_the_pointer_hears_its_hover() {
        let Some(mut app) = headless() else { return };
        let heard: Heard = Rc::default();
        let step = create_signal(0usize);
        let child_heard = heard.clone();
        let bar = app.surface(SurfaceConfig::new().width(200).height(100), move || {
            let heard = child_heard.clone();
            container().child(move || {
                let n = step.get();
                let heard = heard.clone();
                container()
                    .width(100.0)
                    .height(50.0)
                    .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
            })
        });
        app.configure(bar, 200, 100, 1.0);
        app.step();
        rest_at(&mut app, bar, 50.0, 25.0);
        step.set(1);
        app.step();
        assert_eq!(heard.take(), [(0, true), (1, true)]);
    }

    /// A container hidden under a still pointer hears its hover end once the
    /// frame that hides it is drawn, not at the pointer's next move.
    #[test]
    fn a_container_hidden_under_the_pointer_hears_its_leave_at_the_next_frame() {
        let Some(mut app) = headless() else { return };
        let heard: Heard = Rc::default();
        let shown = create_signal(true);
        let card_heard = heard.clone();
        let bar = app.surface(SurfaceConfig::new().width(200).height(100), move || {
            let heard = card_heard.clone();
            container().child(
                container()
                    .width(100.0)
                    .height(50.0)
                    .visible(shown)
                    .on_hover(move |hovered| heard.borrow_mut().push((0, hovered))),
            )
        });
        app.configure(bar, 200, 100, 1.0);
        app.step();
        rest_at(&mut app, bar, 50.0, 25.0);
        assert_eq!(heard.take(), [(0, true)], "the pointer rests on the card");
        shown.set(false);
        app.step();
        assert_eq!(heard.take(), [(0, false)]);
    }

    /// A hover style that lifts its box off a pointer resting near its edge
    /// draws the box elsewhere, and nothing under the pointer moved: the
    /// hover stays, and the loop goes to sleep. Worked out again from where
    /// the box was drawn, it would take itself back on the next frame and give
    /// itself back on the one after, for as long as the pointer rested there.
    #[test]
    fn a_hover_style_that_moves_its_box_settles() {
        let Some(mut app) = headless() else { return };
        let heard = Rc::new(RefCell::new(Vec::new()));
        let card_heard = heard.clone();
        let bar = app.surface(SurfaceConfig::new().width(200).height(100), move || {
            let heard = card_heard.clone();
            container().padding(20.0).child(
                container()
                    .width(100.0)
                    .height(50.0)
                    .background(Color::RED)
                    .when_hovered(|s| s.translate(Translate::new(0.0, -4.0)))
                    .on_hover(move |hovered| heard.borrow_mut().push(hovered)),
            )
        });
        app.configure(bar, 200, 100, 1.0);
        app.step();
        // The box spans 20..70 at rest and 16..66 lifted: 68 is inside it only
        // where it rests. One move, because the enter at the same point would
        // be a second event, asked against the lifted box.
        app.event_at(bar, Event::mouse_move(50.0, 68.0), Instant::now());
        app.step();
        let mut at = Instant::now();
        for _ in 0..10 {
            at += Duration::from_millis(16);
            app.step_at(at);
        }
        assert_eq!(heard.take(), [true], "hovered once, and left so");
        assert_eq!(app.next_wait(), None, "and the loop sleeps");
    }

    /// A hover style that changes the size moves the edge across a pointer
    /// resting near it, and that is a layout like any other: once the frame
    /// draws the label smaller, the hover is worked out again, the pointer is
    /// outside, the label grows back, and the pointer is inside again — every
    /// frame for as long as it rests there, the edge flicker a CSS `:hover`
    /// that resizes its element has. Where the smaller label still covers the
    /// pointer, the hover stays.
    #[test]
    fn a_hover_style_that_shrinks_its_box_flickers_at_its_edge() {
        let hovered_at = |x: f32| {
            // Inside, before each application: one that goes takes the fonts
            // it was given with it.
            guido::load_font(FONT.to_vec());
            let mut app = headless()?;
            let heard = Rc::new(RefCell::new(Vec::new()));
            let button_heard = heard.clone();
            let bar = app.surface(SurfaceConfig::new().width(300).height(60), move || {
                let heard = button_heard.clone();
                container().child(
                    container()
                        .on_hover(move |hovered| heard.borrow_mut().push(hovered))
                        .child(
                            text("Hover me please")
                                .font_family(FontFamily::name("DejaVu Sans Mono"))
                                .font_size(24.0)
                                .when_hovered(|s| s.font_size(12.0)),
                        ),
                )
            });
            app.configure(bar, 300, 60, 1.0);
            app.step();
            let mut at = Instant::now();
            app.event_at(bar, Event::mouse_move(x, 10.0), at);
            app.step_at(at);
            for _ in 0..4 {
                at += Duration::from_millis(16);
                app.step_at(at);
            }
            let waits = app.next_wait();
            Some((heard.take(), waits))
        };
        // Fifteen characters of DejaVu Sans Mono: about 217 wide at 24 and
        // 108 at 12.
        let Some((settled, waits)) = hovered_at(40.0) else {
            return;
        };
        assert_eq!(settled, [true], "inside it at either size");
        assert_eq!(waits, None, "and the loop sleeps");
        let Some((flicker, _)) = hovered_at(160.0) else {
            return;
        };
        assert_eq!(
            flicker,
            [true, false, true, false, true, false],
            "inside it only at its full size"
        );
    }

    /// An `on_hover` that makes its own container smaller flickers the same
    /// way at the edge it pulls in: a box 50 tall that is 40 while hovered,
    /// under a pointer resting at 45.
    #[test]
    fn an_on_hover_that_shrinks_its_box_flickers_at_its_edge() {
        let Some(mut app) = headless() else { return };
        let heard = Rc::new(RefCell::new(Vec::new()));
        let box_heard = heard.clone();
        let bar = app.surface(SurfaceConfig::new().width(200).height(100), move || {
            let heard = box_heard.clone();
            let small = create_signal(false);
            container().child(
                container()
                    .width(100.0)
                    .height(move || if small.get() { 40.0 } else { 50.0 })
                    .on_hover(move |hovered| {
                        small.set(hovered);
                        heard.borrow_mut().push(hovered);
                    }),
            )
        });
        app.configure(bar, 200, 100, 1.0);
        app.step();
        let mut at = Instant::now();
        app.event_at(bar, Event::mouse_move(50.0, 45.0), at);
        app.step_at(at);
        for _ in 0..4 {
            at += Duration::from_millis(16);
            app.step_at(at);
        }
        assert_eq!(heard.take(), [true, false, true, false, true, false]);
    }

    /// An `on_hover` that shows something on top of its own container takes
    /// the pointer off it, and the hover is worked out again once the frame
    /// has drawn the cover: the container leaves, the cover goes, the
    /// container is under the pointer again — every frame for as long as it
    /// rests there, like a style that shrinks the box.
    #[test]
    fn an_on_hover_that_shows_a_cover_over_its_box_flickers() {
        let Some(mut app) = headless() else { return };
        let heard = Rc::new(RefCell::new(Vec::new()));
        let card_heard = heard.clone();
        let bar = app.surface(SurfaceConfig::new().width(100).height(100), move || {
            let heard = card_heard.clone();
            let shown = create_signal(false);
            container()
                .width(100.0)
                .height(50.0)
                .layout(ZStack::new())
                .child(
                    container()
                        .width(100.0)
                        .height(50.0)
                        .on_hover(move |hovered| {
                            shown.set(hovered);
                            heard.borrow_mut().push(hovered);
                        }),
                )
                .child(move || {
                    shown
                        .get()
                        .then(|| container().width(100.0).height(50.0).background(Color::RED))
                })
        });
        app.configure(bar, 100, 100, 1.0);
        app.step();
        let mut at = Instant::now();
        app.event_at(bar, Event::mouse_move(50.0, 25.0), at);
        app.step_at(at);
        for _ in 0..4 {
            at += Duration::from_millis(16);
            app.step_at(at);
        }
        assert_eq!(heard.take(), [true, false, true, false, true, false]);
        assert!(app.next_wait().is_some(), "and the loop does not sleep");
    }
}
