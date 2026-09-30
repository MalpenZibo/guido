#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What says the draw-call counter counts draw calls, pipeline by pipeline.
//!
//! #546 asks whether one draw call per image is a cost worth an atlas, and the
//! answer is a number `benches/icon_grid` prints. That number is only worth
//! reading if each pipeline's column counts what that pipeline issued, so each
//! is asserted here against a frame whose draw calls can be worked out by hand:
//! small images share an atlas page and are one call however many there are,
//! and the shapes and the text around them are one call per draw group.

mod common;

use guido::prelude::*;
use guido::render_stats::{self, DrawCalls, QuadAllocations, StatsSnapshot};

const IMAGES: usize = 7;

/// A distinct 4×4 source per index, so no two images share a texture.
fn icon(index: usize) -> ImageSource {
    let shade = (index * 30) as u8;
    ImageSource::Rgba {
        width: 4,
        height: 4,
        pixels: [shade, 0, 255 - shade, 255].repeat(16).into(),
    }
}

/// A background, seven images, a box pulled back over the last of them — a
/// shape over an image, so a second draw group — and a label in that group.
fn scene() -> Container {
    container()
        .layout(Flex::row())
        .background(Color::rgb(0.1, 0.1, 0.1))
        .children((0..IMAGES).map(|index| {
            container()
                .width(16.0)
                .height(16.0)
                .child(image(icon(index)).content_fit(ContentFit::Fill))
        }))
        .child(
            container()
                .width(16.0)
                .height(16.0)
                .translate((-8.0, 0.0))
                .background(Color::rgb(0.8, 0.2, 0.2)),
        )
        .child(text("icons"))
}

fn draw_calls_of_one_frame(build: impl FnOnce() -> Container + 'static) -> Option<DrawCalls> {
    first_frame(build).map(|stats| stats.draw_calls)
}

fn first_frame(build: impl FnOnce() -> Container + 'static) -> Option<StatsSnapshot> {
    let mut app = common::headless()?;
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        build,
    );
    app.configure(surface, 240, 20, 1.0);

    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();
    assert_eq!(
        stats.frames_painted, 1,
        "the frame under test was not painted"
    );
    Some(stats)
}

#[test]
fn seven_small_images_are_one_draw_call_and_the_rest_are_one_per_group() {
    let Some(calls) = draw_calls_of_one_frame(scene) else {
        return;
    };

    assert_eq!(
        calls,
        DrawCalls {
            shapes: 2,
            images: 1,
            text: 1,
            text_quads: 0,
            backdrop: 0,
        }
    );
    assert_eq!(calls.total(), 2 + 1 + 1);
}

/// The two pipelines the first frame does not reach, on a frame that reaches
/// them and nothing else.
///
/// A backdrop blur is four passes per region — downsample, a blur each way,
/// composite — and one present for the frame, so two regions are nine. A
/// rotated label is one textured quad, and one glyphon pass to rasterize the
/// texture it samples, which on its first frame has not been made yet.
#[test]
fn backdrop_passes_and_transformed_text_are_counted_under_their_own_pipelines() {
    let Some(calls) = draw_calls_of_one_frame(|| {
        container()
            .layout(Flex::row().spacing(8.0))
            .child(container().width(40.0).height(16.0).backdrop_blur(4.0))
            .child(container().width(40.0).height(16.0).backdrop_blur(4.0))
            .child(container().rotate(10.0).child(text("turned")))
    }) else {
        return;
    };

    assert_eq!(
        calls,
        DrawCalls {
            shapes: 0,
            images: 0,
            text: 1,
            text_quads: 1,
            backdrop: 9,
        }
    );
    assert_eq!(calls.total(), 1 + 1 + 9);
}

/// What preparing those seven images made on the GPU: one bind group, for the
/// atlas page all seven were packed onto, and the vertex and index buffers
/// every quad of the frame shares.
#[test]
fn preparing_a_frame_of_images_is_counted_in_buffers_and_bind_groups() {
    let Some(stats) = first_frame(scene) else {
        return;
    };

    assert_eq!(
        stats.quad_allocations,
        QuadAllocations {
            buffers: 2,
            bind_groups: 1,
        }
    );
}

/// Drawing the same images again makes nothing on the GPU. The frame is forced
/// by the backdrop's colour, which no image depends on, so every image is
/// prepared again from textures that are already cached.
#[test]
fn a_frame_that_draws_cached_textures_again_makes_no_buffer_and_no_bind_group() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || scene().background(backdrop),
    );
    app.configure(surface, 240, 20, 1.0);
    app.step();

    backdrop.set(Color::rgb(0.2, 0.2, 0.2));
    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();

    assert_eq!(stats.frames_painted, 1, "the second frame was not painted");
    assert!(
        stats.draw_calls.images > 0,
        "the second frame drew no image"
    );
    assert_eq!(stats.quad_allocations, QuadAllocations::default());
}

/// A distinct `side`×`side` source per index.
fn sized_icon(index: usize, side: u32) -> ImageSource {
    let shade = (index % 256) as u8;
    ImageSource::Rgba {
        width: side,
        height: side,
        pixels: [shade, 255 - shade, (index / 256) as u8, 255]
            .repeat((side * side) as usize)
            .into(),
    }
}

fn images_row(sources: Vec<ImageSource>) -> Container {
    container()
        .layout(Flex::row())
        .children(sources.into_iter().map(|source| {
            container()
                .width(4.0)
                .height(4.0)
                .child(image(source).content_fit(ContentFit::Fill))
        }))
}

/// The case the atlas is for: two hundred distinct small images, every one a
/// texture of its own before it and two hundred draw calls, are one.
///
/// The surface is as wide as the row, so none of them is off screen. That
/// all two hundred are in that one call is what the texture count says: an
/// image's texture is made only when its quad is prepared, and every prepared
/// quad is drawn.
#[test]
fn two_hundred_small_images_are_one_draw_call() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let surface = app.surface(
        SurfaceConfig::new()
            .height(4)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        || images_row((0..200).map(|i| sized_icon(i, 4)).collect()),
    );
    app.configure(surface, 800, 4, 1.0);

    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();

    assert_eq!(stats.frames_painted, 1);
    assert_eq!(app.image_textures(), 200, "every image was prepared");
    assert_eq!(stats.draw_calls.images, 1);
}

/// Paint order outranks batching. An image too large for a page keeps a
/// texture of its own, and drawing the small image after it early — with the
/// one before it — would draw it underneath: small, large, small is three.
#[test]
fn an_image_too_large_for_a_page_ends_the_run_it_interrupts() {
    let Some(stats) =
        first_frame(|| images_row(vec![sized_icon(0, 4), sized_icon(1, 300), sized_icon(2, 4)]))
    else {
        return;
    };

    assert_eq!(stats.draw_calls.images, 3);
}

/// A page is given back once eviction has taken everything off it — by the
/// frame's own `trim`, with nothing asked of it by hand. Four megabytes is
/// what a page costs, whether it holds one icon or four hundred.
///
/// The budget is set below one entry so the three images, once off screen and
/// idle past `KEEP_UNUSED`, are all evicted. That frame's `trim` frees their
/// entries after it has looked for empty pages, so the page goes on the next.
#[test]
fn a_page_eviction_empties_is_given_back() {
    let Some(mut app) = common::headless() else {
        return;
    };
    guido::set_image_cache_budget(0);
    let shown = create_signal(true);
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .width(fill())
                .height(fill())
                .background(backdrop)
                .child(move || {
                    shown
                        .get()
                        .then(|| images_row((0..3).map(|i| sized_icon(i, 4)).collect()))
                })
        },
    );
    app.configure(surface, 240, 20, 1.0);
    app.step();
    assert_eq!(app.image_atlas_pages(), 1);

    shown.set(false);
    app.step();
    assert_eq!(app.image_atlas_pages(), 1, "drawn within the last second");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    for shade in [0.2, 0.3] {
        backdrop.set(Color::rgb(shade, shade, shade));
        app.step();
    }

    assert_eq!(app.image_textures(), 0, "all three were evicted");
    assert_eq!(app.image_atlas_pages(), 0);
}

/// The frame's vertex and index buffers grow only when a frame holds more
/// quads than they ever have, and then to at least twice that: a frame that
/// fits allocates nothing, the full-to-the-brim one included.
///
/// Two, five, sixteen, seventeen, thirty images: the first frame allocates
/// the minimum of sixteen, which the next two fit in; seventeen is the first
/// that does not, and doubles to thirty-two, which thirty fits in. Buffers
/// sized by anything but the quad count, or grown by less than double, or
/// grown when merely full, or a frame that kept the last one's quads, would
/// each allocate on a frame that is zero here.
#[test]
fn the_frame_buffers_grow_only_past_what_they_have_held() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let count = create_signal(2usize);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(4)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .child(move || images_row((0..count.get()).map(|i| sized_icon(i, 4)).collect()))
        },
    );
    app.configure(surface, 120, 4, 1.0);

    let mut buffers = Vec::new();
    for images in [2, 5, 16, 17, 30] {
        count.set(images);
        render_stats::reset_stats();
        app.step();
        let stats = render_stats::get_stats();
        assert_eq!(stats.frames_painted, 1, "{images} images were painted");
        assert_eq!(stats.draw_calls.images, 1, "{images} images are one run");
        buffers.push(stats.quad_allocations.buffers);
    }

    assert_eq!(buffers, [2, 0, 0, 2, 0]);
    assert_eq!(
        app.read_pixel(surface, 29 * 4 + 2, 2),
        [29, 255 - 29, 0, 255],
        "the last image of the grown buffer is drawn from its own vertices"
    );
}

/// Transformed text draws through a pipeline of its own, with a frame buffer
/// of its own: drawing the same labels again allocates nothing there either.
#[test]
fn transformed_text_drawn_again_allocates_no_buffer() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(40)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .background(backdrop)
                .layout(Flex::row())
                .children((0..10).map(|i| container().rotate(10.0).child(text(format!("{i}")))))
        },
    );
    app.configure(surface, 240, 40, 1.0);
    app.step();

    for shade in [0.2, 0.3] {
        backdrop.set(Color::rgb(shade, shade, shade));
        render_stats::reset_stats();
        app.step();
        let stats = render_stats::get_stats();
        assert_eq!(stats.draw_calls.text_quads, 10, "the labels were drawn");
        assert_eq!(stats.quad_allocations.buffers, 0);
    }
}
