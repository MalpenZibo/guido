//! `no_caret()`: nothing drawn, not merely nothing blinking.
//!
//! The first version of this option only stopped the blink. `cursor_visible`
//! stayed at what the constructor left it — true — so a field asked for no caret
//! got a permanent one instead: still, and still there. Scheduling tests could not
//! see that, because the wakeups were correctly absent. These look at what is
//! drawn.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::reactive::focus::{clear_focus, request_focus};
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::Tree;

/// Every rectangle a focused input draws. With no selection, the caret is the
/// only one there can be.
fn rects(input: TextInput) -> Vec<Rect> {
    clear_focus();
    let mut tree = Tree::new();
    let id = tree.register(Box::new(input));
    tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(id, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 200.0, 40.0))
    });
    request_focus(&tree, id);

    let mut node = RenderNode::new(id.as_u64());
    tree.with_widget_mut(id, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    let mut out = Vec::new();
    collect(&node, &mut out);
    out
}

fn collect(node: &RenderNode, out: &mut Vec<Rect>) {
    for cmd in &node.commands {
        if let DrawCommand::RoundedRect { rect, .. } = &**cmd {
            out.push(*rect);
        }
    }
    for child in &node.children {
        collect(child, out);
    }
}

#[test]
fn a_focused_field_draws_its_caret() {
    let drawn = rects(text_input(create_signal("hi".to_owned())));

    assert_eq!(drawn.len(), 1, "the caret, and nothing else: {drawn:?}");
}

#[test]
fn no_caret_draws_no_caret() {
    let drawn = rects(text_input(create_signal("hi".to_owned())).no_caret());

    assert!(
        drawn.is_empty(),
        "asked for no caret and got one anyway: {drawn:?}"
    );
}

#[test]
fn no_caret_still_takes_the_focus() {
    // The point of the option is losing the caret, not losing the keyboard.
    clear_focus();
    let mut tree = Tree::new();
    let id = tree.register(Box::new(
        text_input(create_signal(String::new()))
            .no_caret()
            .autofocus(),
    ));
    tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(id, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 200.0, 40.0))
    });

    assert!(guido::reactive::focus::has_focus(id));
}
