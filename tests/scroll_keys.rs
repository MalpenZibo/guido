//! The keys scroll the nearest scroller around the focus: Page Up and Page
//! Down by a viewport (#747), the arrows by a line, and Home and End to either
//! end (#765).
//!
//! The key goes down the focus path as every key does; once the focused widget
//! and the scroller's own `on_key_down` have let it through, the scroller
//! moves, and takes the key only if it moved. At its end it lets the key go, so
//! the next scroller out moves instead — Chromium's scroll chain. A line is
//! forty pixels, one wheel notch, as in Chromium. With nothing focused, nothing
//! scrolls.

#![cfg(feature = "testing")]

use guido::layout::Flex;
use guido::prelude::*;
use guido::reactive::focus::clear_focus;
use guido::renderer::RenderNode;

mod common;
use common::Harness;

use EventResponse::{Handled, Ignored};

/// One arrow press, as one wheel notch.
const LINE: f32 = 40.0;

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
/// which is not a whole number of pages or lines from either end.
const CONTENT: f32 = 1050.0;
const END: f32 = CONTENT - 200.0;

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

/// How far the content of `width` by `height` — `None` for either is any —
/// wherever it was painted, has been scrolled along each axis; `None` if it
/// was not painted.
fn content_offset(
    node: &RenderNode,
    (width, height): (Option<f32>, Option<f32>),
) -> Option<(f32, f32)> {
    let fits = |want: Option<f32>, got: f32| want.is_none_or(|want| want == got);
    if fits(width, node.bounds.width) && fits(height, node.bounds.height) {
        return Some((-node.local_transform.tx(), -node.local_transform.ty()));
    }
    node.children
        .iter()
        .find_map(|child| content_offset(child, (width, height)))
}

/// How far the content `height` tall has been scrolled down.
fn scrolled_down(node: &RenderNode, height: f32) -> Option<f32> {
    content_offset(node, (None, Some(height))).map(|(_, y)| y)
}

fn offset(surface: &mut Harness) -> Option<f32> {
    scrolled_down(&surface.paint(), CONTENT)
}

/// Press each key in turn: what it answered, and what `read` saw painted
/// after it.
fn presses_reading<T>(
    surface: &mut Harness,
    keys: &[Key],
    read: impl Fn(&RenderNode) -> T,
) -> Vec<(EventResponse, T)> {
    keys.iter()
        .map(|&key| (press(surface, key), read(&surface.paint())))
        .collect()
}

/// Press each key in turn: what it answered, and where the list was after it.
fn presses(surface: &mut Harness, keys: &[Key]) -> Vec<(EventResponse, Option<f32>)> {
    presses_reading(surface, keys, |painted| scrolled_down(painted, CONTENT))
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
    Some(END),
    Some(END),
    // Up, and stopped at the top.
    Some(650.0),
    Some(450.0),
    Some(250.0),
    Some(50.0),
    Some(0.0),
    Some(0.0),
];

/// A focusable list over twenty-one rows, with the focus.
fn focused_list() -> Harness {
    Harness::focused(list().focusable(true).child(rows(21, false)), 200.0, 200.0)
}

/// A page of 300 over 640: 260 of rows, then a list of 100 over 250 of its
/// own with the focus on its first row, then 280 more. Placed so the list is
/// in view at every offset the page can reach.
fn a_list_in_a_page() -> Harness {
    clear_focus();
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
    surface
}

/// Where the list and the page around it were painted.
fn list_and_page(painted: &RenderNode) -> (Option<f32>, Option<f32>) {
    (scrolled_down(painted, 250.0), scrolled_down(painted, 640.0))
}

#[test]
fn a_focused_scroller_pages_by_its_viewport_and_stops_at_both_ends() {
    let mut surface = focused_list();

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
    let mut surface = a_list_in_a_page();

    assert_eq!(
        presses_reading(&mut surface, &[Key::PageDown; 5], list_and_page),
        [
            (Handled, (Some(100.0), Some(0.0))),
            (Handled, (Some(150.0), Some(0.0))),
            (Handled, (Some(150.0), Some(300.0))),
            (Handled, (Some(150.0), Some(340.0))),
            (Ignored, (Some(150.0), Some(340.0))),
        ],
        "the list pages to its end, then the page around it does, and at the \
         end of both the key goes on"
    );
}

#[test]
fn an_arrow_moves_a_focused_scroller_by_one_line() {
    let mut surface = focused_list();

    assert_eq!(
        presses(
            &mut surface,
            &[Key::Down, Key::Down, Key::Down, Key::Up, Key::Up, Key::Up]
        ),
        [
            (Handled, Some(LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(3.0 * LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(LINE)),
            (Handled, Some(0.0)),
        ]
    );
}

#[test]
fn home_and_end_go_to_either_end_and_stop_there() {
    let mut surface = focused_list();

    assert_eq!(
        presses(
            &mut surface,
            &[
                Key::Home,
                Key::Up,
                Key::End,
                Key::End,
                Key::Down,
                Key::Up,
                Key::Home,
                Key::Up,
            ]
        ),
        [
            (Ignored, Some(0.0)),
            (Ignored, Some(0.0)),
            (Handled, Some(END)),
            (Ignored, Some(END)),
            (Ignored, Some(END)),
            (Handled, Some(END - LINE)),
            (Handled, Some(0.0)),
            (Ignored, Some(0.0)),
        ],
        "at either end the scroller lets the key go on"
    );
}

#[test]
fn the_last_line_stops_at_the_end() {
    let mut surface = focused_list();
    press(&mut surface, Key::End);
    press(&mut surface, Key::Up);

    assert_eq!(
        presses(&mut surface, &[Key::Down, Key::Down]),
        [(Handled, Some(END)), (Ignored, Some(END))],
        "a line that would pass the end stops at it"
    );
}

#[test]
fn a_scroller_at_its_end_lets_the_next_one_out_go_to_its_end() {
    let mut surface = a_list_in_a_page();

    assert_eq!(
        presses_reading(
            &mut surface,
            &[Key::End, Key::End, Key::Down],
            list_and_page
        ),
        [
            (Handled, (Some(150.0), Some(0.0))),
            (Handled, (Some(150.0), Some(340.0))),
            (Ignored, (Some(150.0), Some(340.0))),
        ],
        "the list goes to its end, then the page around it does, and at the \
         end of both the key goes on"
    );
}

#[test]
fn a_horizontal_scroller_takes_left_right_home_and_end() {
    // A 200 viewport over a strip 650 wide, ending at 450.
    let strip = container()
        .layout(Flex::row())
        .children((0..13).map(|_| container().width(50.0).height(fill())));
    let view = container()
        .width(200.0)
        .height(200.0)
        .scroll(Scroll::horizontal())
        .focusable(true)
        .child(strip);
    let mut surface = Harness::focused(view, 200.0, 200.0);

    let keys = [
        Key::Right,
        Key::Right,
        Key::Left,
        Key::Down,
        Key::Up,
        Key::PageDown,
        Key::End,
        Key::Right,
        Key::Home,
        Key::Left,
    ];
    let seen = presses_reading(&mut surface, &keys, |painted| {
        content_offset(painted, (Some(650.0), None)).map(|(x, _)| x)
    });

    assert_eq!(
        keys.into_iter().zip(seen).collect::<Vec<_>>(),
        [
            (Key::Right, (Handled, Some(LINE))),
            (Key::Right, (Handled, Some(2.0 * LINE))),
            (Key::Left, (Handled, Some(LINE))),
            (Key::Down, (Ignored, Some(LINE))),
            (Key::Up, (Ignored, Some(LINE))),
            (Key::PageDown, (Ignored, Some(LINE))),
            (Key::End, (Handled, Some(450.0))),
            (Key::Right, (Ignored, Some(450.0))),
            (Key::Home, (Handled, Some(0.0))),
            (Key::Left, (Ignored, Some(0.0))),
        ]
    );
}

#[test]
fn on_a_scroller_of_both_axes_home_and_end_are_vertical() {
    // 600 by 1050 under a 200 viewport, the bars floating over it so that
    // neither takes from the viewport.
    let view = list()
        .scroll(Scroll::both().overlay())
        .focusable(true)
        .child(rows(21, false).width(600.0));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    assert_eq!(
        presses_reading(
            &mut surface,
            &[Key::Right, Key::Down, Key::End, Key::Home],
            |painted| content_offset(painted, (Some(600.0), Some(CONTENT)))
        ),
        [
            (Handled, Some((LINE, 0.0))),
            (Handled, Some((LINE, LINE))),
            (Handled, Some((LINE, END))),
            (Handled, Some((LINE, 0.0))),
        ]
    );
}

#[test]
fn a_focused_text_field_keeps_its_keys_and_passes_up_and_down() {
    clear_focus();
    // A field fifty high above twenty rows: 1050 of content again.
    let content = container()
        .layout(Flex::column())
        .child(
            container()
                .width(fill())
                .height(50.0)
                .child(text_input(create_signal("hello".to_string()))),
        )
        .child(rows(20, false));
    let mut surface = Harness::laid_out(list().child(content), 200.0, 200.0);
    press(&mut surface, Key::Tab);

    assert_eq!(
        presses(
            &mut surface,
            &[
                Key::Down,
                Key::Down,
                Key::Left,
                Key::Right,
                Key::Home,
                Key::End,
                Key::Up,
            ]
        ),
        [
            (Handled, Some(LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(2.0 * LINE)),
            (Handled, Some(LINE)),
        ],
        "the field moves its caret with Left, Right, Home and End, and lets Up \
         and Down scroll the list"
    );
}

#[test]
fn with_nothing_focused_nothing_scrolls() {
    clear_focus();
    // A listener inside the scroller that lets the key through: the key goes
    // down to it, past the scroller, and the scroller must not move for it.
    let view = list().child(rows(21, false).on_key_down(|_, _, _| Ignored));
    let mut surface = Harness::laid_out(view, 200.0, 200.0);

    assert_eq!(
        presses(&mut surface, &[Key::PageDown, Key::Down, Key::End]),
        [(Ignored, Some(0.0)); 3]
    );
}

#[test]
fn the_scrollers_own_key_handler_hears_the_key_first() {
    let view = list()
        .focusable(true)
        .on_key_down(|key, _, _| {
            if matches!(key, Key::PageDown | Key::Down) {
                Handled
            } else {
                Ignored
            }
        })
        .child(rows(21, false));
    let mut surface = Harness::focused(view, 200.0, 200.0);

    assert_eq!(
        presses(&mut surface, &[Key::PageDown, Key::Down, Key::End]),
        [
            (Handled, Some(0.0)),
            (Handled, Some(0.0)),
            (Handled, Some(END))
        ],
        "the handler took Page Down and Down, so only End scrolled"
    );
}

#[test]
fn a_shortcut_held_with_a_scrolling_key_scrolls_nothing() {
    let mut surface = focused_list();

    let held = Modifiers::default();
    for modifiers in [
        Modifiers { ctrl: true, ..held },
        Modifiers { alt: true, ..held },
        Modifiers { logo: true, ..held },
    ] {
        for key in [Key::PageDown, Key::Down, Key::End] {
            assert_eq!(
                press_with(&mut surface, key, modifiers),
                Ignored,
                "{key:?} with {modifiers:?}"
            );
        }
    }
    assert_eq!(offset(&mut surface), Some(0.0));
}
