//! Page Up and Page Down page the nearest scroller around the focus (#747).
//!
//! The key goes down the focus path as every key does; once the focused widget
//! and the scroller's own `on_key_down` have let it through, the scroller moves
//! by one viewport height, and takes the key only if it moved. At its end it
//! lets the key go, so the next scroller out pages instead — Chromium's scroll
//! chain. With nothing focused, nothing pages.

#![cfg(feature = "testing")]

use guido::layout::Flex;
use guido::prelude::*;
use guido::reactive::focus::clear_focus;
use guido::renderer::RenderNode;

mod common;
use common::Harness;

use EventResponse::{Handled, Ignored};

/// `count` rows of fifty, the first one focusable if `focus_first`.
fn rows(count: usize, focus_first: bool) -> Container {
    container()
        .layout(Flex::column())
        .children((0..count).map(move |i| {
            container()
                .width(fill())
                .height(50.0)
                .focusable(focus_first && i == 0)
        }))
}

/// A 200 viewport over twenty-one rows: 1050 of content, so it ends at 850,
/// which is not a whole number of pages from either end.
const CONTENT: f32 = 1050.0;

fn list() -> Container {
    container()
        .width(200.0)
        .height(200.0)
        .scroll(Scroll::vertical())
}

fn press_with(surface: &mut Harness, key: Key, modifiers: Modifiers) -> EventResponse {
    surface.send_at(
        Event::KeyDown {
            key,
            modifiers,
            repeat: false,
        },
        std::time::Instant::now(),
    )
}

fn press(surface: &mut Harness, key: Key) -> EventResponse {
    press_with(surface, key, Modifiers::default())
}

/// How far the content `content_height` tall, wherever it was painted, has
/// been scrolled — `None` if it was not painted.
fn content_offset(node: &RenderNode, content_height: f32) -> Option<f32> {
    if node.bounds.height == content_height {
        return Some(-node.local_transform.ty());
    }
    node.children
        .iter()
        .find_map(|child| content_offset(child, content_height))
}

fn offset(surface: &mut Harness) -> Option<f32> {
    content_offset(&surface.paint(), CONTENT)
}

/// Page Down six times, then Page Up six times, and the offsets after each.
fn page_down_then_up(surface: &mut Harness) -> Vec<Option<f32>> {
    let mut seen = Vec::new();
    for key in [Key::PageDown, Key::PageUp] {
        for _ in 0..6 {
            press(surface, key);
            seen.push(offset(surface));
        }
    }
    seen
}

const DOWN_AND_BACK: [Option<f32>; 12] = [
    // Down, and stopped at the end.
    Some(200.0),
    Some(400.0),
    Some(600.0),
    Some(800.0),
    Some(850.0),
    Some(850.0),
    // Up, and stopped at the top.
    Some(650.0),
    Some(450.0),
    Some(250.0),
    Some(50.0),
    Some(0.0),
    Some(0.0),
];

#[test]
fn a_focused_scroller_pages_by_its_viewport_and_stops_at_both_ends() {
    let view = list().focusable(true).child(rows(21, false));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    assert_eq!(page_down_then_up(&mut surface), DOWN_AND_BACK);
}

#[test]
fn a_scroller_pages_for_the_focused_row_inside_it() {
    clear_focus();
    let mut surface = Harness::laid_out(list().child(rows(21, true)), 200.0, 200.0);
    press(&mut surface, Key::Tab);

    assert_eq!(page_down_then_up(&mut surface), DOWN_AND_BACK);
}

#[test]
fn a_scroller_takes_the_key_only_while_it_moves() {
    let view = list().focusable(true).child(rows(5, false));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    assert_eq!(press(&mut surface, Key::PageUp), Ignored);
    assert_eq!(press(&mut surface, Key::PageDown), Handled);
    assert_eq!(
        press(&mut surface, Key::PageDown),
        Ignored,
        "at its end the scroller lets the key go on"
    );
}

#[test]
fn a_scroller_at_its_end_lets_the_next_one_out_page() {
    clear_focus();
    // A page of 300 over 640: 260 of rows, then a list of 100 over 250 of its
    // own, then 280 more. Placed so the list is in view at every offset the
    // page can reach.
    let view = container()
        .width(200.0)
        .height(300.0)
        .scroll(Scroll::vertical())
        .child(
            container()
                .layout(Flex::column())
                .child(container().width(fill()).height(260.0))
                .child(
                    container()
                        .width(fill())
                        .height(100.0)
                        .scroll(Scroll::vertical())
                        .child(rows(5, true)),
                )
                .child(container().width(fill()).height(280.0)),
        );
    let mut surface = Harness::laid_out(view, 200.0, 300.0);
    press(&mut surface, Key::Tab);

    let mut seen = Vec::new();
    for _ in 0..5 {
        let response = press(&mut surface, Key::PageDown);
        let painted = surface.paint();
        seen.push((
            response,
            content_offset(&painted, 250.0),
            content_offset(&painted, 640.0),
        ));
    }

    assert_eq!(
        seen,
        [
            (Handled, Some(100.0), Some(0.0)),
            (Handled, Some(150.0), Some(0.0)),
            (Handled, Some(150.0), Some(300.0)),
            (Handled, Some(150.0), Some(340.0)),
            (Ignored, Some(150.0), Some(340.0)),
        ],
        "the list pages to its end, then the page around it does, and at the \
         end of both the key goes on"
    );
}

#[test]
fn with_nothing_focused_nothing_pages() {
    clear_focus();
    // A listener inside the scroller that lets the key through: the key goes
    // down to it, past the scroller, and the scroller must not page for it.
    let view = list().child(rows(21, false).on_key_down(|_, _, _| Ignored));
    let mut surface = Harness::laid_out(view, 200.0, 200.0);

    assert_eq!(press(&mut surface, Key::PageDown), Ignored);
    assert_eq!(offset(&mut surface), Some(0.0));
}

#[test]
fn the_scrollers_own_key_handler_hears_the_key_first() {
    let view = list()
        .focusable(true)
        .on_key_down(|key, _, _| {
            if key == Key::PageDown {
                Handled
            } else {
                Ignored
            }
        })
        .child(rows(21, false));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    press(&mut surface, Key::PageDown);

    assert_eq!(
        offset(&mut surface),
        Some(0.0),
        "the handler took Page Down, so the scroller did not page"
    );
}

#[test]
fn a_shortcut_held_with_page_down_is_not_a_page() {
    let view = list().focusable(true).child(rows(21, false));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    let held = Modifiers::default();
    for modifiers in [
        Modifiers { ctrl: true, ..held },
        Modifiers { alt: true, ..held },
        Modifiers { logo: true, ..held },
    ] {
        assert_eq!(
            press_with(&mut surface, Key::PageDown, modifiers),
            Ignored,
            "{modifiers:?}"
        );
    }
    assert_eq!(offset(&mut surface), Some(0.0));
}
