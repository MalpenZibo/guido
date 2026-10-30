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

/// The first and last rows of ink of two lines of 40-pixel letters set 24
/// pixels apart, in a card whose corners reach them or do not.
fn ink_of_tightly_set_lines(corners: f32) -> Option<(u32, u32)> {
    let mut app = common::headless()?;
    let surface = app.surface(
        SurfaceConfig::new()
            .height(180)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().width(fill()).padding(30.0).child(
                container()
                    .width(200.0)
                    .height(120.0)
                    .overflow(Overflow::Hidden)
                    .corners(corners)
                    .background(Color::BLACK)
                    .padding([18.0, 20.0])
                    .child(
                        text("Ágyp Ágyp")
                            .font_size(40.0)
                            .line_height(LineHeight::Relative(0.6))
                            .color(Color::WHITE),
                    ),
            )
        },
    );
    app.configure(surface, 300, 180, 1.0);
    app.step();
    let rows: Vec<u32> = (0..180)
        .filter(|&y| (60..200).any(|x| app.read_pixel(surface, x, y)[0] > 128))
        .collect();
    Some((*rows.first()?, *rows.last()?))
}

/// Letters taller than the lines they are set on reach past them, above the
/// first and below the last, and the quad's texture has room for that.
///
/// cosmic-text centres a line's glyphs in it, so where the line is shorter
/// than they are they overhang it by half the difference on each side. The
/// texture was as tall as the lines and a fixed padding, which an accent
/// above the first line and the descenders below the last ran out of.
#[test]
fn tightly_set_lines_keep_their_overhang_on_a_quad() {
    let (Some(upright), Some(on_a_quad)) = (
        ink_of_tightly_set_lines(0.0),
        ink_of_tightly_set_lines(30.0),
    ) else {
        return;
    };
    // A row apart at most, as for the squeezed label above.
    assert!(
        on_a_quad.0 <= upright.0 + 1 && on_a_quad.1 + 1 >= upright.1,
        "ink from row {} to {} on the quad, {} to {} upright",
        on_a_quad.0,
        on_a_quad.1,
        upright.0,
        upright.1
    );
}
