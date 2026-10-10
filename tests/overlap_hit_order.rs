//! Where siblings overlap, a pointer event goes to the one drawn on top (#636).
//!
//! Children are painted in the order they are declared, so the last one is on
//! top, and they are asked in the reverse of that order. A sibling the point
//! *hits* stops the walk whether or not it takes the event: one that draws
//! there, listens there, or has a child hit there. One that only lays out —
//! a full-size `ZStack` layer aligning its content — is not hit, and the
//! event goes on to the sibling beneath it.

use std::cell::Cell;
use std::rc::Rc;

use guido::layout::ZStack;
use guido::prelude::*;

mod common;
use common::Harness;

/// A counter a callback bumps, readable from the test.
fn counter() -> (Rc<Cell<u32>>, impl Fn() + 'static) {
    let count = Rc::new(Cell::new(0));
    let bump = {
        let count = Rc::clone(&count);
        move || count.set(count.get() + 1)
    };
    (count, bump)
}

/// Whether a container is hovered, as its `on_hover` last said.
fn hover_flag() -> (Rc<Cell<bool>>, impl Fn(bool) + 'static) {
    let hovered = Rc::new(Cell::new(false));
    let set = {
        let hovered = Rc::clone(&hovered);
        move |now| hovered.set(now)
    };
    (hovered, set)
}

/// An 80×40 stack of `children`, the last one drawn on top.
fn stack(children: impl IntoIterator<Item = Container>) -> Container {
    container()
        .layout(ZStack::new())
        .width(80.0)
        .height(40.0)
        .children(children)
}

#[test]
fn the_sibling_drawn_on_top_takes_the_click() {
    let (below, bump_below) = counter();
    let (above, bump_above) = counter();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_below),
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::BLUE)
                .on_click(bump_above),
        ]),
        400.0,
        200.0,
    );

    h.click(40.0, 20.0);
    assert_eq!(
        (below.get(), above.get()),
        (0, 1),
        "the card underneath was clicked through the one on top"
    );
}

#[test]
fn a_button_over_a_card_takes_its_own_click_and_leaves_the_rest() {
    let (card, bump_card) = counter();
    let (button, bump_button) = counter();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_card),
            container()
                .width(16.0)
                .height(16.0)
                .background(Color::BLUE)
                .on_click(bump_button),
        ]),
        400.0,
        200.0,
    );

    h.click(8.0, 8.0);
    assert_eq!(
        (card.get(), button.get()),
        (0, 1),
        "a click on the button is the button's"
    );

    h.click(60.0, 30.0);
    assert_eq!(
        (card.get(), button.get()),
        (1, 1),
        "a click beside the button is the card's"
    );
}

/// A card is hit where it draws, handler or none — so what is beneath it is
/// covered, as it is to the eye.
#[test]
fn a_card_that_only_draws_stops_the_click_on_the_button_beneath() {
    let (button, bump_button) = counter();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_button),
            container().width(80.0).height(40.0).background(Color::BLUE),
        ]),
        400.0,
        200.0,
    );

    h.click(40.0, 20.0);
    assert_eq!(button.get(), 0, "the card on top covers the button");
}

/// A layer that only lays out draws nothing and listens for nothing, so the
/// point is not on it: the bar idiom of full-size layers each aligning its own
/// content keeps working without a pass-through declaration.
#[test]
fn a_layer_that_only_lays_out_lets_the_click_and_the_hover_through() {
    let (button, bump_button) = counter();
    let (hovered, on_hover) = hover_flag();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_button)
                .on_hover(on_hover),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row().main_alignment(MainAlignment::End))
                .child(container().width(8.0).height(8.0).background(Color::BLUE)),
        ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_move(20.0, 20.0));
    assert!(hovered.get(), "the layer above is not over the button");

    h.click(20.0, 20.0);
    assert_eq!(button.get(), 1, "the layer above is not over the button");
}

/// Hover follows the hit: the sibling beneath is not hovered where another
/// covers it, and one hovered before the pointer moved onto the overlap is
/// told it is not hovered any more.
#[test]
fn moving_onto_the_overlap_leaves_the_sibling_beneath_unhovered() {
    let (card_hovered, on_card_hover) = hover_flag();
    let (button_hovered, on_button_hover) = hover_flag();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_hover(on_card_hover),
            container()
                .width(16.0)
                .height(16.0)
                .background(Color::BLUE)
                .on_hover(on_button_hover),
        ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_move(60.0, 30.0));
    assert_eq!(
        (card_hovered.get(), button_hovered.get()),
        (true, false),
        "beside the button, only the card is under the pointer"
    );

    h.send(Event::mouse_move(8.0, 8.0));
    assert_eq!(
        (card_hovered.get(), button_hovered.get()),
        (false, true),
        "over the button, the card beneath it is covered"
    );

    h.send(Event::mouse_move(60.0, 30.0));
    assert_eq!(
        (card_hovered.get(), button_hovered.get()),
        (true, false),
        "and back beside it, the card is the one under the pointer again"
    );
}

/// A child that is hit but takes nothing stops the walk at its parent, and the
/// parent's own handling runs — the click is the top card's, through its
/// label, and never reaches the card beneath.
#[test]
fn a_hit_container_whose_child_took_nothing_takes_its_own_click() {
    let (below, bump_below) = counter();
    let (above, bump_above) = counter();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_below),
            container()
                .width(80.0)
                .height(40.0)
                .on_click(bump_above)
                .child(container().width(40.0).height(20.0).background(Color::BLUE)),
        ]),
        400.0,
        200.0,
    );

    h.click(10.0, 10.0);
    assert_eq!(
        (below.get(), above.get()),
        (0, 1),
        "the press on the child is its parent's, not the card's beneath"
    );
}

/// The cursors a surface asks for as the pointer enters beside `top` and then
/// moves onto it, where `top` is drawn over a 120-pixel strip that declares
/// the pointing hand. `None` where there is no adapter to run it on.
#[cfg(feature = "testing")]
fn cursors_beside_then_over(top: fn() -> Container) -> Option<Vec<CursorIcon>> {
    use std::time::Instant;

    let mut app = common::headless()?;
    let surface = app.surface(
        SurfaceConfig::new()
            .height(50)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .width(fill())
                .height(fill())
                .layout(ZStack::new())
                .child(
                    container()
                        .width(120.0)
                        .height(fill())
                        .cursor(CursorIcon::Pointer),
                )
                .child(top().width(40.0).height(fill()))
        },
    );
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let at = Instant::now();
    let beside = Point::new(80.0, 10.0);
    app.event_at(surface, Event::MouseEnter { at: Some(beside) }, at);
    app.event_at(surface, Event::mouse_move(beside.x, beside.y), at);
    app.step();
    app.event_at(surface, Event::mouse_move(20.0, 10.0), Instant::now());
    app.step();
    Some(app.cursors_asked().to_vec())
}

/// The cursor is decided by the same walk, so it follows the hit: the sibling
/// beneath is never asked where the one on top covers it, and cannot overwrite
/// what the one on top declared.
#[cfg(feature = "testing")]
#[test]
fn the_cursor_is_the_one_the_sibling_on_top_declares() {
    let Some(asked) = cursors_beside_then_over(|| container().cursor(CursorIcon::Crosshair)) else {
        return;
    };
    assert_eq!(
        asked,
        [CursorIcon::Pointer, CursorIcon::Crosshair],
        "beside the top one, then over it, where it covers the one beneath"
    );
}

/// A card that draws and declares no cursor covers a strip that declares
/// one: over the card the pointer is over nothing that asks for a shape, so it
/// shows the arrow.
#[cfg(feature = "testing")]
#[test]
fn a_card_on_top_hides_the_cursor_of_the_button_beneath() {
    let Some(asked) = cursors_beside_then_over(|| container().background(Color::BLUE)) else {
        return;
    };
    assert_eq!(
        asked,
        [CursorIcon::Pointer, CursorIcon::Default],
        "beside the card, then over it, where it covers the strip"
    );
}

/// A drag that began on the sibling beneath is the press's, wherever it goes:
/// moving under the sibling on top does not take its position away, as a
/// point a scroller clips away does not.
#[test]
fn a_drag_begun_beneath_keeps_its_position_under_the_sibling_on_top() {
    let moves = Rc::new(Cell::new(None));
    let seen = Rc::clone(&moves);
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(|| {})
                .on_pointer_move(move |x, y| seen.set(Some((x, y)))),
            container().width(16.0).height(16.0).background(Color::BLUE),
        ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_down(60.0, 30.0, MouseButton::Left));
    h.send(Event::mouse_move(8.0, 8.0));
    assert_eq!(
        moves.get(),
        Some((8.0, 8.0)),
        "the card holding the press follows the pointer under the button"
    );
}

/// A disabled card that draws still covers what is beneath it: its handlers
/// and its children are not asked, and neither is the button under it (Qt's
/// answer — the event goes to the enabled parent, never to a sibling).
#[test]
fn a_disabled_card_that_draws_covers_the_button_beneath() {
    let (button, bump_button) = counter();
    let (hovered, on_hover) = hover_flag();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_button)
                .on_hover(on_hover),
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::BLUE)
                .enabled(false)
                .on_click(|| panic!("a disabled card takes no click")),
        ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_move(40.0, 20.0));
    assert!(!hovered.get(), "the disabled card covers the button");
    h.click(40.0, 20.0);
    assert_eq!(button.get(), 0, "the disabled card covers the button");
}

/// A disabled container that only lays out draws nothing to cover with, and
/// the click goes on to what is beneath it.
#[test]
fn a_disabled_layer_that_only_lays_out_lets_the_click_through() {
    let (button, bump_button) = counter();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump_button),
            container()
                .width(fill())
                .height(fill())
                .enabled(false)
                .layout(Flex::row().main_alignment(MainAlignment::End))
                .child(container().width(8.0).height(8.0).background(Color::BLUE)),
        ]),
        400.0,
        200.0,
    );

    h.click(20.0, 20.0);
    assert_eq!(button.get(), 1, "the disabled layer is not over the button");
}

/// A password field in a layer that only lays out covers the button beneath
/// it: the pointer over the field is over the field, not the button.
#[test]
fn a_password_field_covers_the_button_beneath() {
    let (hovered, on_hover) = hover_flag();
    let mut h = Harness::laid_out(
        stack([
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(|| {})
                .on_hover(on_hover),
            container()
                .width(fill())
                .height(fill())
                .child(password_input(create_password())),
        ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_move(20.0, 5.0));
    assert!(!hovered.get(), "the field covers the button");
}

/// A text field in a layer that only lays out shows its own cursor over the
/// strip beneath it, which is never asked to overwrite it.
#[cfg(feature = "testing")]
#[test]
fn a_text_field_on_top_shows_its_own_cursor() {
    let Some(asked) =
        cursors_beside_then_over(|| container().child(text_input(create_signal(String::new()))))
    else {
        return;
    };
    assert_eq!(
        asked,
        [CursorIcon::Pointer, CursorIcon::Text],
        "beside the field, then over it, where it covers the strip"
    );
}

/// A disabled card that draws hides the cursor of the strip beneath it, as an
/// enabled one does.
#[cfg(feature = "testing")]
#[test]
fn a_disabled_card_on_top_hides_the_cursor_beneath() {
    let Some(asked) =
        cursors_beside_then_over(|| container().background(Color::BLUE).enabled(false))
    else {
        return;
    };
    assert_eq!(
        asked,
        [CursorIcon::Pointer, CursorIcon::Default],
        "beside the card, then over it, where it covers the strip"
    );
}
