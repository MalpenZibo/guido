//! The interaction unit: which thing's hover a widget is talking about.
//!
//! Every case here is one the old rule got wrong or could not express. A
//! label's hover is not its own glyphs, and it is not any ancestor either —
//! it is the nearest thing marked as a unit.

use guido::layout::Flex;
use guido::prelude::*;
use guido::reactive::focus::{clear_focus, request_focus};
use guido::renderer::{DrawCommand, RenderNode};

mod common;
use common::Harness;

struct H {
    surface: Harness,
}

impl H {
    fn new(widget: impl Widget + 'static) -> Self {
        Self {
            surface: Harness::laid_out(widget, 400.0, 200.0),
        }
    }

    fn point_at(&mut self, x: f32, y: f32) {
        self.surface.send(Event::mouse_move(x, y));
    }

    /// Every text drawn, in paint order, as (content, colour).
    fn texts(&mut self) -> Vec<(String, Color)> {
        let mut out = Vec::new();
        collect(&self.surface.paint(), &mut out);
        out
    }

    fn colour_of(&mut self, content: &str) -> Color {
        self.texts()
            .into_iter()
            .find(|(t, _)| t == content)
            .unwrap_or_else(|| panic!("no text {content:?}"))
            .1
    }
}

fn collect(node: &RenderNode, out: &mut Vec<(String, Color)>) {
    for cmd in &node.commands {
        if let DrawCommand::Text { text, color, .. } = &**cmd {
            out.push((text.clone(), *color));
        }
    }
    for child in &node.children {
        collect(child, out);
    }
}

const WEAK: Color = Color::rgb(0.5, 0.5, 0.5);
const STRONG: Color = Color::rgb(1.0, 1.0, 1.0);

/// A label lights up while the pointer is on the button's *padding*, nowhere
/// near the glyphs. This is the case that rules out "my own bounds".
#[test]
fn a_label_follows_the_button_it_is_inside() {
    let mut h = H::new(
        container()
            .padding(20.0)
            .on_click(|| {})
            .child(text("Save").color(WEAK).when_hovered(|s| s.color(STRONG))),
    );

    assert_eq!(h.colour_of("Save"), WEAK);

    // Inside the button, inside the padding — far from any glyph.
    h.point_at(3.0, 3.0);
    assert_eq!(h.colour_of("Save"), STRONG);

    h.point_at(-50.0, -50.0);
    assert_eq!(h.colour_of("Save"), WEAK);
}

/// And it stays dark while the pointer is over a *sibling* button in the same
/// row. This is the case that rules out "any ancestor".
#[test]
fn a_label_ignores_the_hover_of_a_sibling_button() {
    let mut h = H::new(
        container()
            .layout(Flex::row())
            .control()
            .child(
                container()
                    .width(100.0)
                    .height(40.0)
                    .on_click(|| {})
                    .child(text("left").color(WEAK).when_hovered(|s| s.color(STRONG))),
            )
            .child(
                container()
                    .width(100.0)
                    .height(40.0)
                    .on_click(|| {})
                    .child(text("right").color(WEAK).when_hovered(|s| s.color(STRONG))),
            ),
    );

    // Over the right-hand button.
    h.point_at(150.0, 20.0);
    assert_eq!(h.colour_of("right"), STRONG);
    assert_eq!(
        h.colour_of("left"),
        WEAK,
        "a label must not follow a sibling button's hover"
    );
}

/// A nested control scopes *resolution*, not state: the row is still hovered
/// while the pointer is over the button inside it, because the pointer is
/// plainly still on the row.
#[test]
fn a_nested_control_does_not_switch_the_outer_one_off() {
    let mut h = H::new(
        container()
            .layout(Flex::row())
            .width(300.0)
            .height(40.0)
            .on_click(|| {})
            .child(
                container()
                    .width(100.0)
                    .height(40.0)
                    .on_click(|| {})
                    .child(text("button").color(WEAK).when_hovered(|s| s.color(STRONG))),
            )
            .child(text("row").color(WEAK).when_hovered(|s| s.color(STRONG))),
    );

    // Over the nested button, which is also over the row.
    h.point_at(50.0, 20.0);
    assert_eq!(h.colour_of("button"), STRONG);
    assert_eq!(
        h.colour_of("row"),
        STRONG,
        "the row is still under the pointer, so its own label still follows it"
    );
}

/// The case the old mechanism had no answer for: the focus is in a *sibling*,
/// not below the label, and both belong to the same unit.
#[test]
fn a_label_reacts_to_the_focus_of_the_input_beside_it() {
    clear_focus();
    let mut h = H::new(
        container()
            .layout(Flex::column())
            .control()
            .child(
                text("Password")
                    .color(WEAK)
                    .when_focused(|s| s.color(STRONG)),
            )
            .child(text_input(create_signal(String::from("x")))),
    );

    assert_eq!(h.colour_of("Password"), WEAK);

    let input = h.surface.tree.get_children(h.surface.root)[1];
    request_focus(&h.surface.tree, input);
    assert_eq!(h.colour_of("Password"), STRONG);

    clear_focus();
    assert_eq!(h.colour_of("Password"), WEAK);
}

/// With nothing marked above it, a text is its own unit and notices the
/// pointer over its own bounds.
#[test]
fn a_text_with_no_control_above_it_is_its_own_unit() {
    let mut h = H::new(
        container()
            .layout(Flex::row())
            .child(text("solo").color(WEAK).when_hovered(|s| s.color(STRONG))),
    );

    assert_eq!(h.colour_of("solo"), WEAK);

    h.point_at(2.0, 5.0);
    assert_eq!(h.colour_of("solo"), STRONG);

    h.point_at(-50.0, -50.0);
    assert_eq!(h.colour_of("solo"), WEAK);
}

/// A state the app owns needs no unit at all, so it works wherever it is
/// written.
#[test]
fn a_condition_the_app_owns_needs_no_control() {
    let wrong = create_signal(false);
    let mut h = H::new(
        container().layout(Flex::row()).child(
            text("Wrong password")
                .color(WEAK)
                .state(wrong, |s| s.color(STRONG)),
        ),
    );

    assert_eq!(h.colour_of("Wrong password"), WEAK);
    wrong.set(true);
    assert_eq!(h.colour_of("Wrong password"), STRONG);
}
