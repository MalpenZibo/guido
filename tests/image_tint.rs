#![cfg(all(feature = "testing", feature = "svg"))]
//! A tint colours a monochrome image on the GPU, from the texture it already
//! has (#545).

use guido::prelude::*;

mod common;
use common::headless;

const BACKDROP: Color = Color::rgb(0.0, 0.0, 1.0);

/// A 20×20 SVG, black all over: the colour a tint has to replace.
fn black_square() -> ImageSource {
    ImageSource::SvgBytes(
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="#000"/></svg>"##
            .to_vec()
            .into(),
    )
}

fn tinted(tint: Signal<Color>) -> Container {
    container().width(20.0).height(20.0).child(
        image(black_square())
            .content_fit(ContentFit::Fill)
            .tint(tint),
    )
}

#[test]
fn a_tinted_svg_is_drawn_in_its_tint() {
    let Some(mut app) = headless() else { return };
    let tint = create_signal(Color::rgb(1.0, 0.0, 0.0));
    let surface = app.surface(
        SurfaceConfig::new().height(20).background_color(BACKDROP),
        move || tinted(tint.into()),
    );
    app.configure(surface, 20, 20, 1.0);
    app.step();

    assert_eq!(app.read_pixel(surface, 10, 10), [255, 0, 0, 255]);
}

/// A new tint is a repaint, not a new raster: the texture the first one drew
/// from is the one the second draws from.
#[test]
fn a_new_tint_is_drawn_from_the_same_texture() {
    let Some(mut app) = headless() else { return };
    let tint = create_signal(Color::rgb(1.0, 0.0, 0.0));
    let surface = app.surface(
        SurfaceConfig::new().height(20).background_color(BACKDROP),
        move || tinted(tint.into()),
    );
    app.configure(surface, 20, 20, 1.0);
    app.step();
    assert_eq!(app.image_textures(), 1);

    tint.set(Color::rgb(0.0, 1.0, 0.0));
    app.step();

    assert_eq!(app.read_pixel(surface, 10, 10), [0, 255, 0, 255]);
    assert_eq!(app.image_textures(), 1, "the second tint made no texture");
}
