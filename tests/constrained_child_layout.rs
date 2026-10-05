mod common;

use common::Harness;
use guido::prelude::*;

#[test]
fn exact_children_center_within_the_final_parent_extent() {
    for (declared, available, expected) in [
        (200.0, 100.0, 40.0),
        (100.0, 100.0, 40.0),
        (200.0, 200.0, 90.0),
    ] {
        let mut h = Harness::new(
            container()
                .width(declared)
                .height(declared)
                .layout(Flex::row().center())
                .child(container().width(20.0).height(20.0)),
        );
        h.tree.set_frame_instant(Some(std::time::Instant::now()));
        h.lay_out(available, available);
        let child = h.tree.get_children(h.root)[0];
        let bounds = h.tree.get_bounds(child).unwrap();
        assert_eq!((bounds.x, bounds.y), (expected, expected));
    }
}

#[test]
fn exact_length_bounds_apply_before_children_are_laid_out() {
    for (length, available, expected) in [
        (Length::exact(200.0).at_most(100.0), 300.0, 100.0),
        (Length::exact(20.0).at_least(100.0), 300.0, 100.0),
        (Length::exact(20.0).at_least(200.0), 100.0, 100.0),
        (fraction(2.0), 100.0, 100.0),
    ] {
        let mut h = Harness::new(
            container()
                .width(length)
                .height(length)
                .child(container().width(fill()).height(fill())),
        );
        h.tree.set_frame_instant(Some(std::time::Instant::now()));
        h.lay_out(available, available);
        let child = h.tree.get_children(h.root)[0];
        let bounds = h.tree.get_bounds(child).unwrap();
        assert_eq!((bounds.width, bounds.height), (expected, expected));
        let parent = h.tree.get_bounds(h.root).unwrap();
        assert_eq!((parent.width, parent.height), (expected, expected));
    }
}

#[test]
fn padding_and_reserved_gutter_are_subtracted_from_the_clamped_extent() {
    let mut h = Harness::new(
        container()
            .width(200.0)
            .height(100.0)
            .padding(10.0)
            .scroll(
                Scroll::vertical()
                    .width(6.0)
                    .margin(2.0)
                    .reserve_gutter(true),
            )
            .child(container().width(fill()).height(150.0)),
    );
    h.tree.set_frame_instant(Some(std::time::Instant::now()));
    h.lay_out(100.0, 100.0);
    let child = h.tree.get_children(h.root)[0];
    let bounds = h.tree.get_bounds(child).unwrap();
    assert_eq!((bounds.x, bounds.y, bounds.width), (10.0, 10.0, 70.0));
}

#[test]
fn a_clamped_exact_scroller_reaches_its_last_content_on_both_axes() {
    for horizontal in [false, true] {
        for declared in [100.0, 200.0] {
            let scroll = if horizontal {
                Scroll::horizontal()
            } else {
                Scroll::vertical()
            };
            let content = if horizontal {
                container().width(150.0).height(100.0)
            } else {
                container().width(100.0).height(150.0)
            };
            let mut h = Harness::new(
                container()
                    .width(declared)
                    .height(declared)
                    .scroll(scroll)
                    .child(content),
            );
            let now = std::time::Instant::now();
            h.tree.set_frame_instant(Some(now));
            h.lay_out(100.0, 100.0);
            let child = h.tree.get_children(h.root)[0];
            let (dx, dy) = if horizontal { (50.0, 0.0) } else { (0.0, 50.0) };
            h.send_at(Event::scroll(50.0, 50.0, dx, dy, ScrollSource::Wheel), now);
            let painted = h.paint();
            let content = painted
                .children
                .iter()
                .find(|node| node.id == child.as_u64())
                .unwrap();
            let offset = if horizontal {
                content.local_transform.tx()
            } else {
                content.local_transform.ty()
            };
            assert_eq!(
                offset, -50.0,
                "horizontal={horizontal}, declared={declared}"
            );
        }
    }
}
