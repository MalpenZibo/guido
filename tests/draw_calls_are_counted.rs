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
use guido::render_stats::{self, DrawCalls, GpuAllocations, LayerWork, PathMeshes, StatsSnapshot};
use guido::widget_prelude::*;

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
            paths: 0,
        }
    );
    assert_eq!(calls.total(), 2 + 1 + 1);
}

/// The two pipelines the first frame does not reach, on a frame that reaches
/// them and nothing else.
///
/// A backdrop blur is four draws per region — downsample, a blur each way,
/// composite — and one present for the frame, so two regions are nine. The
/// two share their passes, three and the present (#766). A rotated label is
/// one textured quad, and one glyphon pass to rasterize the texture it
/// samples, which on its first frame has not been made yet.
#[test]
fn backdrop_passes_and_transformed_text_are_counted_under_their_own_pipelines() {
    let Some(stats) = first_frame(|| {
        container()
            .layout(Flex::row().spacing(8.0))
            .child(container().width(40.0).height(16.0).backdrop_blur(4.0))
            .child(container().width(40.0).height(16.0).backdrop_blur(4.0))
            .child(container().rotate(10.0).child(text("turned")))
    }) else {
        return;
    };

    let calls = stats.draw_calls;
    assert_eq!(
        calls,
        DrawCalls {
            shapes: 0,
            images: 0,
            text: 1,
            text_quads: 1,
            backdrop: 9,
            paths: 0,
        }
    );
    assert_eq!(calls.total(), 1 + 1 + 9);
    assert_eq!(stats.backdrop_passes, 3 + 1);
}

/// A widget that draws one path, or one arc, in a 16×16 box.
enum Drawn {
    Line,
    Filled,
    Arc,
}

impl Widget for Drawn {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        constraints.constrain(Size::new(16.0, 16.0))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let zigzag = Path::new()
            .move_to((1.0, 14.0))
            .line_to((6.0, 2.0))
            .line_to((10.0, 12.0))
            .line_to((15.0, 3.0));
        let line = Stroke::new(1.6, Color::WHITE);
        match self {
            Drawn::Line => ctx.draw_path(&zigzag, None, line),
            Drawn::Filled => ctx.draw_path(&zigzag.close(), Fill::color(Color::BLUE), line),
            Drawn::Arc => ctx.draw_arc((8.0, 8.0), 6.0, 0.0, 270.0, line.caps(Cap::Round)),
        }
    }
}

/// Paths are a layer of their own between the shapes and the images: a
/// group's paths are one call however many shapes sit between them, as its
/// shapes are, and an arc is a shape like any rect.
///
/// A fill and a stroke are two meshes, so three paths drawn on a first frame
/// are four meshes — and two of the paths are the same zigzag, which is
/// tessellated once and drawn twice.
#[test]
fn a_group_s_paths_are_one_call_whatever_lies_between_them() {
    // Spaced, because a stroke's bounds reach past its box by as far as a
    // miter could, and a shape those bounds meet would split the group.
    let row = || {
        container()
            .layout(Flex::row().spacing(8.0))
            .child(container().width(16.0).height(16.0).background(Color::RED))
            .child(Drawn::Line)
            .child(Drawn::Filled)
            .child(
                container()
                    .width(16.0)
                    .height(16.0)
                    .background(Color::GREEN),
            )
            .child(Drawn::Line)
            .child(Drawn::Arc)
    };
    let Some(stats) = first_frame(row) else {
        return;
    };
    assert_eq!(
        stats.draw_calls,
        DrawCalls {
            shapes: 1,
            paths: 1,
            ..DrawCalls::default()
        }
    );
    assert_eq!(
        stats.path_meshes,
        PathMeshes {
            tessellated: 3,
            reused: 1,
        },
        "the two lines are one path, tessellated once; the closed one is a \
         fill and a stroke"
    );
}

/// A zigzag stroked with dashes ten pixels to a period, started `offset` in.
struct Dashed(f32);

impl Widget for Dashed {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        constraints.constrain(Size::new(16.0, 16.0))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let zigzag = Path::new()
            .move_to((1.0, 14.0))
            .line_to((6.0, 2.0))
            .line_to((10.0, 12.0))
            .line_to((15.0, 3.0));
        let line = Stroke::new(1.6, Color::WHITE).dashes(&[6.0, 4.0], self.0);
        ctx.draw_path(&zigzag, None, line);
    }
}

/// A dashed mesh is cached by its pattern and offset as well as its path, and
/// the offset modulo the pattern: two offsets a period apart are one mesh,
/// which is what lets dashes marching along a path reuse what they drew a
/// period ago, and an offset between them is a mesh of its own — kept beside
/// the first rather than in its place, so a second frame of the same strokes
/// tessellates nothing.
#[test]
fn dashes_a_period_apart_are_one_mesh() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .background(backdrop)
                .layout(Flex::row().spacing(8.0))
                .child(Dashed(3.0))
                .child(Dashed(13.0))
                .child(Dashed(4.0))
                .child(Drawn::Line)
        },
    );
    app.configure(surface, 240, 20, 1.0);
    render_stats::reset_stats();
    app.step();
    assert_eq!(
        render_stats::get_stats().path_meshes,
        PathMeshes {
            tessellated: 3,
            reused: 1,
        },
        "3 and 13 are one offset; 4 is another, and the same path undashed a \
         third mesh"
    );

    backdrop.set(Color::rgb(0.2, 0.2, 0.2));
    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();
    assert_eq!(stats.frames_painted, 1, "the second frame was not painted");
    assert_eq!(
        stats.path_meshes,
        PathMeshes {
            tessellated: 0,
            reused: 4,
        },
        "a mesh was tessellated again for the strokes the first frame drew"
    );
}

/// A shape painted over a path is drawn over it: the group splits there, as
/// it splits for a shape over an image, and each half draws its own shapes
/// and its own paths.
#[test]
fn a_shape_over_a_path_opens_a_group() {
    let Some(calls) = draw_calls_of_one_frame(|| {
        container()
            .layout(Flex::row())
            .child(container().width(16.0).height(16.0).background(Color::RED))
            .child(Drawn::Line)
            .child(
                container()
                    .width(16.0)
                    .height(16.0)
                    .translate((-8.0, 0.0))
                    .background(Color::GREEN),
            )
            .child(Drawn::Filled)
    }) else {
        return;
    };
    assert_eq!(
        calls,
        DrawCalls {
            shapes: 2,
            paths: 2,
            ..DrawCalls::default()
        }
    );
}

/// A label under a chain of clips is drawn by glyphon while every clip's box
/// cuts it as the clip would, and by the quad as soon as one of them does not.
///
/// The inner clip is a square inset from a rounded card by less than the
/// card's corners reach, so the card's curve crosses the square's sides and
/// the two cannot be merged: the square is the chain's first shape and the
/// card hangs from it. Asking only the first whether its box will do says yes
/// for both labels, and the one reaching the card's corner is drawn across it.
#[test]
fn a_label_under_a_chain_goes_to_the_quad_only_when_a_clip_in_it_cuts() {
    let chained = |centred: bool| {
        move || {
            let inner = container()
                .width(fill())
                .height(fill())
                .overflow(Overflow::Hidden)
                .layout(Flex::row().main_alignment(if centred {
                    MainAlignment::Center
                } else {
                    MainAlignment::Start
                }))
                .child(text("label"));
            container()
                .width(120.0)
                .height(20.0)
                .corners(10.0)
                .padding(2.0)
                .overflow(Overflow::Hidden)
                .child(inner)
        }
    };

    let Some(centred) = draw_calls_of_one_frame(chained(true)) else {
        return;
    };
    assert_eq!(
        (centred.text, centred.text_quads),
        (1, 0),
        "in the middle of the card no clip's box cuts differently from it"
    );

    let Some(cornered) = draw_calls_of_one_frame(chained(false)) else {
        return;
    };
    assert_eq!(
        cornered.text_quads, 1,
        "the label reaches the card's corner, which only the chain says"
    );
}

/// A label reaching the corner of a square clip is cut there by the clip's
/// box, which is the clip — so it stays with glyphon.
///
/// A square corner used to count as a corner box of zero size, which still
/// "overlapped" any ink covering the corner point, and every such label was
/// rasterized into a texture of its own for a cut glyphon makes exactly.
#[test]
fn a_label_reaching_a_square_clip_s_corner_stays_with_glyphon() {
    let Some(calls) = draw_calls_of_one_frame(|| {
        container()
            .width(120.0)
            .height(20.0)
            .overflow(Overflow::Hidden)
            .child(text("label"))
    }) else {
        return;
    };
    assert_eq!(
        (calls.text, calls.text_quads),
        (1, 0),
        "one glyphon call and no text quad"
    );
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
        GpuAllocations {
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
    assert_eq!(stats.quad_allocations, GpuAllocations::default());
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

/// A merged shape is drawn by a pipeline of its own, so the shapes before it
/// and after it in its group are a draw each, and it is one between them. A
/// frame that merges nothing draws its shapes in one call, as it always did —
/// which `a_group_s_paths_are_one_call_whatever_lies_between_them` holds.
#[test]
fn a_merged_shape_is_a_draw_between_the_shapes_around_it() {
    let row = || {
        container()
            .layout(Flex::row().spacing(8.0))
            .background(Color::rgb(0.1, 0.1, 0.1))
            .child(
                container()
                    .layout(Flex::row())
                    .merge_children(8.0)
                    .child(container().width(16.0).height(16.0).background(Color::RED))
                    .child(container().width(16.0).height(16.0).background(Color::RED))
                    // What a member holds is drawn after the merged shape.
                    .child(
                        container()
                            .padding(2.0)
                            .child(container().width(8.0).height(8.0).background(Color::BLUE)),
                    ),
            )
    };
    let Some(stats) = first_frame(row) else {
        return;
    };
    assert_eq!(
        stats.draw_calls,
        DrawCalls {
            shapes: 3,
            ..DrawCalls::default()
        },
        "the background, the merged shape and the box after it: three runs"
    );
}

/// A cell of its own colour with a white stripe down its middle, and a frosted
/// panel over the whole of it. Through the frost the middle is the two mixed:
/// green says how much white the blur drew in, and red and blue say the rest
/// was this cell's colour and no other's.
fn frosted_cell(index: usize) -> Container {
    let (red, blue) = cell_colour(index);
    container()
        .width(16.0)
        .height(16.0)
        .background(Color::rgb(red as f32 / 255.0, 0.0, blue as f32 / 255.0))
        .layout(ZStack::new())
        .children([
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .child(container().width(5.0))
                .child(
                    container()
                        .width(6.0)
                        .height(fill())
                        .background(Color::WHITE),
                )
                .into_any(),
            container()
                .width(fill())
                .height(fill())
                .backdrop_blur(BackdropBlur::new(4.0).sources(BackdropSources::SURFACE))
                .into_any(),
        ])
}

/// Red and blue of the cell at `index`; green is always nothing.
fn cell_colour(index: usize) -> (u8, u8) {
    let red = (index * 40 % 256) as u8;
    (red, 255 - red)
}

/// A frosted panel and a frosted text, blurred again over a background that
/// changed: the passes' uniforms are rewritten where they already are, and
/// every texture they read is one they have bound before.
#[test]
fn a_frame_that_blurs_its_backdrop_again_makes_no_buffer_and_no_bind_group() {
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
                .child(frosted_cell(0))
                .child(text("frost").backdrop_blur(8.0))
        },
    );
    app.configure(surface, 240, 40, 1.0);

    render_stats::reset_stats();
    app.step();
    let first = render_stats::get_stats();
    assert!(
        first.backdrop_allocations.buffers > 0 && first.backdrop_allocations.bind_groups > 0,
        "the first frame made nothing to blur with: {:?}",
        first.backdrop_allocations
    );

    for shade in [0.2, 0.3] {
        backdrop.set(Color::rgb(shade, shade, shade));
        render_stats::reset_stats();
        app.step();
        let stats = render_stats::get_stats();
        assert_eq!(stats.frames_painted, 1, "the frame was not painted");
        assert!(stats.draw_calls.backdrop > 0, "the frame drew no backdrop");
        assert_eq!(stats.backdrop_allocations, GpuAllocations::default());
    }
}

/// More backdrops than the uniforms have held grow them; the same number again
/// does not. And on every frame, the one that grew among them, each cell shows
/// its own colour through its frost, which is what says each pass read its own
/// uniforms and not a neighbour's — or a slot the growth left unwritten.
#[test]
fn the_backdrop_uniforms_grow_only_past_what_they_have_held() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let count = create_signal(1usize);
    // What forces a frame when the count does not change.
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(16)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().background(backdrop).child(move || {
                container()
                    .layout(Flex::row())
                    .children((0..count.get()).map(frosted_cell))
            })
        },
    );
    app.configure(surface, 240, 16, 1.0);

    let mut buffers = Vec::new();
    for (frame, cells) in [1, 1, 9, 9].into_iter().enumerate() {
        count.set(cells);
        let shade = frame as f32 / 10.0;
        backdrop.set(Color::rgb(shade, shade, shade));
        render_stats::reset_stats();
        app.step();
        let stats = render_stats::get_stats();
        assert_eq!(stats.frames_painted, 1, "{cells} cells were painted");
        buffers.push(stats.backdrop_allocations.buffers);

        for index in 0..cells {
            let [r, g, b, _] = app.read_pixel(surface, index as u32 * 16 + 8, 8);
            assert!(
                (32..224).contains(&g),
                "cell {index} reads [{r}, {g}, {b}] in the middle: the stripe was not blurred"
            );
            let white = f32::from(g) / 255.0;
            let mixed = |own: u8| f32::from(own) + (255.0 - f32::from(own)) * white;
            let (red, blue) = cell_colour(index);
            assert!(
                (f32::from(r) - mixed(red)).abs() <= 8.0
                    && (f32::from(b) - mixed(blue)).abs() <= 8.0,
                "cell {index} reads [{r}, {g}, {b}] in the middle, which is not its own \
                 colour blurred into the white"
            );
        }
    }

    assert!(buffers[0] > 0, "the first frame made no uniform buffer");
    assert_eq!(buffers[1], 0, "the same backdrop again made a buffer");
    assert!(buffers[2] > 0, "nine backdrops fit where one did");
    assert_eq!(buffers[3], 0, "nine backdrops again made a buffer");
}

/// A card at half opacity whose label overlaps its background, which makes it
/// a layer, beside a signal that moves only the card's opacity.
fn fading_card(opacity: RwSignal<f32>, blur: RwSignal<f32>, blurred: bool) -> Container {
    let card = container()
        .width(120.0)
        .height(18.0)
        .background(Color::rgb(0.8, 0.2, 0.2))
        .opacity(opacity)
        .child(text("card"));
    container().child(if blurred { card.blur(blur) } else { card })
}

/// Two frames of one scene: the first, and one after `between` has run.
fn two_frames(
    build: impl FnOnce() -> Container + 'static,
    between: impl FnOnce(),
) -> Option<(StatsSnapshot, StatsSnapshot)> {
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
    let first = render_stats::get_stats();
    between();
    render_stats::reset_stats();
    app.step();
    let second = render_stats::get_stats();
    assert_eq!(
        (first.frames_painted, second.frames_painted),
        (1, 1),
        "both frames under test were painted"
    );
    Some((first, second))
}

/// The passes a layer costs, worked out by hand.
///
/// Its first frame draws the card into a texture — the background in the
/// shape pipeline and the label in glyphon, against the texture — and lays
/// the texture down as one textured quad on the surface: one call each, one
/// texture allocated. The next frame changes only the card's opacity, so the
/// card repaints and its label does not, and the texture still holds the
/// same content: nothing is drawn into it, and the frame is the one quad.
#[test]
fn a_fading_layer_draws_its_content_once_and_composites_it_every_frame() {
    let opacity = create_signal(0.5);
    let blur = create_signal(0.0);
    let Some((first, second)) = two_frames(
        move || fading_card(opacity, blur, false),
        move || opacity.set(0.3),
    ) else {
        return;
    };

    assert_eq!(
        first.draw_calls,
        DrawCalls {
            shapes: 1,
            images: 1,
            text: 1,
            ..DrawCalls::default()
        },
        "the content into the texture, and the texture onto the surface"
    );
    assert_eq!(
        first.layers,
        LayerWork {
            drawn: 1,
            kept: 0,
            blurred: 0,
            textures: 1,
        }
    );

    assert_eq!(
        second.draw_calls,
        DrawCalls {
            images: 1,
            ..DrawCalls::default()
        },
        "the opacity moved and the content did not: one composite, nothing drawn into it"
    );
    assert_eq!(
        second.layers,
        LayerWork {
            drawn: 0,
            kept: 1,
            blurred: 0,
            textures: 0,
        }
    );
    assert_eq!(
        (second.paint_children_painted, second.paint_children_cached),
        (1, 1),
        "the card painted again, for its opacity, and its label served from \
         the cache: nothing under the card was painted to find out the texture \
         still holds it"
    );
}

/// A fade over draws that never overlap needs no layer: no texture, no
/// composite, and the shapes in the one call they always were.
#[test]
fn a_fade_over_draws_that_never_overlap_is_drawn_without_a_layer() {
    let Some(stats) = first_frame(|| {
        container()
            .layout(Flex::row().spacing(8.0))
            .opacity(0.5)
            .child(
                container()
                    .width(16.0)
                    .height(16.0)
                    .background(Color::rgb(0.8, 0.2, 0.2)),
            )
            .child(
                container()
                    .width(16.0)
                    .height(16.0)
                    .background(Color::rgb(0.2, 0.2, 0.8)),
            )
    }) else {
        return;
    };

    assert_eq!(stats.layers, LayerWork::default(), "no layer");
    assert_eq!(
        stats.draw_calls,
        DrawCalls {
            shapes: 1,
            ..DrawCalls::default()
        }
    );
}

/// A blur is three passes over the texture — a shrink and one each way — on
/// the frame it is first drawn, and on a frame where only its radius moves it
/// is those three again and no drawing: the blur runs over the content the
/// texture already holds.
#[test]
fn a_blur_that_moves_blurs_the_texture_again_without_drawing_into_it() {
    let opacity = create_signal(1.0);
    let blur = create_signal(4.0);
    let Some((first, second)) = two_frames(
        move || fading_card(opacity, blur, true),
        move || blur.set(6.0),
    ) else {
        return;
    };

    assert_eq!(
        first.draw_calls,
        DrawCalls {
            shapes: 1,
            images: 1,
            text: 1,
            backdrop: 3,
            ..DrawCalls::default()
        }
    );
    assert_eq!(
        first.layers,
        LayerWork {
            drawn: 1,
            kept: 0,
            blurred: 1,
            textures: 3,
        },
        "the content's texture and the two the blur ping-pongs between"
    );

    assert_eq!(
        second.draw_calls,
        DrawCalls {
            images: 1,
            backdrop: 3,
            ..DrawCalls::default()
        }
    );
    assert_eq!(
        second.layers,
        LayerWork {
            drawn: 0,
            kept: 1,
            blurred: 1,
            textures: 0,
        }
    );
    assert_eq!(
        second.backdrop_allocations,
        GpuAllocations::default(),
        "the blur's passes reuse the uniforms and bind groups the first frame made"
    );
}

/// A layer's texture outlives the layer by as many painted frames as the
/// backdrop's target does, in the pool, so a popup that fades in every time
/// it opens draws into the texture it had — and is then given back.
///
/// Two stages of a hundred and twenty: a layer no frame names is released
/// into the pool, and a texture nobody takes from the pool is dropped.
#[test]
fn a_layer_s_texture_is_kept_while_it_might_be_wanted_and_then_released() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let opacity = create_signal(0.5);
    let poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .background(poke)
                .layout(Flex::row())
                .child(fading_card(opacity, create_signal(0.0), false))
        },
    );
    app.configure(surface, 240, 20, 1.0);
    app.step();
    assert_eq!(app.layer_textures(), 1, "the card's texture");

    // No longer a layer, at full opacity.
    opacity.set(1.0);
    let mut frames = 0;
    while app.layer_textures() > 0 {
        frames += 1;
        assert!(
            frames <= 300,
            "a texture no layer has named is never released"
        );
        poke.set(Color::rgb(0.1, 0.1, (frames % 2) as f32 * 0.1));
        app.step();
    }
    assert!(
        frames > 120,
        "released after {frames} frames, before the idle frames that keep a \
         popup that opens again from allocating again"
    );
}

/// A layer's texture is kept for idle frames of its own surface, not of
/// every surface: a still panel on one surface beside another animating at
/// full rate keeps the texture it would be drawn from when it next paints.
#[test]
fn a_layer_s_texture_idles_by_its_own_surface_s_frames() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let opacity = create_signal(0.5);
    let still = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || container().child(fading_card(opacity, create_signal(0.0), false)),
    );
    app.configure(still, 240, 20, 1.0);
    let poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let busy = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT),
        move || container().width(40.0).height(20.0).background(poke),
    );
    app.configure(busy, 240, 20, 1.0);
    app.step();
    assert_eq!(app.layer_textures(), 1, "the still panel's texture");

    for frame in 0..300 {
        poke.set(Color::rgb(0.1, 0.1, (frame % 2) as f32 * 0.1));
        app.step();
    }
    assert_eq!(
        app.layer_textures(),
        1,
        "three hundred frames of another surface released the still panel's \
         texture, which its own surface never stopped needing"
    );
}

/// A layer moved by whole device pixels is only composited again; moved by
/// a new fraction of one, its content is drawn again at the new alignment.
///
/// The move is an ancestor's — the container `fading_card` wraps the card in
/// — so the card itself never repaints: what is asked is whether its flatten
/// entry may be replayed at the new offset, which a whole pixel allows and a
/// fraction does not.
#[test]
fn a_layer_moved_by_whole_pixels_is_only_composited_again() {
    let x = create_signal(0.0f32);
    let opacity = create_signal(0.5);
    let blur = create_signal(0.0);
    let Some((_, whole)) = two_frames(
        move || {
            // Well inside the surface, so the move changes nothing of what
            // the surface's edge cuts off the layer.
            container()
                .padding([0.0, 20.0])
                .child(fading_card(opacity, blur, false).translate(move || (x.get(), 0.0)))
        },
        move || x.set(3.0),
    ) else {
        return;
    };
    assert_eq!(
        (whole.layers.drawn, whole.layers.kept),
        (0, 1),
        "three whole pixels across: the same content, laid down again"
    );

    // Signals of their own: the first application's went with it.
    let x = create_signal(0.0f32);
    let opacity = create_signal(0.5);
    let blur = create_signal(0.0);
    let Some((_, fraction)) = two_frames(
        move || {
            // Well inside the surface, so the move changes nothing of what
            // the surface's edge cuts off the layer.
            container()
                .padding([0.0, 20.0])
                .child(fading_card(opacity, blur, false).translate(move || (x.get(), 0.0)))
        },
        move || x.set(0.5),
    ) else {
        return;
    };
    assert_eq!(
        (fraction.layers.drawn, fraction.layers.kept),
        (1, 0),
        "half a pixel across: drawn again, on the surface's grid where it now stands"
    );
}

/// A frosted panel fading is not made a layer: a frost inside one would see
/// only what the layer drew, and pop back to frosting the surface when the
/// fade ends. It fades draw by draw, its frost with it.
#[test]
fn a_fading_frost_is_not_made_a_layer() {
    let Some(stats) = first_frame(|| {
        container().child(
            container()
                .width(120.0)
                .height(18.0)
                .background(Color::rgba(1.0, 1.0, 1.0, 0.2))
                .backdrop_blur(BackdropBlur::new(8.0).sources(BackdropSources::SURFACE))
                .opacity(0.5)
                .child(text("frosted")),
        )
    }) else {
        return;
    };
    assert_eq!(stats.layers, LayerWork::default(), "no layer");
    assert_eq!(
        stats.draw_calls.backdrop, 5,
        "the frost, faded with the rest: four draws and the present"
    );
}

/// A frost of its own, `width` wide, over whatever was drawn before it.
fn frost(width: f32) -> Container {
    container()
        .width(width)
        .height(16.0)
        .backdrop_blur(BackdropBlur::new(4.0).sources(BackdropSources::SURFACE))
}

/// Backdrops side by side in one group share their passes: one shrink and
/// one blur each way for all of them, with a draw per region in each, and
/// what they lay back drawn in the pass the group resumes. Three passes and
/// the present, however many there are — not four for each (#766).
#[test]
fn backdrops_that_do_not_overlap_share_three_passes() {
    const FROSTS: u64 = 6;
    let Some(stats) = first_frame(|| {
        container()
            .background(Color::rgb(0.1, 0.1, 0.1))
            .layout(Flex::row())
            .children((0..FROSTS).map(|_| frost(40.0)))
    }) else {
        return;
    };
    assert_eq!(stats.backdrop_passes, 3 + 1);
    assert_eq!(
        stats.draw_calls.backdrop,
        4 * FROSTS + 1,
        "a shrink, a blur each way and a composite for each frost, and the present"
    );
}

/// Backdrops that overlap cannot share: the later one has to read the earlier
/// one's frost, which is not there until the earlier is laid back. So they
/// split into batches in paint order, three passes each.
#[test]
fn overlapping_backdrops_split_into_batches() {
    let Some(stats) = first_frame(|| {
        container()
            .background(Color::rgb(0.1, 0.1, 0.1))
            .layout(ZStack::new())
            .child(frost(42.0))
            .child(frost(40.0).translate((40.0, 0.0)))
            .child(frost(40.0).translate((80.0, 0.0)))
    }) else {
        return;
    };
    assert_eq!(
        stats.backdrop_passes,
        2 * 3 + 1,
        "the second, two pixels over the first, opens a second batch, which the third \
         beside it joins"
    );
}

/// Inside a layer the frosts filter the layer's texture, and those of one of
/// its groups batch as the surface's do: three passes for all of them.
#[test]
fn backdrops_inside_a_layer_share_three_passes() {
    let Some(stats) = first_frame(|| {
        container().child(
            container()
                .background(Color::rgb(0.1, 0.1, 0.1))
                .layout(Flex::row())
                .children((0..4).map(|_| frost(40.0)))
                .blur(0.0),
        )
    }) else {
        return;
    };
    assert_eq!(stats.layers.drawn, 1, "the frosts are inside a layer");
    assert_eq!(stats.backdrop_passes, 3);
}

/// A frost too thin to shrink still takes a working texel, and at the far
/// edge of a surface whose width is not a multiple of the shrink that texel
/// is past the working texture's last one. The slot is moved back inside it,
/// and the column is frosted: red over blue, blurred where they meet, rather
/// than drawn from a texel nobody wrote and left as sharp as it was.
#[test]
fn a_frost_too_thin_to_shrink_at_the_surface_s_edge_is_blurred_inside_it() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        || {
            let half = |colour: Color| container().width(fill()).height(10.0).background(colour);
            container().layout(ZStack::new()).children([
                container()
                    .width(fill())
                    .layout(Flex::column())
                    .child(half(Color::rgb(1.0, 0.0, 0.0)))
                    .child(half(Color::rgb(0.0, 0.0, 1.0)))
                    .into_any(),
                container()
                    .layout(Flex::row())
                    .child(frost(40.0))
                    .child(container().width(201.0))
                    .child(frost(1.0))
                    .into_any(),
            ])
        },
    );
    app.configure(surface, 242, 20, 1.0);
    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();
    assert_eq!(stats.frames_painted, 1, "the frame was not painted");
    assert_eq!(stats.backdrop_passes, 3 + 1, "the two share a batch");
    let [r, _, b, _] = app.read_pixel(surface, 241, 9);
    assert!(
        r < 250 && b > 5,
        "the thin frost reads [{r}, _, {b}] above the seam: not blurred"
    );
}

/// A card that is a layer, beside a box whose colour forces a frame without
/// touching the card — so the card is laid down from its texture, replayed.
fn card_beside(poke: RwSignal<Color>) -> Container {
    container()
        .layout(Flex::row().spacing(8.0))
        .child(fading_card(create_signal(0.5), create_signal(0.0), false))
        .child(container().width(16.0).height(16.0).background(poke))
}

/// A layer replayed from a texture that is not there any more is drawn again
/// in the same frame, rather than missing from it: the frame is flattened
/// again before it is drawn, with the layer's content.
#[test]
fn a_layer_whose_texture_is_gone_is_drawn_again_in_the_same_frame() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || card_beside(poke),
    );
    app.configure(surface, 240, 20, 1.0);
    app.step();
    app.forget_layer_textures();

    poke.set(Color::rgb(0.2, 0.2, 0.2));
    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();
    assert_eq!(stats.frames_painted, 1);
    assert_eq!(
        (stats.layers.drawn, stats.draw_calls.images),
        (1, 1),
        "the card's texture was gone and the frame went without it"
    );
    assert_eq!(app.layer_textures(), 1, "and it has one again");
}

/// A layer no surface has named for thirty times its own surface's patience
/// is let go even while its surface stands still — what releases a texture
/// whose surface has gone — and drawn again when that surface next paints.
#[test]
fn a_layer_no_surface_names_is_released_and_drawn_again_when_it_is() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let still_poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let still = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || card_beside(still_poke),
    );
    app.configure(still, 240, 20, 1.0);
    let poke = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let busy = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT),
        move || container().width(40.0).height(20.0).background(poke),
    );
    app.configure(busy, 240, 20, 1.0);
    app.step();
    assert_eq!(app.layer_textures(), 1);

    let mut frames = 0;
    while app.layer_textures() > 0 {
        frames += 1;
        assert!(
            frames <= 4000,
            "a texture no surface names is kept for good"
        );
        poke.set(Color::rgb(0.1, 0.1, (frames % 2) as f32 * 0.1));
        app.step();
    }
    assert!(
        frames > 120 * 30,
        "released after {frames} frames of another surface, sooner than its \
         own surface's patience thirty times over"
    );

    still_poke.set(Color::rgb(0.2, 0.2, 0.2));
    render_stats::reset_stats();
    app.step();
    let stats = render_stats::get_stats();
    assert_eq!(
        (stats.layers.drawn, stats.draw_calls.images),
        (1, 1),
        "the still surface painted again and its card was not drawn"
    );
}
