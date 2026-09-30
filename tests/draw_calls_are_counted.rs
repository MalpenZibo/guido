#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What says the draw-call counter counts draw calls, pipeline by pipeline.
//!
//! #546 asks whether one draw call per image is a cost worth an atlas, and the
//! answer is a number `benches/icon_grid` prints. That number is only worth
//! reading if each pipeline's column counts what that pipeline issued, so each
//! is asserted here against a frame whose draw calls can be worked out by hand:
//! seven images are seven calls, and the shapes and the text around them are
//! one call per draw group.

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
fn each_image_is_a_draw_call_and_the_rest_are_one_per_group() {
    let Some(calls) = draw_calls_of_one_frame(scene) else {
        return;
    };

    assert_eq!(
        calls,
        DrawCalls {
            shapes: 2,
            images: IMAGES as u64,
            text: 1,
            text_quads: 0,
            backdrop: 0,
        }
    );
    assert_eq!(calls.total(), 2 + IMAGES as u64 + 1);
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
}

/// What preparing those seven images made on the GPU: a vertex buffer and a
/// bind group each, on the frame that uploaded their textures.
#[test]
fn preparing_a_frame_of_images_is_counted_in_buffers_and_bind_groups() {
    let Some(stats) = first_frame(scene) else {
        return;
    };

    assert_eq!(
        stats.quad_allocations,
        QuadAllocations {
            buffers: IMAGES as u64,
            bind_groups: IMAGES as u64,
        }
    );
}
