//! Nested rotations add up.
//!
//! The render snapshots already cover nested transforms, but they record the
//! composed *matrix*: a diff where 10° inside 20° became 25° is six changed
//! floats, and nobody reading it would see the arithmetic. Here the angle is
//! recovered and asserted, so the property has a name.
//!
//! The tree is `examples/transform_test_3level.rs`, whose four cases reach two
//! distinct answers by four routes — three of them rigid, where only the
//! outermost turns and the rest inherit.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{PaintContext, RenderNode};
use guido::tree::Tree;

fn three_level(gp: f32, p: f32, c: f32) -> Container {
    container()
        .width(120.0)
        .height(120.0)
        .rotate(gp)
        .padding(15.0)
        .child(
            container()
                .width(90.0)
                .height(90.0)
                .rotate(p)
                .padding(12.0)
                .child(container().width(50.0).height(50.0).rotate(c)),
        )
}

/// The angle each level is *seen* at: its own rotation plus everything above
/// it, which is what composition means and what the eye reads.
fn angles_down_the_chain(gp: f32, p: f32, c: f32) -> Vec<f32> {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(three_level(gp, p, c)));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 400.0))
    });
    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    let mut out = Vec::new();
    let mut running = 0.0;
    let mut cursor = Some(&node);
    while let Some(n) = cursor {
        let t = n.local_transform;
        running += t.c().atan2(t.a()).to_degrees();
        out.push(running);
        cursor = n.children.first().map(|c| &**c);
    }
    out
}

fn assert_angles(got: &[f32], want: [f32; 3]) {
    assert_eq!(got.len(), 3, "three levels, got {got:?}");
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            (g - w).abs() < 0.01,
            "level {i}: expected {w}°, got {g}° (whole chain {got:?})"
        );
    }
}

#[test]
fn a_rotation_at_each_level_accumulates_downward() {
    assert_angles(&angles_down_the_chain(10.0, 10.0, 10.0), [10.0, 20.0, 30.0]);
}

#[test]
fn a_rotation_only_at_the_top_turns_the_whole_stack_rigidly() {
    assert_angles(&angles_down_the_chain(20.0, 0.0, 0.0), [20.0, 20.0, 20.0]);
    assert_angles(&angles_down_the_chain(0.0, 0.0, 0.0), [0.0, 0.0, 0.0]);
}

/// Two routes to the same place: a child three levels down is at 30° whether
/// that came from one rotation or three, even though the boxes containing it
/// are at completely different angles.
#[test]
fn the_same_total_is_the_same_angle_however_it_was_reached() {
    let spread = angles_down_the_chain(10.0, 10.0, 10.0);
    let rigid = angles_down_the_chain(30.0, 0.0, 0.0);

    assert!((spread[2] - rigid[2]).abs() < 0.01, "the innermost agrees");
    assert!(
        (spread[0] - rigid[0]).abs() > 15.0,
        "while the outermost does not, which is what makes it a real test"
    );
}
