#![cfg(feature = "testing")]
//! A text the quad path draws is the text glyphon would have drawn.

use guido::prelude::*;

mod common;

/// The lowest row of ink of a label 20 pixels tall squeezed into a box 8
/// tall, in a card 40 tall, looked for where the card's corners do not reach.
fn lowest_ink_of_a_squeezed_label(corners: f32) -> Option<u32> {
    let mut app = common::headless()?;
    let surface = app.surface(
        SurfaceConfig::new()
            .height(60)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().width(fill()).child(
                container()
                    .width(200.0)
                    .height(40.0)
                    .overflow(Overflow::Hidden)
                    .corners(corners)
                    .background(Color::BLACK)
                    .child(
                        container()
                            .height(8.0)
                            .child(text("Mg Mg Mg").font_size(20.0).color(Color::WHITE)),
                    ),
            )
        },
    );
    app.configure(surface, 240, 60, 1.0);
    app.step();
    (0..40)
        .filter(|&y| (20..120).any(|x| app.read_pixel(surface, x, y)[0] > 128))
        .max()
}

/// A box shorter than its text cuts its letters only where a clip does, on
/// either path.
///
/// A rounded card whose corner reaches a label's ink sends the label to the
/// quad path, and the quad shaped it in its box's height where glyphon shapes
/// it in at least 50 pixels: so a label a closing panel squeezed was drawn
/// whole while upright, and cut at its box the moment the corner reached it
/// (#913).
#[test]
fn a_squeezed_label_is_cut_by_its_clip_not_by_its_box_on_a_quad() {
    let (Some(upright), Some(on_a_quad)) = (
        lowest_ink_of_a_squeezed_label(0.0),
        lowest_ink_of_a_squeezed_label(12.0),
    ) else {
        return;
    };
    // The box ends at row 8; the descenders are well below it.
    assert!(upright > 16, "glyphon cut the label at row {upright}");
    // A row apart at most: the quad is rasterized supersampled, and its
    // antialiasing is not glyphon's.
    assert!(
        on_a_quad + 1 >= upright,
        "ink down to row {on_a_quad} on the quad, {upright} upright"
    );
}
