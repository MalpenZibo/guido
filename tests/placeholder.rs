//! `placeholder`: what an empty field says.
//!
//! Asserted on the draw commands, since the whole feature is "which text, in which
//! colour" — including the one that matters for a password field: a placeholder is
//! a label, not a value, so it is never masked.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::Tree;

/// Every text this widget draws, as (content, colour).
fn drawn(widget: impl Widget + 'static) -> Vec<(String, Color)> {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 40.0))
    });

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    let mut out = Vec::new();
    collect(&node, &mut out);
    out
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

#[test]
fn an_empty_field_says_its_placeholder() {
    let texts = drawn(text_input(create_signal(String::new())).placeholder("Password"));

    assert_eq!(
        texts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>(),
        ["Password"]
    );
}

#[test]
fn a_field_with_a_value_says_the_value() {
    let texts = drawn(text_input(create_signal("zibo".to_owned())).placeholder("Username"));

    assert_eq!(
        texts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>(),
        ["zibo"],
        "the placeholder must stand in for the text, not sit beside it"
    );
}

#[test]
fn a_password_placeholder_is_not_masked() {
    // It is a label, not a value. Masking it would draw eight bullets where the
    // field is supposed to say what it wants.
    let texts = drawn(
        text_input(create_signal(String::new()))
            .password(true)
            .placeholder("Password"),
    );

    assert_eq!(texts[0].0, "Password");
}

#[test]
fn the_value_of_a_password_field_still_is() {
    let texts = drawn(
        text_input(create_signal("hunter2".to_owned()))
            .password(true)
            .placeholder("Password"),
    );

    assert_eq!(texts[0].0, "•••••••");
}

#[test]
fn a_placeholder_is_quieter_than_the_text() {
    let text_color = Color::rgba(1.0, 1.0, 1.0, 1.0);
    let empty = drawn(
        container().child(
            text_input(create_signal(String::new()))
                .color(text_color)
                .placeholder("Password"),
        ),
    );
    let filled = drawn(
        container().child(
            text_input(create_signal("x".to_owned()))
                .color(text_color)
                .placeholder("Password"),
        ),
    );

    let placeholder_alpha = empty[0].1.a;
    let value_alpha = filled[0].1.a;
    assert!(
        placeholder_alpha < value_alpha,
        "placeholder was {placeholder_alpha}, value {value_alpha}"
    );
    assert_eq!(
        (empty[0].1.r, empty[0].1.g, empty[0].1.b),
        (text_color.r, text_color.g, text_color.b),
        "and it is the same colour, only quieter — not a colour of its own"
    );
}

/// The field draws the placeholder, so the field is what says its colour.
#[test]
fn a_field_can_declare_its_own_placeholder_colour() {
    let own = Color::rgba(0.0, 1.0, 0.0, 1.0);
    let texts = drawn(
        container().child(
            text_input(create_signal(String::new()))
                .color(Color::rgba(1.0, 1.0, 1.0, 1.0))
                .placeholder("Password")
                .placeholder_color(own),
        ),
    );

    assert_eq!(texts[0].1, own);
}

#[test]
fn an_empty_field_without_a_placeholder_draws_no_text_at_all() {
    let texts = drawn(text_input(create_signal(String::new())));

    assert!(
        texts.is_empty(),
        "empty text is not drawn, so the placeholder is the only thing that can \
         put something there: {texts:?}"
    );
}
