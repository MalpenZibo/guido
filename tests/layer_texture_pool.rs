#![cfg(feature = "testing")]
//! A layer draws the same picture whatever textures the pool hands it.

mod common;

use guido::prelude::*;

/// #808: a blurred row drawn from the pool a larger layer left behind is the
/// row drawn from fresh textures.
///
/// The panel's 256-by-128 texture is twice, each way, what the row's blur
/// would be allocated, and within what the pool lets it take. Taken one at a
/// time, the blur's two textures came out as that one and a fresh one half
/// its size, and the blur read the second as if it were the first's: the row
/// at twice its size, shifted right and down, cut by its composite.
#[test]
fn a_blurred_layer_is_the_same_from_a_pool_another_layer_left() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let panel = create_signal(true);
    let row = create_signal(false);
    let poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(140)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .layout(Flex::column())
                .child(container().width(10.0).height(10.0).background(poke))
                .child(move || {
                    row.get().then(|| {
                        container()
                            .width(60.0)
                            .height(20.0)
                            .background(Color::rgb(0.2, 0.6, 0.9))
                            .blur(3.0)
                            .child(text("No windows here").color(Color::WHITE))
                    })
                })
                .child(move || {
                    // Text over a background at half opacity: a layer.
                    panel.get().then(|| {
                        container()
                            .width(200.0)
                            .height(100.0)
                            .background(Color::rgb(0.8, 0.2, 0.2))
                            .opacity(0.5)
                            .child(text("panel"))
                    })
                })
        },
    );
    app.configure(surface, 240, 140, 1.0);
    app.step();

    // The panel goes, and its texture goes to the pool once it has idled.
    panel.set(false);
    for frame in 0..125 {
        poke.set(Color::rgb(0.1, 0.1, (frame % 2) as f32 * 0.1));
        app.step();
    }

    assert_eq!(
        app.layer_textures(),
        1,
        "the panel's texture is in the pool when the row arrives, or this \
         asks nothing of the pool"
    );
    poke.set(Color::rgb(0.1, 0.1, 0.1));
    row.set(true);
    app.step();
    let from_pool = app.read_frame(surface);

    app.forget_layer_textures();
    poke.set(Color::rgb(0.1, 0.1, 0.5));
    app.step();
    poke.set(Color::rgb(0.1, 0.1, 0.1));
    app.step();
    let fresh = app.read_frame(surface);

    let differing = from_pool
        .chunks(4)
        .zip(fresh.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "the blurred row drawn from the pool the panel left differs from the \
         same row drawn from fresh textures in {differing} pixels"
    );
}
