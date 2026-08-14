//! Stroke and shadow: the draws they emit, and in what order.
//!
//! Both are approximated by re-drawing the glyphs at offsets, so what there is
//! to test is exactly that: how many copies, where, in what colour, and — the
//! part that is easy to get backwards — that the stroke lands *under* the fill.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::Tree;
use guido::widgets::Widget;

/// Every text command a widget emits, in draw order, as (x, y, colour).
fn draws(widget: impl Widget + 'static) -> Vec<(f32, f32, Color)> {
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

    let mut out = Vec::new();
    collect(&node, &mut out);
    out
}

fn collect(node: &RenderNode, out: &mut Vec<(f32, f32, Color)>) {
    for cmd in &node.commands {
        if let DrawCommand::Text { rect, color, .. } = &**cmd {
            out.push((rect.x, rect.y, *color));
        }
    }
    for child in &node.children {
        collect(child, out);
    }
}

#[test]
fn plain_text_is_a_single_draw() {
    assert_eq!(
        draws(container().text_color(Color::WHITE).child(text("hi"))).len(),
        1
    );
}

#[test]
fn the_fill_is_drawn_last() {
    // The one ordering that matters: painted over the fill, a stroke eats half
    // the weight of every stem and the text reads as thinner, not outlined.
    let cmds = draws(
        container()
            .text_color(Color::WHITE)
            .text_stroke(TextStroke::new(2.0, Color::BLACK))
            .text_shadow(TextShadow::new(0.0, 2.0, 4.0, Color::RED))
            .child(text("hi")),
    );

    let (x, y, color) = *cmds.last().unwrap();
    assert_eq!(color, Color::WHITE, "the fill is the last thing drawn");
    assert_eq!((x, y), (0.0, 0.0), "and it is the undisplaced one");
    assert!(
        cmds[..cmds.len() - 1]
            .iter()
            .all(|(_, _, c)| *c != Color::WHITE),
        "nothing else should be drawn in the fill colour"
    );
}

#[test]
fn the_shadow_is_drawn_before_the_stroke() {
    let cmds = draws(
        container()
            .text_color(Color::WHITE)
            .text_stroke(TextStroke::new(1.0, Color::BLACK))
            .text_shadow(TextShadow::new(0.0, 0.0, 0.0, Color::RED))
            .child(text("hi")),
    );

    let last_shadow = cmds.iter().rposition(|(_, _, c)| *c == Color::RED).unwrap();
    let first_stroke = cmds
        .iter()
        .position(|(_, _, c)| *c == Color::BLACK)
        .unwrap();
    assert!(last_shadow < first_stroke);
}

#[test]
fn a_stroke_surrounds_the_glyphs() {
    let cmds = draws(
        container()
            .text_color(Color::WHITE)
            .text_stroke(TextStroke::new(2.0, Color::BLACK))
            .child(text("hi")),
    );

    let ring: Vec<_> = cmds.iter().filter(|(_, _, c)| *c == Color::BLACK).collect();
    assert!(ring.len() >= 8, "got {} samples", ring.len());

    // Every copy sits on a circle of the stroke's width around the original.
    for (x, y, _) in &ring {
        let distance = (x * x + y * y).sqrt();
        assert!(
            (distance - 2.0).abs() < 1e-3,
            "sample at ({x}, {y}) is {distance} from the glyph, expected 2"
        );
    }

    // And they surround it rather than bunching on one side.
    assert!(ring.iter().any(|(x, _, _)| *x > 1.0));
    assert!(ring.iter().any(|(x, _, _)| *x < -1.0));
    assert!(ring.iter().any(|(_, y, _)| *y > 1.0));
    assert!(ring.iter().any(|(_, y, _)| *y < -1.0));
}

#[test]
fn a_wider_stroke_uses_more_samples() {
    let count = |width: f32| {
        draws(
            container()
                .text_stroke(TextStroke::new(width, Color::BLACK))
                .child(text("hi")),
        )
        .iter()
        .filter(|(_, _, c)| *c == Color::BLACK)
        .count()
    };
    assert!(
        count(4.0) > count(1.0),
        "the gaps between samples grow with the radius, so the tap count has \
         to grow with it too or the corners scallop"
    );
}

#[test]
fn an_unblurred_shadow_is_one_offset_copy() {
    let cmds = draws(
        container()
            .text_color(Color::WHITE)
            .text_shadow(TextShadow::new(3.0, 4.0, 0.0, Color::RED))
            .child(text("hi")),
    );

    let shadow: Vec<_> = cmds.iter().filter(|(_, _, c)| *c == Color::RED).collect();
    assert_eq!(shadow.len(), 1);
    assert_eq!((shadow[0].0, shadow[0].1), (3.0, 4.0));
}

#[test]
fn a_blurred_shadow_spreads_around_the_offset() {
    let cmds = draws(
        container()
            .text_color(Color::WHITE)
            .text_shadow(TextShadow::new(
                0.0,
                4.0,
                6.0,
                Color::rgba(0.0, 0.0, 0.0, 0.8),
            ))
            .child(text("hi")),
    );

    let shadow: Vec<_> = cmds
        .iter()
        .filter(|(_, _, c)| c.a > 0.0 && c.r == 0.0)
        .collect();
    assert!(shadow.len() > 8, "got {} samples", shadow.len());

    // Centred on the offset, not on the glyph.
    let mean_y = shadow.iter().map(|(_, y, _)| y).sum::<f32>() / shadow.len() as f32;
    assert!(
        (mean_y - 4.0).abs() < 0.5,
        "mean y was {mean_y}, expected ~4"
    );

    // The outer ring is fainter than the core, so the edge falls off.
    let core = shadow
        .iter()
        .find(|(x, y, _)| *x == 0.0 && *y == 4.0)
        .unwrap();
    assert!(shadow.iter().any(|(_, _, c)| c.a < core.2.a));
}

#[test]
fn a_zero_width_stroke_draws_nothing_extra() {
    assert_eq!(
        draws(
            container()
                .text_stroke(TextStroke::new(0.0, Color::BLACK))
                .child(text("hi"))
        )
        .len(),
        1
    );
}

#[test]
fn decoration_is_inherited_like_everything_else() {
    let cmds = draws(
        container()
            .text_stroke(TextStroke::new(1.0, Color::BLACK))
            .child(container().layout(Flex::row()).child(text("hi"))),
    );
    assert!(cmds.iter().any(|(_, _, c)| *c == Color::BLACK));
}

#[test]
fn decoration_does_not_change_how_much_room_the_text_takes() {
    // A shadow must not push the text's neighbours around.
    let measure = |decorated: bool| {
        let mut tree = Tree::new();
        let c = container().text_color(Color::WHITE);
        let c = if decorated {
            c.text_stroke(TextStroke::new(3.0, Color::BLACK))
                .text_shadow(TextShadow::new(2.0, 2.0, 8.0, Color::RED))
        } else {
            c
        };
        let root = tree.register(Box::new(c.child(text("hi"))));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 800.0, 600.0))
        })
        .unwrap()
    };
    assert_eq!(measure(true), measure(false));
}
