//! Momentum belongs to the gesture that ended.
//!
//! Both cases here dispatch real `Event::Scroll`s through a real container and
//! then run animation frames, which is what the loop does. What they assert is
//! that a frame in the middle of a gesture, or a frame long after one, does not
//! move the content: only the deltas the user actually produced do.

use std::time::{Duration, Instant};

use guido::layout::Flex;
use guido::prelude::*;

mod common;
use common::Harness;

struct H {
    surface: Harness,
}

impl H {
    fn new() -> Self {
        let view = container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column().spacing(8.0))
                    .padding(8.0)
                    .children((0..20).map(|_| container().width(120.0).height(24.0))),
            );

        Self {
            surface: Harness::laid_out(view, 200.0, 200.0),
        }
    }

    /// One scroll sample from any source, at a named moment.
    fn scroll_at(&mut self, delta: f32, source: ScrollSource, at: std::time::Instant) {
        self.surface
            .send_at(Event::scroll(100.0, 100.0, 0.0, delta, source), at);
    }

    /// The finger lifted at a named moment.
    fn scroll_end_at(&mut self, at: std::time::Instant) {
        self.surface.send_at(Event::scroll_end(100.0, 100.0), at);
    }

    /// Play a flick of six samples eight milliseconds apart, and lift.
    ///
    /// The instants are named rather than slept through: a flick's velocity is
    /// the distance over the gap between samples, so a test that sleeps is
    /// measuring the scheduler as much as the gesture — and on a loaded machine
    /// it measures something else again.
    ///
    /// Returns the moment of the lift, which is where whatever comes next
    /// starts counting from.
    fn flick_from(&mut self, source: ScrollSource, start: Instant) -> Instant {
        let mut at = start;
        for _ in 0..6 {
            self.scroll_at(10.0, source, at);
            at += Duration::from_millis(8);
        }
        self.scroll_end_at(at);
        at
    }

    /// Run `n` animation frames at sixty a second from `start`, as the loop does
    /// while anything is animating, and return the moment of the last one.
    fn frames_from(&mut self, n: usize, start: Instant) -> Instant {
        let mut at = start;
        for _ in 0..n {
            at += Duration::from_millis(16);
            self.surface.advance(at);
        }
        at
    }

    fn offset(&mut self) -> f32 {
        -self.surface.paint().children[0].local_transform.ty()
    }
}

/// A gap between two samples is what "scrolling slowly" is made of, not a
/// signal that the finger left. Frames taken during the gap must not fling the
/// list: after two 30px samples the content has moved 60px and no more.
#[test]
fn a_gap_inside_a_gesture_does_not_start_the_momentum() {
    let mut h = H::new();

    let t0 = Instant::now();
    h.scroll_at(30.0, ScrollSource::Finger, t0);
    let after = h.frames_from(10, t0 + Duration::from_millis(60));

    h.scroll_at(30.0, ScrollSource::Finger, after);
    h.frames_from(10, after + Duration::from_millis(60));

    let offset = h.offset();
    assert!(
        (offset - 60.0).abs() < 0.01,
        "content moved {offset}px for 60px of gesture: a gap between samples \
         was read as the end of the gesture"
    );
}

/// A gesture that never ended leaves nothing behind to be resumed. The finger
/// is still down as far as the library knows, so however long the surface sits
/// idle, the frame that eventually runs must not move the content.
#[test]
fn a_gesture_that_never_ended_is_not_resumed_by_a_later_frame() {
    let mut h = H::new();

    let t0 = Instant::now();
    h.scroll_at(30.0, ScrollSource::Finger, t0);
    let after_gesture = h.offset();

    // The surface goes idle: nothing advances for a while. Named rather than
    // slept, so "a while" is a number and the test costs nothing.
    //
    // Something unrelated then asks for an animation frame — re-entering the
    // container starts the scrollbar's hover expansion, which does exactly this.
    h.frames_from(10, t0 + Duration::from_millis(120));

    let offset = h.offset();
    assert!(
        (offset - after_gesture).abs() < 0.01,
        "content drifted from {after_gesture} to {offset} on a later frame: \
         a momentum nobody advanced was waiting instead of being over"
    );
}

/// And the other half: a gesture that does end carries on. The whole path,
/// from the samples through the end-of-gesture event to the frames that run
/// the momentum — which is what `wl_pointer.axis_stop` reaches.
#[test]
fn a_gesture_that_ended_carries_on_past_the_last_sample() {
    let mut h = H::new();

    let t0 = Instant::now();
    let mut at = t0;
    for _ in 0..6 {
        h.scroll_at(10.0, ScrollSource::Finger, at);
        at += Duration::from_millis(8);
    }
    let at_lift = h.offset();
    assert!(
        (at_lift - 60.0).abs() < 0.01,
        "the gesture itself moved {at_lift}px, not the 60px dispatched"
    );

    h.scroll_end_at(at);
    h.frames_from(60, at);

    let coasted = h.offset() - at_lift;
    assert!(
        coasted > 10.0,
        "the finger lifted after a 60px flick and the content coasted {coasted}px"
    );
}

/// A continuous source is a gesture: it is measured for a speed and it coasts.
/// A wheel is not — its steps have no duration to divide by, and a stop, if one
/// ever arrives, has nothing to release.
///
/// `is_continuous` is the line between them, and without this both sides of it
/// are unwatched: spelling it `matches!(self, Finger)` leaves the suite green.
#[test]
fn a_continuous_gesture_coasts_and_a_wheel_does_not() {
    let mut wheel = H::new();
    let lifted = wheel.flick_from(ScrollSource::Wheel, Instant::now());
    let at_lift = wheel.offset();
    wheel.frames_from(60, lifted);
    let wheel_coast = wheel.offset() - at_lift;
    assert!(
        wheel_coast.abs() < 0.01,
        "a wheel coasted {wheel_coast}px: it has no gesture to have a speed"
    );

    let mut continuous = H::new();
    let lifted = continuous.flick_from(ScrollSource::Continuous, Instant::now());
    let at_lift = continuous.offset();
    continuous.frames_from(60, lifted);
    let continuous_coast = continuous.offset() - at_lift;
    assert!(
        continuous_coast > 10.0,
        "a continuous gesture coasted {continuous_coast}px after a 60px flick"
    );
}

/// A momentum left half-run is not resumed at the speed it had. If the loop
/// goes idle with velocity left — the pointer wandered off, nothing asked for
/// a frame — the frame that eventually arrives finds a motion that spent the
/// whole idle slowing down, because friction is applied per elapsed
/// millisecond rather than per call (#340).
///
/// It used to be cancelled outright, by a rule about a number: nothing
/// advanced for 200ms was declared over. That rule is what #265's cross-clock
/// bug lived in, and what this replaces — the 304px resumption it was written
/// against is now a short tail, and a longer idle leaves nothing at all.
#[test]
fn a_momentum_left_half_run_is_not_resumed_at_the_speed_it_had() {
    let mut h = H::new();
    let lifted = h.flick_from(ScrollSource::Finger, Instant::now());

    let stopped = h.frames_from(5, lifted);
    let interrupted_at = h.offset();
    assert!(
        interrupted_at > 60.0,
        "the flick was coasting when it stopped"
    );

    // The loop goes idle mid-flight, and then something unrelated asks for an
    // animation frame — re-entering the container starts the scrollbar's hover
    // expansion, which does exactly this.
    h.frames_from(30, stopped + Duration::from_millis(400));

    let tail = h.offset() - interrupted_at;
    assert!(
        tail < interrupted_at * 0.25,
        "content carried on {tail}px after a 400ms idle, against the \
         {interrupted_at}px it had covered while it was being drawn: an \
         abandoned momentum has to arrive slowed, not where it left off"
    );
}

/// The same gesture, twice as fast, coasts further — and by how much is a
/// number, on any machine, with nothing asleep.
///
/// This could not be written before. A velocity is distance over time, and the
/// only time available was whatever had elapsed between two `sleep`s, so the
/// suite could ask that the content moved and never how fast. `Event::Scroll`
/// now arrives with the moment the compositor saw it, and a test can say what
/// that moment is.
#[test]
fn the_speed_of_a_flick_is_the_distance_over_the_time_it_took() {
    // Sixty pixels of finger, in six samples. The only difference between the
    // two gestures is how long they took.
    fn coast(sample_gap_ms: u64) -> f32 {
        let mut h = H::new();
        let t0 = Instant::now();
        for i in 1..=6u64 {
            h.scroll_at(
                10.0,
                ScrollSource::Finger,
                t0 + Duration::from_millis(i * sample_gap_ms),
            );
        }
        let lifted = h.offset();
        let at = t0 + Duration::from_millis(6 * sample_gap_ms);
        h.scroll_end_at(at);
        h.frames_from(60, at);
        h.offset() - lifted
    }

    let slow = coast(16);
    let fast = coast(8);

    assert!(
        slow > 0.0,
        "a flick has to coast at all before its speed can be compared, got {slow}"
    );
    assert!(
        fast > slow,
        "the same sixty pixels in half the time is twice the speed and has to \
         coast further: 8ms samples gave {fast}, 16ms samples {slow}"
    );
}

/// A 200×200 page: 40 of rows, then `middle`, 200 tall, then 600 more.
fn page_around(middle: Container) -> Harness {
    let view = container()
        .width(200.0)
        .height(200.0)
        .scroll(Scroll::vertical())
        .child(
            container()
                .layout(Flex::column())
                .child(container().width(200.0).height(40.0))
                .child(middle)
                .child(container().width(200.0).height(600.0)),
        );
    Harness::laid_out(view, 200.0, 200.0)
}

/// How far the page and the scroller in its middle have scrolled, down or
/// across, as painted: found by widget id, since a scroller culls what left
/// its viewport and a child's index in the render tree is not its index here.
/// The inner one is `None` once the page has scrolled it out of view.
fn offsets(page: &mut Harness) -> (f32, Option<f32>) {
    fn find(node: &guido::renderer::RenderNode, id: u64) -> Option<&guido::renderer::RenderNode> {
        if node.id == id {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let column = page.tree.get_children(page.root)[0];
    let inner = page.tree.get_children(column)[1];
    let inner_content = page.tree.get_children(inner)[0];
    let painted = page.paint();
    let column_at = find(&painted, column.as_u64()).expect("the page's column is painted");
    let inner_at = find(&painted, inner_content.as_u64())
        .map(|node| -(node.local_transform.tx() + node.local_transform.ty()));
    (-column_at.local_transform.ty(), inner_at)
}

/// Six 10px samples 8ms apart at (100, 150), the lift, and a second of frames.
/// Returns where the page and the inner scroller were at the lift, and how far
/// each went after it; the inner one is in view at the lift.
fn flick_over_the_middle(page: &mut Harness) -> ((f32, f32), (f32, Option<f32>)) {
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    let (page_at_lift, inner) = offsets(page);
    let lifted = (
        page_at_lift,
        inner.expect("the inner scroller is in view at the lift"),
    );
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let (page_after, inner_after) = offsets(page);
    (
        lifted,
        (page_after - lifted.0, inner_after.map(|at| at - lifted.1)),
    )
}

/// The end of a gesture belongs to the scroller the gesture moved. A flick
/// that scrolled a page over a carousel, which cannot scroll that way, coasts
/// as it does over anything else: the carousel under the pointer answered the
/// lift and the page never heard it (#841).
#[test]
fn a_flick_over_a_scroller_it_did_not_move_carries_the_page_on() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::horizontal())
            .child(container().width(400.0).height(200.0)),
    );
    let ((page_at_lift, _), (coasted, _)) = flick_over_the_middle(&mut page);
    assert!(
        (page_at_lift - 60.0).abs() < 0.01,
        "the samples scrolled the page {page_at_lift}, not 60"
    );
    assert!(coasted > 10.0, "the page coasted {coasted} past the lift");
}

/// A list inside the page that runs out of room partway through the flick
/// hands the rest of it to the page; the lift goes where the last sample did,
/// and the page coasts.
#[test]
fn a_flick_that_ran_a_list_to_its_end_carries_the_page_on() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(container().width(200.0).height(220.0)),
    );
    let ((page_at_lift, list_at_lift), (coasted, _)) = flick_over_the_middle(&mut page);
    assert!(
        (list_at_lift - 20.0).abs() < 0.01 && (page_at_lift - 40.0).abs() < 0.01,
        "the list took {list_at_lift} of the flick and the page {page_at_lift}, \
         not 20 and 40"
    );
    assert!(coasted > 10.0, "the page coasted {coasted} past the lift");
}

/// And a list that took the whole flick coasts itself, with the page left
/// where it was.
#[test]
fn a_flick_a_list_took_whole_carries_the_list_on() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(container().width(200.0).height(800.0)),
    );
    let ((page_at_lift, list_at_lift), (page_coasted, list_coasted)) =
        flick_over_the_middle(&mut page);
    assert!(
        page_at_lift.abs() < 0.01 && (list_at_lift - 60.0).abs() < 0.01,
        "the list took {list_at_lift} of the flick and the page {page_at_lift}"
    );
    let list_coasted = list_coasted.expect("the page did not move, so the list is in view");
    assert!(
        list_coasted > 10.0,
        "the list coasted {list_coasted} past the lift"
    );
    assert!(
        page_coasted.abs() < 0.01,
        "the page moved {page_coasted} after the lift"
    );
}

/// What a scroller remembers of a gesture is that gesture's. A list run to
/// its end by an earlier flick does not take the end of a later one that only
/// slid it under the pointer: the page that later flick moved coasts.
#[test]
fn a_scroller_an_earlier_flick_moved_does_not_take_a_later_ones_end() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(container().width(200.0).height(220.0)),
    );
    let mut at = Instant::now();
    // Two samples over the list take it to its end, 20 down.
    for _ in 0..2 {
        page.send_at(
            Event::scroll(100.0, 150.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..30 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let (page_before, list) = offsets(&mut page);
    assert!(
        page_before.abs() < 0.01 && list.is_some_and(|y| (y - 20.0).abs() < 0.01),
        "the first flick ran the list to its end and left the page: {page_before}, {list:?}"
    );

    // Over the row above the list, a second flick moves the page 20, which
    // brings the list under the pointer for the lift.
    for _ in 0..2 {
        page.send_at(
            Event::scroll(100.0, 25.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    let (page_at_lift, _) = offsets(&mut page);
    assert!(
        (page_at_lift - 20.0).abs() < 0.01,
        "the second flick scrolled the page {page_at_lift}, not 20"
    );
    page.send_at(Event::scroll_end(100.0, 25.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let coasted = offsets(&mut page).0 - page_at_lift;
    assert!(coasted > 10.0, "the page coasted {coasted} past the lift");
}

/// A last sample across a list's axis, fingers drifting as they lift, is no
/// sample of the list's: it keeps the glide the rest of the flick gave it.
#[test]
fn a_last_sample_across_a_lists_axis_leaves_its_glide() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(container().width(200.0).height(800.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    page.send_at(
        Event::scroll(100.0, 150.0, 3.0, 0.0, ScrollSource::Finger),
        at,
    );
    at += Duration::from_millis(8);
    let (_, list) = offsets(&mut page);
    let list_at_lift = list.expect("the list is in view");
    assert!(
        (list_at_lift - 60.0).abs() < 0.01,
        "the flick scrolled the list {list_at_lift}, not 60"
    );
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let list_after = offsets(&mut page)
        .1
        .expect("the page did not move, so the list is in view");
    assert!(
        list_after - list_at_lift > 10.0,
        "the list coasted {} past the lift",
        list_after - list_at_lift
    );
}

/// The same for a list that scrolls both ways but has no room across: a
/// sideways drift it cannot take is no sample of its, and it keeps its glide.
#[test]
fn a_last_sample_a_both_ways_list_has_no_room_for_leaves_its_glide() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::both())
            .child(container().width(150.0).height(800.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    page.send_at(
        Event::scroll(100.0, 150.0, 3.0, 0.0, ScrollSource::Finger),
        at,
    );
    at += Duration::from_millis(8);
    let list_at_lift = offsets(&mut page).1.expect("the list is in view");
    assert!(
        (list_at_lift - 60.0).abs() < 0.01,
        "the flick scrolled the list {list_at_lift}, not 60"
    );
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let list_after = offsets(&mut page)
        .1
        .expect("the page did not move, so the list is in view");
    assert!(
        list_after - list_at_lift > 10.0,
        "the list coasted {} past the lift",
        list_after - list_at_lift
    );
}

/// A flick that runs mostly across a carousel but still moves it is the
/// carousel's: it glides, as any flick that moved it does.
#[test]
fn a_diagonal_flick_that_moved_a_carousel_carries_it_on() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::horizontal())
            .child(container().width(800.0).height(200.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 5.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    let (page_at_lift, carousel) = offsets(&mut page);
    let carousel_at_lift = carousel.expect("the carousel is in view");
    assert!(
        (carousel_at_lift - 30.0).abs() < 0.01 && page_at_lift.abs() < 0.01,
        "the flick moved the carousel {carousel_at_lift} and the page {page_at_lift}, \
         not 30 and 0"
    );
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let carousel_after = offsets(&mut page).1.expect("the page did not move");
    assert!(
        carousel_after - carousel_at_lift > 5.0,
        "the carousel coasted {} past the lift",
        carousel_after - carousel_at_lift
    );
}

/// An end nobody's samples own does not stop a glide already running: a page
/// coasting from one flick keeps coasting when a stray end arrives over a
/// carousel nothing moved.
#[test]
fn an_end_nobody_owns_leaves_a_glide_running() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::horizontal())
            .child(container().width(400.0).height(200.0)),
    );
    // A short flick over the row above the carousel, lifted while that row is
    // still under the pointer, sets the page gliding.
    let mut at = Instant::now();
    for _ in 0..2 {
        page.send_at(
            Event::scroll(100.0, 10.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    page.send_at(Event::scroll_end(100.0, 10.0), at);
    for _ in 0..3 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let gliding = offsets(&mut page).0;
    assert!(gliding > 20.0, "the page is gliding: {gliding}");
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..30 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let coasted = offsets(&mut page).0 - gliding;
    assert!(
        coasted > 10.0,
        "the page coasted {coasted} after a stray end over the carousel"
    );
}

/// A finger that rested before it lifted threw nothing: the list it moved
/// does not glide on an end that comes long after its last sample, and the
/// page around it is not moved by that end either.
#[test]
fn an_end_long_after_the_last_sample_glides_nothing() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::vertical())
            .child(container().width(200.0).height(800.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    let lifted = offsets(&mut page);
    at += Duration::from_millis(300);
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let after = offsets(&mut page);
    assert_eq!(
        (after.0, after.1),
        (lifted.0, lifted.1),
        "a finger that rested 300ms before lifting threw nothing"
    );
}

/// A 200×200 page that scrolls across: 40 of columns, then `middle`, 200
/// wide, then 600 more.
fn page_across(middle: Container) -> Harness {
    let view = container()
        .width(200.0)
        .height(200.0)
        .scroll(Scroll::horizontal())
        .child(
            container()
                .layout(Flex::row())
                .child(container().width(40.0).height(200.0))
                .child(middle)
                .child(container().width(600.0).height(200.0)),
        );
    Harness::laid_out(view, 200.0, 200.0)
}

/// How far a page across and the scroller in its middle have scrolled, as
/// painted, found by widget id.
fn offsets_across(page: &mut Harness) -> (f32, Option<f32>) {
    fn find(node: &guido::renderer::RenderNode, id: u64) -> Option<&guido::renderer::RenderNode> {
        if node.id == id {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let row = page.tree.get_children(page.root)[0];
    let inner = page.tree.get_children(row)[1];
    let inner_content = page.tree.get_children(inner)[0];
    let painted = page.paint();
    let row_at = find(&painted, row.as_u64()).expect("the page's row is painted");
    let inner_at = find(&painted, inner_content.as_u64())
        .map(|node| -(node.local_transform.tx() + node.local_transform.ty()));
    (-row_at.local_transform.tx(), inner_at)
}

/// The same as a list run to its end, across: a strip inside a page that
/// scrolls across, with 20 of room, takes 20 of a sideways flick, the page
/// takes the rest, and the page coasts.
#[test]
fn a_flick_that_ran_a_strip_to_its_end_carries_the_page_across_on() {
    let mut page = page_across(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::horizontal())
            .child(container().width(220.0).height(200.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(150.0, 100.0, 10.0, 0.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    let (page_at_lift, strip) = offsets_across(&mut page);
    assert!(
        strip.is_some_and(|x| (x - 20.0).abs() < 0.01) && (page_at_lift - 40.0).abs() < 0.01,
        "the strip took {strip:?} of the flick and the page {page_at_lift}, not 20 and 40"
    );
    page.send_at(Event::scroll_end(150.0, 100.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let coasted = offsets_across(&mut page).0 - page_at_lift;
    assert!(coasted > 10.0, "the page coasted {coasted} past the lift");
}

/// A both-ways scroller with no room down: a last sample straight down is no
/// sample of its, and a sideways flick that moved it keeps it gliding.
#[test]
fn a_last_sample_a_both_ways_strip_has_no_room_for_leaves_its_glide() {
    let mut page = page_around(
        container()
            .width(200.0)
            .height(200.0)
            .scroll(Scroll::both())
            .child(container().width(800.0).height(150.0)),
    );
    let mut at = Instant::now();
    for _ in 0..6 {
        page.send_at(
            Event::scroll(100.0, 150.0, 10.0, 0.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    page.send_at(
        Event::scroll(100.0, 150.0, 0.0, 3.0, ScrollSource::Finger),
        at,
    );
    at += Duration::from_millis(8);
    let strip_at_lift = offsets(&mut page).1.expect("the strip is in view");
    assert!(
        (strip_at_lift - 60.0).abs() < 0.01,
        "the flick moved the strip {strip_at_lift}, not 60"
    );
    page.send_at(Event::scroll_end(100.0, 150.0), at);
    for _ in 0..60 {
        at += Duration::from_millis(16);
        page.advance(at);
    }
    let strip_after = offsets(&mut page).1;
    assert!(
        strip_after.is_some_and(|x| x - strip_at_lift > 10.0),
        "the strip coasted from {strip_at_lift} to {strip_after:?}"
    );
}
