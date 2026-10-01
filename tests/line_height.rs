//! `line_height`: how tall a line of text is, declared or left to the font.
//!
//! Measured with the vendored font, so a line the font decides is DejaVu Sans
//! Mono's and not whatever the machine has installed.

mod common;

use common::Harness;
use guido::prelude::*;

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");

/// DejaVu Sans Mono's own line height over its size: (ascent − descent +
/// line gap) / units per em, from its hhea table.
const DEJAVU_RATIO: f32 = (1901.0 + 483.0) / 2048.0;

fn dejavu() -> FontFamily {
    // The measurer reads the registered fonts when it is first built on this
    // thread, so they are registered before anything is measured.
    guido::load_font(FONT.to_vec());
    FontFamily::name("DejaVu Sans Mono")
}

fn height_of(widget: impl Widget + 'static) -> f32 {
    let mut harness = Harness::laid_out(widget, 400.0, 400.0);
    harness.lay_out(400.0, 400.0).height
}

/// A field beside a label shares its line box when it shares its style, so
/// the two sit on one row without either being padded to match.
#[test]
fn a_field_and_a_label_with_one_style_are_one_height() {
    let family = dejavu();
    for line_height in [
        LineHeight::Normal,
        LineHeight::Relative(1.5),
        LineHeight::Absolute(20.0),
    ] {
        let label = height_of(
            text("Sign in")
                .font_size(14.0)
                .font_family(family)
                .line_height(line_height),
        );
        let field = height_of(
            text_input(create_signal("Sign in".to_owned()))
                .font_size(14.0)
                .font_family(family)
                .line_height(line_height),
        );
        assert!(
            (label - field).abs() < 0.01,
            "at {line_height:?} the label is {label} tall and the field {field}"
        );
    }

    let field = height_of(
        text_input(create_signal(String::new()))
            .font_size(14.0)
            .font_family(family),
    );
    assert!(
        (field - DEJAVU_RATIO * 14.0).abs() < 0.01,
        "an empty field is one line of the font's own height, not 1.2 of its \
         size: {field}"
    );
}

/// A line height a state supplies reaches the text it is declared for, as the
/// size and weight it is declared beside do.
#[test]
fn a_line_height_an_override_supplies_reaches_the_text() {
    let family = dejavu();
    let hot = create_signal(true);
    let mut harness = Harness::laid_out(
        text("a\nb")
            .font_size(14.0)
            .font_family(family)
            .line_height(LineHeight::Absolute(20.0))
            .state(hot, |s| s.line_height(1.5)),
        400.0,
        400.0,
    );
    let overridden = harness.lay_out(400.0, 400.0).height;
    assert!(
        (overridden - 42.0).abs() < 0.01,
        "two lines at 1.5 times 14 are 42 tall, got {overridden}"
    );

    hot.set(false);
    // What the job a state change queues does in the running loop.
    harness.tree.mark_needs_layout(harness.root);
    let declared = harness.lay_out(400.0, 400.0).height;
    assert!(
        (declared - 40.0).abs() < 0.01,
        "and the declaration comes back when the state goes: {declared}"
    );
}
