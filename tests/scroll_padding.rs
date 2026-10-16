//! A scroller with padding of its own stops where its content and padding end.
//!
//! The padding is inside the scroller and scrolls with the content, so a
//! scroller 100 tall with 20 of padding over 300 of content has 300 + 40 - 100
//! = 240 to scroll — the same as when the padding is on its child (#721).
//! Where a case is measured both ways, the two have to agree.

use std::time::Instant;

use guido::prelude::*;

mod common;
use common::Harness;

/// How far a scroller 200 × 100 moves under a wheel event far larger than
/// there is to scroll, along `x` or `y`: with `padding` on the scroller, or on
/// a child wrapped around `content` (`on_scroller == false`).
fn scrolled(scroll: Scroll, padding: f32, content: (f32, f32), on_scroller: bool) -> (f32, f32) {
    let inner = container().width(content.0).height(content.1);
    let view = if on_scroller {
        container()
            .width(200.0)
            .height(100.0)
            .padding(padding)
            .scroll(scroll)
            .child(inner)
    } else {
        container()
            .width(200.0)
            .height(100.0)
            .scroll(scroll)
            .child(container().padding(padding).child(inner))
    };
    let mut h = Harness::laid_out(view, 200.0, 100.0);
    let at = |h: &mut Harness| {
        let t = h.paint().children[0].local_transform;
        (t.tx(), t.ty())
    };
    let before = at(&mut h);
    h.send_at(
        Event::scroll(100.0, 50.0, 1000.0, 1000.0, ScrollSource::Wheel),
        Instant::now(),
    );
    let after = at(&mut h);
    (before.0 - after.0, before.1 - after.1)
}

/// The same scroller measured both ways, which have to agree.
fn both_ways(scroll: impl Fn() -> Scroll, padding: f32, content: (f32, f32)) -> (f32, f32) {
    let on_scroller = scrolled(scroll(), padding, content, true);
    let on_child = scrolled(scroll(), padding, content, false);
    assert_eq!(
        on_scroller, on_child,
        "padding on the scroller or on its child"
    );
    on_scroller
}

#[test]
fn a_padded_scroller_stops_where_its_content_and_padding_end() {
    // 300 + 40 in a box 100 tall, and in a box 200 wide.
    assert_eq!(
        both_ways(Scroll::vertical, 20.0, (100.0, 300.0)),
        (0.0, 240.0)
    );
    assert_eq!(
        both_ways(Scroll::horizontal, 20.0, (300.0, 50.0)),
        (140.0, 0.0)
    );
    // Scrolled both ways with no gutter, 340 in a box 200 × 100.
    assert_eq!(
        both_ways(
            || Scroll::both().reserve_gutter(false),
            20.0,
            (300.0, 300.0)
        ),
        (140.0, 240.0)
    );
}

/// Padding wider than the box leaves no room inside it, and the whole of the
/// padding still scrolls: 300 + 140 in a box 100 tall.
#[test]
fn padding_larger_than_the_box_still_scrolls_to_its_end() {
    assert_eq!(
        both_ways(Scroll::vertical, 70.0, (100.0, 300.0)),
        (0.0, 340.0)
    );
}

/// Content and padding that fit the box leave nothing to scroll.
#[test]
fn a_padded_scroller_whose_content_fits_does_not_scroll() {
    assert_eq!(both_ways(Scroll::vertical, 20.0, (100.0, 50.0)), (0.0, 0.0));
    assert_eq!(
        scrolled(
            Scroll::vertical().fill_viewport(true),
            20.0,
            (100.0, 10.0),
            true
        ),
        (0.0, 0.0),
        "content filling the viewport fits it, padding and all"
    );
}

/// A reserved gutter is room the content never has, on the axis across the
/// scroll: scrolled both ways in a box 200 × 100, the 10 of each gutter is
/// still there to scroll past, 340 in 190 × 90.
#[test]
fn a_padded_scroller_with_a_gutter_stops_where_its_content_and_padding_end() {
    assert_eq!(
        both_ways(Scroll::both, 20.0, (300.0, 300.0)),
        (150.0, 250.0)
    );
}

/// A scroller that sizes to its content, scrolled both ways with its gutters
/// reserved, has nothing to scroll when that content fits what it is offered.
#[test]
fn a_padded_scroller_sized_to_its_content_does_not_scroll() {
    let view = container()
        .padding(20.0)
        .scroll(Scroll::both())
        .child(container().width(100.0).height(40.0));
    let mut h = Harness::laid_out(view, 200.0, 100.0);
    let at = |h: &mut Harness| {
        let t = h.paint().children[0].local_transform;
        (t.tx(), t.ty())
    };
    let before = at(&mut h);
    h.send_at(
        Event::scroll(50.0, 30.0, 1000.0, 1000.0, ScrollSource::Wheel),
        Instant::now(),
    );
    assert_eq!(at(&mut h), before);
}

/// The scrollbar's thumb is sized from the content and the box it is seen
/// through, so it is as long with the padding on the scroller as with the
/// padding on its child — longer than the 20 a thumb is never shorter than.
#[test]
fn a_padded_scroller_sizes_its_thumb_as_its_child_would() {
    let thumb = |on_scroller: bool| {
        let inner = container().width(100.0).height(300.0);
        let view = if on_scroller {
            container()
                .width(200.0)
                .height(100.0)
                .padding(20.0)
                .scroll(Scroll::vertical())
                .child(inner)
        } else {
            container()
                .width(200.0)
                .height(100.0)
                .scroll(Scroll::vertical())
                .child(container().padding(20.0).child(inner))
        };
        let rects = Harness::laid_out(view, 200.0, 100.0).painted_rects();
        assert_eq!(rects.len(), 2, "the track, then the thumb on it");
        rects[1]
    };
    assert_eq!(thumb(true), thumb(false));
    assert!(thumb(true).height > 20.0, "a thumb past its minimum");
}
