//! What a container declares must reach the glyphs.
//!
//! The unit tests in `tree.rs` cover the walk in isolation, over styles poked
//! straight onto nodes. These go through the real thing: a container built with
//! the public builders, laid out and painted, with the assertions made on the
//! `DrawCommand::Text` that comes out the far end. That is the only place where
//! a mistake in *any* of the links — the builder, the node publication, the
//! walk, or the widget reading the wrong signal — actually shows up.
//!
//! Only the style is asserted, never the measured geometry: text metrics depend
//! on the fonts installed on the machine, and the declared colour and size do
//! not.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::Tree;
use guido::widgets::Widget;

/// Lay out and paint `widget`, then collect every text command it emitted.
fn text_commands(widget: impl Widget + 'static) -> Vec<(String, Color, f32, FontWeight)> {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 800.0, 600.0))
    });

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    let mut found = Vec::new();
    collect(&node, &mut found);
    found
}

fn collect(node: &RenderNode, out: &mut Vec<(String, Color, f32, FontWeight)>) {
    for cmd in &node.commands {
        if let DrawCommand::Text {
            text,
            color,
            font_size,
            font_weight,
            ..
        } = &**cmd
        {
            out.push((text.clone(), *color, *font_size, *font_weight));
        }
    }
    for child in &node.children {
        collect(child, out);
    }
}

fn only(widget: impl Widget + 'static) -> (String, Color, f32, FontWeight) {
    let mut cmds = text_commands(widget);
    assert_eq!(cmds.len(), 1, "expected exactly one text command");
    cmds.pop().unwrap()
}

#[test]
fn a_text_with_nothing_declared_gets_the_defaults() {
    let (_, color, size, weight) = only(container().child(text("hi")));
    assert_eq!(color, Color::WHITE);
    assert_eq!(size, 14.0);
    assert_eq!(weight, FontWeight::NORMAL);
}

#[test]
fn the_enclosing_container_styles_the_text() {
    let (content, color, size, weight) = only(
        container()
            .text_color(Color::RED)
            .font_size(30.0)
            .bold()
            .child(text("hi")),
    );
    assert_eq!(content, "hi");
    assert_eq!(color, Color::RED);
    assert_eq!(size, 30.0);
    assert_eq!(weight, FontWeight::BOLD);
}

#[test]
fn style_reaches_through_containers_that_declare_nothing() {
    // The layout wrappers in between are exactly the case a parent-only lookup
    // would have failed on.
    let (_, color, size, _) = only(
        container().text_color(Color::RED).font_size(30.0).child(
            container()
                .padding(8.0)
                .layout(Flex::row())
                .child(container().child(text("hi"))),
        ),
    );
    assert_eq!(color, Color::RED);
    assert_eq!(size, 30.0);
}

#[test]
fn a_nearer_container_wins() {
    let (_, color, _, _) = only(
        container()
            .text_color(Color::RED)
            .child(container().text_color(Color::BLUE).child(text("hi"))),
    );
    assert_eq!(color, Color::BLUE);
}

#[test]
fn overriding_one_property_leaves_the_others_alone() {
    let (_, color, size, weight) = only(
        container()
            .text_color(Color::RED)
            .font_size(30.0)
            .bold()
            .child(container().font_size(12.0).child(text("hi"))),
    );
    assert_eq!(size, 12.0, "the nearer container sets the size");
    assert_eq!(color, Color::RED, "and must not drop the colour above it");
    assert_eq!(weight, FontWeight::BOLD, "nor the weight");
}

#[test]
fn siblings_resolve_independently() {
    let cmds = text_commands(
        container()
            .text_color(Color::RED)
            .layout(Flex::column())
            .child(text("inherits"))
            .child(container().text_color(Color::BLUE).child(text("overrides"))),
    );

    assert_eq!(cmds.len(), 2);
    let inherits = cmds.iter().find(|c| c.0 == "inherits").unwrap();
    let overrides = cmds.iter().find(|c| c.0 == "overrides").unwrap();
    assert_eq!(inherits.1, Color::RED);
    assert_eq!(overrides.1, Color::BLUE);
}

// Following a *change* to an inherited signal needs the job queue pumped, and
// `jobs` is crate-private — that test lives in `widgets::text`.
