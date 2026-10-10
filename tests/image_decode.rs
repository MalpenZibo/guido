#![cfg(feature = "testing")]
//! A raster image is decoded off the frame that first draws it (#507).
//!
//! The first frame goes out with the box laid out at the image's size and
//! nothing in it; the decode runs on a worker, and the loop draws the image on
//! a later frame with nothing from the application to prompt it.
//!
//! A binary of its own rather than a part of `tests/headless_app.rs`, and its
//! tests taken one at a time: the decode lands through the background-write
//! queue, which is process-wide. Every `Headless::step` drains it on whichever
//! thread steps — so beside tests that step in parallel, a decode written by
//! one test's worker would be applied to another test's thread. An application
//! has one loop in its process; a test binary has as many as it has threads.

use std::cell::Cell;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use guido::prelude::*;
use guido::testing::Headless;

mod common;
use common::headless;

/// One test at a time; see the module docs for why.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const BACKDROP: Color = Color::rgb(0.0, 0.0, 1.0);

/// A 20×20 PNG of one opaque colour, encoded here so the test depends on no
/// asset and the colour it should find is the one it wrote.
fn png(rgb: [u8; 3]) -> Vec<u8> {
    let pixels =
        ::image::RgbaImage::from_pixel(20, 20, ::image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut encoded = Vec::new();
    pixels
        .write_to(&mut Cursor::new(&mut encoded), ::image::ImageFormat::Png)
        .expect("a PNG encodes");
    encoded
}

fn sized_png(width: u32, height: u32) -> Vec<u8> {
    let pixels = ::image::RgbaImage::from_pixel(width, height, ::image::Rgba([255, 0, 0, 255]));
    let mut encoded = Cursor::new(Vec::new());
    pixels
        .write_to(&mut encoded, ::image::ImageFormat::Png)
        .unwrap();
    encoded.into_inner()
}

fn upload_limit_image(source: ImageSource, fit: ContentFit) {
    let Some(mut app) = headless() else {
        return;
    };
    let green = ImageSource::Rgba {
        width: 1,
        height: 1,
        pixels: Arc::from([0, 255, 0, 255]),
    };
    let surface =
        app.surface(
            SurfaceConfig::new().height(21).background_color(BACKDROP),
            move || {
                container().layout(Flex::row()).children(
                    [source, green]
                        .into_iter()
                        .enumerate()
                        .map(|(index, source)| {
                            container()
                                .width(21.0)
                                .height(21.0)
                                .overflow(Overflow::Hidden)
                                .child(image(source).content_fit(if index == 0 {
                                    fit
                                } else {
                                    ContentFit::Fill
                                }))
                        }),
                )
            },
        );
    app.configure(surface, 63, 21, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    let started = app.image_decodes_started();
    assert_eq!(
        app.image_textures(),
        2,
        "the image and its neighbor uploaded"
    );
    for width in [64, 63, 64, 63] {
        app.configure(surface, width, 21, 1.0);
        app.step();
        app.wait_for_image_decodes();
        app.step();
        assert!(
            is_red(app.read_pixel(surface, 10, 0)),
            "image was not drawn: {:?}",
            app.read_pixel(surface, 10, 0)
        );
        let neighbor = app.read_pixel(surface, 31, 10);
        assert!(
            neighbor[1] > 200 && neighbor[0] < 50,
            "neighbor changed: {neighbor:?}"
        );
        assert_eq!(app.image_decodes_started(), started, "redraw decoded again");
        assert_eq!(app.image_textures(), 2, "redraw uploaded another texture");
    }
}

fn limit_raster(width: u32, height: u32, encoded: bool) -> ImageSource {
    if encoded {
        ImageSource::Bytes(sized_png(width, height).into())
    } else {
        ImageSource::Rgba {
            width,
            height,
            pixels: [255, 0, 0, 255].repeat((width * height) as usize).into(),
        }
    }
}

#[test]
fn strongly_shrunk_rasters_keep_thin_translucent_lines() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    for encoded in [false, true] {
        for (width, height) in [(limit * 5, 1), (1, limit * 5)] {
            let Some(mut app) = headless() else { return };
            let pixels: Vec<u8> = (0..width * height)
                .flat_map(|x| if x % 5 == 0 { [255, 0, 0, 128] } else { [0; 4] })
                .collect();
            let source = if encoded {
                let raster = ::image::RgbaImage::from_raw(width, height, pixels).unwrap();
                let mut bytes = Cursor::new(Vec::new());
                raster
                    .write_to(&mut bytes, ::image::ImageFormat::Png)
                    .unwrap();
                ImageSource::Bytes(bytes.into_inner().into())
            } else {
                ImageSource::Rgba {
                    width,
                    height,
                    pixels: pixels.into(),
                }
            };
            let surface = app.surface(
                SurfaceConfig::new().height(32).background_color(BACKDROP),
                move || {
                    container()
                        .width(32.0)
                        .height(32.0)
                        .child(image(source).content_fit(ContentFit::Fill))
                },
            );
            app.configure(surface, 32, 32, 1.0);
            app.step();
            app.wait_for_image_decodes();
            app.step();
            for extent in [33, 32, 33, 32] {
                app.configure(surface, extent, 32, 1.0);
                app.step();
                app.wait_for_image_decodes();
                app.step();
                assert_eq!(app.read_pixel(surface, 16, 16), [26, 0, 229, 255]);
                assert_eq!(app.image_textures(), 1);
                assert_eq!(app.image_decodes_started(), u64::from(encoded));
            }
        }
    }
}

#[test]
fn nearly_unscaled_rasters_keep_distinct_rows_and_columns() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    for encoded in [false, true] {
        for (width, height) in [(limit, 2), (limit + 1, 2), (2, limit), (2, limit + 1)] {
            let Some(mut app) = headless() else { return };
            let mut pixels = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    pixels.extend_from_slice(
                        if (height == 2 && y == 0) || (width == 2 && x == 0) {
                            &[255, 0, 0, 255]
                        } else {
                            &[0, 0, 255, 255]
                        },
                    );
                }
            }
            let source = if encoded {
                let pixels = ::image::RgbaImage::from_raw(width, height, pixels).unwrap();
                let mut bytes = Cursor::new(Vec::new());
                pixels
                    .write_to(&mut bytes, ::image::ImageFormat::Png)
                    .unwrap();
                ImageSource::Bytes(bytes.into_inner().into())
            } else {
                ImageSource::Rgba {
                    width,
                    height,
                    pixels: pixels.into(),
                }
            };
            let (box_width, box_height) = if height == 2 { (21, 2) } else { (2, 21) };
            let surface = app.surface(SurfaceConfig::new().height(box_height), move || {
                container()
                    .width(box_width as f32)
                    .height(box_height as f32)
                    .overflow(Overflow::Hidden)
                    .child(image(source).content_fit(ContentFit::None))
            });
            app.configure(surface, box_width, box_height, 1.0);
            app.step();
            app.wait_for_image_decodes();
            app.step();
            let (red, blue) = if height == 2 {
                ((10, 0), (10, 1))
            } else {
                ((0, 10), (1, 10))
            };
            assert_eq!(app.read_pixel(surface, red.0, red.1), [255, 0, 0, 255]);
            assert_eq!(app.read_pixel(surface, blue.0, blue.1), [0, 0, 255, 255]);
        }
    }
}

/// Oversized width must not panic (#603).
#[test]
fn rasters_wider_than_the_texture_limit_draw_without_redecoding() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    for encoded in [false, true] {
        upload_limit_image(limit_raster(limit + 1, 1, encoded), ContentFit::Fill);
    }
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wide-upload-limit.png");
    std::fs::write(&path, sized_png(limit + 1, 1)).unwrap();
    upload_limit_image(ImageSource::Path(path), ContentFit::Fill);
}

/// Oversized height must not panic (#603).
#[test]
fn rasters_taller_than_the_texture_limit_draw_without_redecoding() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    for encoded in [false, true] {
        upload_limit_image(limit_raster(1, limit + 1, encoded), ContentFit::Fill);
    }
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tall-upload-limit.png");
    std::fs::write(&path, sized_png(1, limit + 1)).unwrap();
    upload_limit_image(ImageSource::Path(path), ContentFit::Fill);
}

/// Both axes allow the exact limit (#603).
#[test]
fn rasters_exactly_at_the_texture_limit_still_draw() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    for (width, height) in [(limit, 1), (1, limit)] {
        for encoded in [false, true] {
            upload_limit_image(limit_raster(width, height, encoded), ContentFit::Fill);
        }
    }
}

#[cfg(feature = "svg")]
fn limit_svg(width: u32, height: u32) -> ImageSource {
    ImageSource::SvgBytes(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="#f00"/></svg>"##
    ).into_bytes().into())
}

/// Quality-scaled SVG width stays bounded (#603).
#[cfg(feature = "svg")]
#[test]
fn svgs_wider_than_the_texture_limit_draw_at_intrinsic_size() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    upload_limit_image(limit_svg(limit + 1, 1), ContentFit::None);
}

/// Quality-scaled SVG height stays bounded (#603).
#[cfg(feature = "svg")]
#[test]
fn svgs_taller_than_the_texture_limit_draw_at_intrinsic_size() {
    let _serial = serial();
    let limit = wgpu::Limits::default().max_texture_dimension_2d;
    upload_limit_image(limit_svg(1, limit + 1), ContentFit::None);
}

/// Clamping keeps shared and fitted SVGs usable (#603).
#[cfg(feature = "svg")]
#[test]
fn a_clamped_svg_can_be_shared_and_fitted_again() {
    let _serial = serial();
    for (width, height) in [(8193, 1), (1, 8193)] {
        let Some(mut app) = headless() else { return };
        let source = limit_svg(width, height);
        let fit = create_signal(ContentFit::None);
        let surface = app.surface(
            SurfaceConfig::new().height(21).background_color(BACKDROP),
            move || {
                container().layout(Flex::row()).children([
                    container()
                        .width(21.0)
                        .height(21.0)
                        .overflow(Overflow::Hidden)
                        .child(image(source.clone()).content_fit(fit)),
                    container()
                        .width(21.0)
                        .height(21.0)
                        .child(image(source).content_fit(ContentFit::Fill)),
                ])
            },
        );
        app.configure(surface, 42, 21, 1.0);
        app.step();
        app.wait_for_image_decodes();
        app.step();
        assert!(is_red(app.read_pixel(surface, 10, 0)));
        assert!(is_red(app.read_pixel(surface, 31, 10)));
        fit.set(ContentFit::Fill);
        app.step();
        app.wait_for_image_decodes();
        app.step();
        assert!(is_red(app.read_pixel(surface, 10, 10)));
        assert!(is_red(app.read_pixel(surface, 31, 10)));
        let started = app.image_decodes_started();
        for width in [43, 42, 43, 42] {
            app.configure(surface, width, 21, 1.0);
            app.step();
            assert_eq!(app.image_decodes_started(), started);
            assert!(is_red(app.read_pixel(surface, 10, 10)));
            assert!(is_red(app.read_pixel(surface, 31, 10)));
        }
    }
}

fn png_file(name: &str, rgb: [u8; 3]) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, png(rgb)).expect("the PNG is written");
    path
}

/// A strip of square images `side` across, side by side on the backdrop,
/// starting at x = 0.
fn strip(sources: Vec<ImageSource>, side: u32, ready: Rc<Cell<Option<Signal<bool>>>>) -> Container {
    container()
        .layout(Flex::row())
        .children(sources.into_iter().map(move |source| {
            let image = image(source).content_fit(ContentFit::Fill);
            ready.set(Some(image.ready()));
            container()
                .width(side as f32)
                .height(side as f32)
                .child(image)
        }))
}

/// A surface three images wide, showing `sources` at `side` each.
fn surface(app: &mut Headless, sources: Vec<ImageSource>, side: u32) -> (SurfaceId, Signal<bool>) {
    let ready = Rc::new(Cell::new(None));
    let captured = ready.clone();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(side)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || strip(sources, side, captured),
    );
    app.configure(surface, 3 * side, side, 1.0);
    (surface, ready.get().expect("an image was built"))
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[2] < 50
}

fn is_backdrop(pixel: [u8; 4]) -> bool {
    pixel[0] < 50 && pixel[2] > 200
}

/// The first frame is presented while the decode is still pending, and the
/// image appears on a later one with no input and no write from the
/// application — only the loop turning.
///
/// Red before #507: the renderer decoded inside the frame, so the first frame
/// already held the image, and held the frame until it had it.
fn a_pending_image_is_drawn_on_a_later_frame(source: ImageSource, side: u32) {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let hold = app.hold_image_decodes();
    let (surface, ready) = surface(&mut app, vec![source], side);

    app.step();
    assert_eq!(
        app.frames_presented(surface),
        1,
        "the first frame went out while the decode was held"
    );
    assert!(
        is_backdrop(app.read_pixel(surface, 10, 10)),
        "nothing is drawn before the decode lands, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert!(!ready.get_untracked(), "not ready on the first frame");

    hold.release();
    app.wait_for_image_decodes();
    app.step();

    assert_eq!(
        app.frames_presented(surface),
        2,
        "the decode asked for a frame"
    );
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "the decoded image is drawn, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert!(ready.get_untracked(), "ready once the image is drawn");
}

#[test]
fn an_image_file_is_drawn_on_the_frame_after_its_decode() {
    a_pending_image_is_drawn_on_a_later_frame(
        ImageSource::Path(png_file("decoded-later.png", [255, 0, 0])),
        20,
    );
}

#[test]
fn image_bytes_are_drawn_on_the_frame_after_their_decode() {
    a_pending_image_is_drawn_on_a_later_frame(ImageSource::Bytes(png([255, 0, 0]).into()), 20);
}

fn raster_file(format: ::image::ImageFormat, extension: &str, scenario: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{scenario}-{format:?}"))
        .with_extension(extension);
    let pixels = ::image::RgbImage::from_pixel(20, 20, ::image::Rgb([255, 0, 0]));
    let mut encoded = Cursor::new(Vec::new());
    pixels.write_to(&mut encoded, format).unwrap();
    std::fs::write(&path, encoded.into_inner()).unwrap();
    path
}

#[test]
fn raster_file_dimensions_follow_the_contents() {
    for format in [::image::ImageFormat::Png, ::image::ImageFormat::Jpeg] {
        for extension in ["png", "jpg", "dat", ""] {
            let source = ImageSource::Path(raster_file(format, extension, "dimensions"));
            assert_eq!(
                guido::image_metadata::get_intrinsic_size(&source),
                Some((20.0, 20.0)),
                "{format:?} dimensions with extension {extension:?}"
            );
        }
    }
}

#[test]
fn raster_files_are_drawn_regardless_of_their_extensions() {
    for format in [::image::ImageFormat::Png, ::image::ImageFormat::Jpeg] {
        for extension in ["png", "jpg", "dat", ""] {
            a_pending_image_is_drawn_on_a_later_frame(
                ImageSource::Path(raster_file(format, extension, "drawn")),
                20,
            );
        }
    }
}

#[test]
fn unreadable_raster_files_stay_failed() {
    let _serial = serial();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let corrupt = root.join("corrupt-image.png");
    std::fs::write(&corrupt, b"not an image").unwrap();
    for path in [root.join("missing-image-539.png"), corrupt] {
        let source = ImageSource::Path(path);
        assert_eq!(guido::image_metadata::get_intrinsic_size(&source), None);
        let Some(mut app) = headless() else { return };
        let (surface, ready) = surface(&mut app, vec![source], 20);
        app.step();
        app.wait_for_image_decodes();
        app.step();
        assert!(!ready.get_untracked());
        assert_eq!(app.image_decodes_started(), 0);
        assert!(is_backdrop(app.read_pixel(surface, 10, 10)));
    }
}

#[test]
fn a_corrupt_raster_payload_never_becomes_ready() {
    let _serial = serial();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("corrupt-payload.png");
    let mut bytes = png([255, 0, 0]);
    bytes.truncate(bytes.len() / 2);
    std::fs::write(&path, bytes).unwrap();
    let source = ImageSource::Path(path);
    assert_eq!(
        guido::image_metadata::get_intrinsic_size(&source),
        Some((20.0, 20.0))
    );
    let Some(mut app) = headless() else { return };
    let (surface, ready) = surface(&mut app, vec![source], 20);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    assert!(!ready.get_untracked());
    assert_eq!(app.image_decodes_started(), 1);
    assert!(is_backdrop(app.read_pixel(surface, 10, 10)));
}

/// Two images of one source share one decode, and both are drawn by it.
fn two_images_of_one_source_decode_it_once(first: ImageSource, second: ImageSource, side: u32) {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (surface, _) = surface(&mut app, vec![first, second], side);

    app.step();
    app.wait_for_image_decodes();
    app.step();

    assert_eq!(app.image_decodes_started(), 1, "one decode for both");
    for x in [side / 2, side * 3 / 2] {
        assert!(
            is_red(app.read_pixel(surface, x, 10)),
            "the image at x = {x} is drawn, got {:?}",
            app.read_pixel(surface, x, 10)
        );
    }
}

#[test]
fn two_images_of_one_file_decode_it_once() {
    let path = png_file("decoded-once.png", [255, 0, 0]);
    two_images_of_one_source_decode_it_once(
        ImageSource::Path(path.clone()),
        ImageSource::Path(path),
        20,
    );
}

/// Two buffers, not one `Arc` handed twice: equal bytes are one source
/// whoever allocated them.
#[test]
fn two_images_of_equal_bytes_decode_them_once() {
    let first: Arc<[u8]> = png([255, 0, 0]).into();
    let second: Arc<[u8]> = png([255, 0, 0]).into();
    assert!(!Arc::ptr_eq(&first, &second));
    two_images_of_one_source_decode_it_once(
        ImageSource::Bytes(first),
        ImageSource::Bytes(second),
        20,
    );
}

/// A source that needs no decode is ready from the start and drawn on the
/// first frame — which is what `Rgba` is for.
#[test]
fn raw_pixels_are_ready_and_drawn_on_the_first_frame() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let _hold = app.hold_image_decodes();
    let red: Vec<u8> = [255, 0, 0, 255].repeat(20 * 20);
    let (surface, ready) = surface(
        &mut app,
        vec![ImageSource::Rgba {
            width: 20,
            height: 20,
            pixels: red.into(),
        }],
        20,
    );

    app.step();

    assert!(ready.get_untracked());
    assert!(is_red(app.read_pixel(surface, 10, 10)));
    assert_eq!(app.image_decodes_started(), 0);
}

/// Once the renderer has uploaded an image the cache lets go of its pixels —
/// iced's `Memory::Host` becoming `Memory::Device` — and the image is still
/// ready and still drawn.
///
/// Red before: the entry kept the decoded RGBA for as long as an image showed
/// it, 19 MB for a 2912×1632 wallpaper, next to the texture holding the same.
#[test]
fn the_pixels_go_once_the_renderer_has_uploaded_them() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let path = png_file("uploaded-then-dropped.png", [255, 0, 0]);
    let (surface, ready) = surface(&mut app, vec![ImageSource::Path(path)], 20);

    app.step();
    app.wait_for_image_decodes();
    app.step();

    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "the image is drawn"
    );
    assert!(ready.get_untracked(), "and ready");
    assert_eq!(
        app.image_bytes_held(),
        0,
        "the decoded pixels went with the upload"
    );
}

/// A source whose pixels went to the renderer is still one decode: an image of
/// it mounted afterwards — here in place of the first, so nothing holds the
/// entry in between — is drawn on its first frame, from the texture.
///
/// Red before: the entry went with the last image that held it, so the
/// second one started a decode of its own and drew nothing until it landed.
#[test]
fn an_image_mounted_after_the_upload_is_drawn_from_the_texture() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let path = png_file("mounted-after-upload.png", [255, 0, 0]);
    let generation = create_signal(0u32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || {
            container().child(move || {
                let _ = generation.get();
                container()
                    .width(20.0)
                    .height(20.0)
                    .child(image(path.clone()).content_fit(ContentFit::Fill))
            })
        },
    );
    app.configure(surface, 60, 20, 1.0);

    app.step();
    app.wait_for_image_decodes();
    app.step();
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "the first is drawn"
    );

    generation.set(1);
    app.step();

    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "the second is drawn on its first frame, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert_eq!(app.image_decodes_started(), 1, "and decoded nothing");
}

/// A texture that is gone when its image is drawn again sends the source back
/// to pending and to the worker, and the image comes back through the same
/// repaint as the first time. In between nothing is drawn: there are no
/// pixels left to draw from.
#[test]
fn an_image_whose_texture_is_gone_is_decoded_again() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let path = png_file("texture-gone.png", [255, 0, 0]);
    let backdrop = create_signal(BACKDROP);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .width(fill())
                .height(fill())
                .background(backdrop)
                .child(
                    container()
                        .width(20.0)
                        .height(20.0)
                        .child(image(path).content_fit(ContentFit::Fill)),
                )
        },
    );
    app.configure(surface, 60, 20, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    assert!(is_red(app.read_pixel(surface, 10, 10)));

    app.forget_image_textures();
    // Something else asks for a frame, which draws the image again.
    backdrop.set(Color::rgb(0.0, 0.0, 0.9));
    app.step();
    app.step();
    assert!(
        !is_red(app.read_pixel(surface, 10, 10)),
        "nothing to draw until the decode lands again"
    );

    app.wait_for_image_decodes();
    app.step();
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "drawn again, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert_eq!(app.image_decodes_started(), 2, "one decode more");
    assert_eq!(app.image_bytes_held(), 0, "and let go of again");
}

/// A square SVG `side` across, of one opaque colour.
#[cfg(feature = "svg")]
fn svg(fill: &str, side: u32) -> Vec<u8> {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}"><rect width="{side}" height="{side}" fill="{fill}"/></svg>"#
    )
    .into_bytes()
}

/// A box whose raster is too large to draw inside the frame: 100 logical
/// pixels is 200 rasterized, 160 KB against the 64 KB line.
#[cfg(feature = "svg")]
const LARGE: u32 = 100;

/// An icon is rasterized inside the frame that first draws it, as Chromium,
/// iced, Qt and Android draw small vectors: it is ready, drawn, and the worker
/// never hears of it.
///
/// Red before: every SVG went to the worker, so an icon was blank on its first
/// frame and repainted when it landed.
#[cfg(feature = "svg")]
#[test]
fn a_small_svg_is_drawn_on_its_first_frame() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let _hold = app.hold_image_decodes();
    let (surface, ready) = surface(
        &mut app,
        vec![ImageSource::SvgBytes(svg("#f00", 20).into())],
        20,
    );

    app.step();

    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "drawn on the first frame, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert!(ready.get_untracked(), "and ready");
    assert_eq!(app.image_decodes_started(), 0, "with no worker");
}

/// Whether an SVG is ready is what the renderer drew, not what its intrinsic
/// size suggested. A large document shown as an icon — a 512-unit viewBox at
/// 20 px — is guessed not ready, is drawn inline on its first frame, and is
/// ready from the next with no worker involved.
///
/// Red without the renderer's report: nothing else would ever make it ready,
/// and an icon faded in on `ready()` would stay invisible.
#[cfg(feature = "svg")]
#[test]
fn a_large_document_drawn_as_an_icon_becomes_ready() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (surface, ready) = surface(
        &mut app,
        vec![ImageSource::SvgBytes(svg("#f00", 512).into())],
        20,
    );

    app.step();
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "drawn on the first frame, got {:?}",
        app.read_pixel(surface, 10, 10)
    );
    assert!(!ready.get_untracked(), "though guessed not ready");

    app.step();
    assert!(
        ready.get_untracked(),
        "ready once the renderer said it drew it"
    );
    assert_eq!(app.image_decodes_started(), 0, "with no worker");
}

/// The other wrong guess: a small document shown large is guessed ready, draws
/// nothing on its first frame while its raster is on the worker, and is not
/// ready until that raster lands.
///
/// Red without the renderer's report: it would say ready over a blank box.
#[cfg(feature = "svg")]
#[test]
fn a_small_document_drawn_large_is_not_ready_until_its_raster_lands() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let hold = app.hold_image_decodes();
    let (surface, ready) = surface(
        &mut app,
        vec![ImageSource::SvgBytes(svg("#f00", 20).into())],
        LARGE,
    );

    app.step();
    assert!(is_backdrop(app.read_pixel(surface, 10, 10)), "blank");
    app.step();
    assert!(!ready.get_untracked(), "and not ready while blank");

    hold.release();
    app.wait_for_image_decodes();
    app.step();
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "drawn once it lands"
    );
    assert!(ready.get_untracked(), "and ready");
}

/// A large SVG is rasterized on the worker, like a raster source is decoded
/// there: its first frame draws nothing and it appears on a later one (#547).
///
/// Red before: the renderer read, parsed and rasterized it inside the frame,
/// so the first frame already held it, and held the frame until it had it.
#[cfg(feature = "svg")]
#[test]
fn svg_bytes_are_drawn_on_the_frame_after_their_raster() {
    a_pending_image_is_drawn_on_a_later_frame(
        ImageSource::SvgBytes(svg("#f00", LARGE).into()),
        LARGE,
    );
}

#[cfg(feature = "svg")]
#[test]
fn an_svg_file_is_drawn_on_the_frame_after_its_raster() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rasterized-later.svg");
    std::fs::write(&path, svg("#f00", LARGE)).expect("the SVG is written");
    a_pending_image_is_drawn_on_a_later_frame(ImageSource::SvgPath(path), LARGE);
}

/// Two images of one large SVG at one size share one raster.
///
/// Red before: an SVG never reached the worker, so nothing was counted.
#[cfg(feature = "svg")]
#[test]
fn two_svgs_of_one_source_rasterize_it_once() {
    two_images_of_one_source_decode_it_once(
        ImageSource::SvgBytes(svg("#f00", LARGE).into()),
        ImageSource::SvgBytes(svg("#f00", LARGE).into()),
        LARGE,
    );
}

/// A box that changes size needs a raster of the new size, and until it lands
/// the one it had is drawn stretched: a large SVG in a resize animation does
/// not blink out on every step.
///
/// Red before: the new size was rasterized inside the frame and the worker
/// never heard of it.
#[cfg(feature = "svg")]
#[test]
fn an_svg_resized_keeps_drawing_until_its_new_raster_lands() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    // Both sides: a raster is fitted to its box with the aspect kept, so a
    // square SVG in a box only wider needs no raster of its own.
    let side = create_signal(LARGE as f32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(2 * LARGE)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || {
            container().width(side).height(side).child(
                image(ImageSource::SvgBytes(svg("#f00", LARGE).into()))
                    .content_fit(ContentFit::Fill),
            )
        },
    );
    app.configure(surface, 2 * LARGE, 2 * LARGE, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    assert!(is_red(app.read_pixel(surface, 10, 10)), "drawn at {LARGE}");
    assert_eq!(app.image_decodes_started(), 1);

    let hold = app.hold_image_decodes();
    side.set((2 * LARGE) as f32);
    app.step();
    let inside_both_and_only_the_new = [10, LARGE * 3 / 2];
    for at in inside_both_and_only_the_new {
        assert!(
            is_red(app.read_pixel(surface, at, at)),
            "the old raster is drawn at ({at}, {at}) while the new one is pending, got {:?}",
            app.read_pixel(surface, at, at)
        );
    }
    assert_eq!(app.image_decodes_started(), 2, "a raster of the new size");

    hold.release();
    app.wait_for_image_decodes();
    app.step();
    for at in inside_both_and_only_the_new {
        assert!(
            is_red(app.read_pixel(surface, at, at)),
            "the new raster is drawn at ({at}, {at}), got {:?}",
            app.read_pixel(surface, at, at)
        );
    }
    assert_eq!(app.image_decodes_started(), 2, "and asked for once");
    assert_eq!(
        app.image_textures(),
        2,
        "and uploaded: the stretched one was not drawn for ever"
    );
}

/// One source at two sizes is two rasters, each made once: the frame that
/// uploads one size keeps the other, which an image beside it is about to
/// draw.
///
/// Red before: taking one size dropped every other size already landed, so
/// the second image asked for its raster again.
#[cfg(feature = "svg")]
#[test]
fn one_svg_at_two_sizes_rasterizes_each_once() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let source = ImageSource::SvgBytes(svg("#f00", LARGE).into());
    let (small, large) = (LARGE, LARGE * 3 / 2);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(large)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || {
            let boxed = |side: u32, source: ImageSource| {
                container()
                    .width(side as f32)
                    .height(side as f32)
                    .child(image(source).content_fit(ContentFit::Fill))
            };
            container()
                .layout(Flex::row())
                .child(boxed(small, source.clone()))
                .child(boxed(large, source))
        },
    );
    app.configure(surface, small + large, large, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    app.wait_for_image_decodes();
    app.step();

    assert_eq!(app.image_decodes_started(), 2, "one raster per size");
    for (x, y) in [(small / 2, small / 2), (small + large / 2, large / 2)] {
        assert!(
            is_red(app.read_pixel(surface, x, y)),
            "the image at ({x}, {y}) is drawn, got {:?}",
            app.read_pixel(surface, x, y)
        );
    }
}

/// What one of the 20×20 PNGs above costs the cache: small enough for an atlas
/// page, so its place there — the image and the one-texel gutter around it,
/// 22 × 22, at 4 bytes a texel (#546).
const IMAGE_BYTES: usize = 22 * 22 * 4;

/// Long enough that no texture drawn before it counts as drawn recently: the
/// renderer spares one drawn within the last second.
fn let_the_textures_go_idle() {
    std::thread::sleep(std::time::Duration::from_millis(1100));
}

/// A surface showing one of `sets` at a time, as a row of 4×4 images, over a
/// backdrop the test can change to ask for a frame without touching the
/// images.
struct Switcher {
    surface: SurfaceId,
    which: RwSignal<usize>,
    backdrop: RwSignal<Color>,
}

impl Switcher {
    fn new(app: &mut Headless, sets: Vec<Vec<ImageSource>>) -> Self {
        let width = sets.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32 * 4;
        let which = create_signal(0usize);
        let backdrop = create_signal(BACKDROP);
        let surface = app.surface(
            SurfaceConfig::new()
                .height(4)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .background(backdrop)
                    .child(move || {
                        container().layout(Flex::row()).children(
                            sets[which.get()].clone().into_iter().map(|source| {
                                container()
                                    .width(4.0)
                                    .height(4.0)
                                    .child(image(source).content_fit(ContentFit::Fill))
                            }),
                        )
                    })
            },
        );
        app.configure(surface, width, 4, 1.0);
        Self {
            surface,
            which,
            backdrop,
        }
    }

    /// Show set `which` and step until every decode it asked for is drawn.
    fn show(&self, app: &mut Headless, which: usize) {
        self.which.set(which);
        app.step();
        app.wait_for_image_decodes();
        app.step();
    }

    /// Ask for a frame that draws the same images again.
    fn redraw(&self, app: &mut Headless) {
        let current = self.backdrop.get_untracked();
        self.backdrop.set(if current == BACKDROP {
            Color::rgb(0.0, 0.0, 0.9)
        } else {
            BACKDROP
        });
        app.step();
    }

    /// The images of the set in view, by index, that are not drawn.
    fn blank(&self, app: &Headless, count: usize) -> Vec<usize> {
        (0..count)
            .filter(|i| is_backdrop(app.read_pixel(self.surface, *i as u32 * 4 + 2, 2)))
            .collect()
    }
}

fn sources(count: u8, green: u8) -> Vec<ImageSource> {
    (0..count)
        .map(|i| ImageSource::Bytes(png([200, i, green]).into()))
        .collect()
}

/// Two hundred small images, drawn once and taken off screen, are still
/// textures when they come back: together they are nowhere near the budget.
///
/// Red before #512: the cache held 64 textures whatever their size, and past
/// that evicted what had not been drawn for a second — 168 icons that cost
/// 1600 bytes each, decoded again when a launcher opened a second time.
#[test]
fn small_images_off_screen_stay_cached_while_under_the_budget() {
    const IMAGES: u8 = 200;
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let switcher = Switcher::new(&mut app, vec![sources(IMAGES, 0), Vec::new()]);
    switcher.show(&mut app, 0);
    assert_eq!(app.image_decodes_started(), u64::from(IMAGES));

    switcher.show(&mut app, 1);
    let_the_textures_go_idle();
    switcher.redraw(&mut app);
    switcher.redraw(&mut app);

    switcher.show(&mut app, 0);
    assert_eq!(
        switcher.blank(&app, usize::from(IMAGES)),
        Vec::<usize>::new(),
        "every image is drawn again"
    );
    assert_eq!(
        app.image_decodes_started(),
        u64::from(IMAGES),
        "and none was decoded twice"
    );
}

/// Show image A, then image B in its place, let both go idle and draw B again
/// under `budget`. Returns how many decodes it took to bring A back, after
/// checking that B — on screen throughout — was never evicted.
fn decodes_to_bring_back_an_idle_image(budget: usize) -> Option<u64> {
    let _serial = serial();
    let mut app = headless()?;
    guido::set_image_cache_budget(budget);
    let switcher = Switcher::new(&mut app, vec![sources(1, 0), sources(1, 100)]);
    switcher.show(&mut app, 0);
    switcher.show(&mut app, 1);
    assert_eq!(app.image_decodes_started(), 2);

    let_the_textures_go_idle();
    switcher.redraw(&mut app);
    switcher.redraw(&mut app);
    assert!(switcher.blank(&app, 1).is_empty(), "B, on screen, is drawn");
    assert_eq!(app.image_decodes_started(), 2, "and was not decoded again");

    switcher.show(&mut app, 0);
    assert!(switcher.blank(&app, 1).is_empty(), "A is drawn once back");
    Some(app.image_decodes_started() - 2)
}

/// With room for one image and a half, the image not drawn recently is
/// evicted and the one on screen is not.
///
/// Red before #512: two textures are nowhere near 64, so nothing was evicted
/// whatever they cost.
#[test]
fn over_the_budget_the_image_not_drawn_recently_is_evicted() {
    let Some(decodes) = decodes_to_bring_back_an_idle_image(IMAGE_BYTES * 3 / 2) else {
        return;
    };
    assert_eq!(decodes, 1, "A was evicted, and decoded again to come back");
}

/// The budget is the one the application set, to the byte: exactly two images
/// fit, and a byte less does not.
///
/// Red before #512: there was no budget to set, and two textures were never
/// evicted.
#[test]
fn the_budget_the_application_sets_is_the_one_used() {
    let Some(decodes) = decodes_to_bring_back_an_idle_image(IMAGE_BYTES * 2) else {
        return;
    };
    assert_eq!(decodes, 0, "two images fit a budget of two");
    let Some(decodes) = decodes_to_bring_back_an_idle_image(IMAGE_BYTES * 2 - 1) else {
        return;
    };
    assert_eq!(decodes, 1, "and a byte less evicts one");
}

/// Images on screen whose textures together exceed the budget all stay
/// drawn, each decoded once — even on a frame that comes after a second of
/// nothing, when none of them was drawn recently.
///
/// Red before #512: the cache evicted at the start of a frame, before the
/// frame had drawn anything, so after an idle second every texture in view
/// was old enough to go; 38 of these went blank and back to the worker.
#[test]
fn images_on_screen_over_the_budget_stay_drawn() {
    const IMAGES: u8 = 70;
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    guido::set_image_cache_budget(IMAGE_BYTES * 10);
    let switcher = Switcher::new(&mut app, vec![sources(IMAGES, 0)]);
    switcher.show(&mut app, 0);

    for idle in 0..2 {
        let_the_textures_go_idle();
        switcher.redraw(&mut app);
        app.wait_for_image_decodes();
        app.step();
        assert_eq!(
            switcher.blank(&app, usize::from(IMAGES)),
            Vec::<usize>::new(),
            "after idle second {idle}, images went blank"
        );
    }
    assert_eq!(
        app.image_decodes_started(),
        u64::from(IMAGES),
        "each decoded once"
    );
}

/// A decoded image is premultiplied before it is drawn, and its scaled edge
/// stays between its colour and what is under it (#556).
///
/// An orange disc with an antialiased rim, generated with straight alpha, is
/// drawn twice from the same 80-pixel pixels into a 40-pixel box: left encoded
/// as a PNG and decoded on the worker, right handed over as
/// `ImageSource::Rgba`, which is premultiplied at upload. They must match to
/// the bit — the decode premultiplies as the upload does — and every rim pixel
/// must lie on the line from the tile's colour to the disc's: filtering
/// premultiplied texels mixes colour weighted by coverage, where straight
/// texels would pull in the black of the empty ones around the disc.
///
/// Red without the decode's premultiply: the PNG's rim is added at full
/// colour, lighter than the line and than the raw pixels beside it.
#[test]
fn a_decoded_image_is_premultiplied_like_raw_pixels() {
    const SIDE: u32 = 80;
    const FILL: [u8; 3] = [242, 115, 64];
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let disc = ::image::RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        let centre = SIDE as f32 / 2.0;
        let distance =
            ((x as f32 + 0.5 - centre).powi(2) + (y as f32 + 0.5 - centre).powi(2)).sqrt();
        let coverage = (34.0 - distance).clamp(0.0, 1.0);
        ::image::Rgba([FILL[0], FILL[1], FILL[2], (coverage * 255.0).round() as u8])
    });
    let mut png = Vec::new();
    disc.write_to(&mut Cursor::new(&mut png), ::image::ImageFormat::Png)
        .expect("a PNG encodes");
    let sources = vec![
        ImageSource::Bytes(png.into()),
        ImageSource::Rgba {
            width: SIDE,
            height: SIDE,
            pixels: disc.into_raw().into(),
        },
    ];
    let surface = app.surface(
        SurfaceConfig::new()
            .height(40)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.92, 0.92, 0.88)),
        move || {
            container()
                .layout(Flex::row())
                .children(sources.clone().into_iter().map(|source| {
                    container()
                        .width(40.0)
                        .height(40.0)
                        .child(image(source).content_fit(ContentFit::Fill))
                }))
        },
    );
    app.configure(surface, 80, 40, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();

    let pixel = |x: u32, y: u32| app.read_pixel(surface, x, y);
    let differ: Vec<_> = (0..40)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(x, y) != pixel(x + 40, y))
        .collect();
    assert!(
        differ.is_empty(),
        "{} pixels of the decoded image differ from the same pixels handed over raw, \
         first at {:?}: decoded {:?}, raw {:?}",
        differ.len(),
        differ[0],
        pixel(differ[0].0, differ[0].1),
        pixel(differ[0].0 + 40, differ[0].1),
    );

    let background = pixel(0, 0);
    let along = |p: [u8; 4], c: usize| {
        (f32::from(p[c]) - f32::from(background[c]))
            / (f32::from(FILL[c]) - f32::from(background[c]))
    };
    let worst = (0..40)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .filter(|&p| along(p, 2) > 0.1)
        .map(|p| (along(p, 1) - along(p, 2)).abs())
        .fold(0.0, f32::max);
    assert!(
        worst < 0.05,
        "a rim pixel is {worst:.3} further along the line in green than in blue: \
         the scaled edge left the line from the tile to the disc"
    );
}

/// Two sources that differ in exactly one byte, and how far into the buffers
/// it is: the sources below are only a test of identity when that byte lies
/// outside the start, the middle and the end the texture cache once hashed.
fn assert_differ_off_the_samples(first: &[u8], second: &[u8]) {
    assert_eq!(first.len(), second.len());
    assert!(first.len() >= 1024, "a buffer this small is hashed whole");
    let differ: Vec<usize> = (0..first.len())
        .filter(|&i| first[i] != second[i])
        .collect();
    let (mid, len) = (first.len() / 2, first.len());
    assert!(
        !differ.is_empty()
            && differ
                .iter()
                .all(|&i| i >= 256 && !(mid - 128..mid + 128).contains(&i) && i < len - 256),
        "the sources must differ, and only off the sampled bytes: {differ:?} of {len}"
    );
}

fn is_green(pixel: [u8; 4]) -> bool {
    pixel[1] > 200 && pixel[0] < 50 && pixel[2] < 50
}

/// 40×40 opaque black but for one pixel, at (10, 10).
fn one_pixel_rgba(rgb: [u8; 3]) -> ImageSource {
    let mut pixels = [0, 0, 0, 255].repeat(40 * 40);
    let at = (10 * 40 + 10) * 4;
    pixels[at..at + 3].copy_from_slice(&rgb);
    ImageSource::Rgba {
        width: 40,
        height: 40,
        pixels: pixels.into(),
    }
}

/// An `side`-unit black document with one unit at (10, 10) in `fill`, padded
/// so that unit sits off the sampled bytes, which the comments fill instead.
#[cfg(feature = "svg")]
fn one_pixel_svg(fill: &str, side: u32) -> ImageSource {
    let (before, after) = ("a".repeat(300), "z".repeat(1500));
    ImageSource::SvgBytes(
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}"><!--{before}--><rect width="{side}" height="{side}" fill="#000"/><rect x="10" y="10" width="1" height="1" fill="{fill}"/><!--{after}--></svg>"##
        )
        .into_bytes()
        .into(),
    )
}

/// A surface showing one image `side` across, whose source is `source`.
fn swappable(app: &mut Headless, source: Signal<ImageSource>, side: u32) -> SurfaceId {
    let surface = app.surface(
        SurfaceConfig::new()
            .height(side)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || {
            container()
                .width(side as f32)
                .height(side as f32)
                .child(image(source).content_fit(ContentFit::Fill))
        },
    );
    app.configure(surface, side, side, 1.0);
    surface
}

fn bytes(source: &ImageSource) -> &[u8] {
    match source {
        ImageSource::Rgba { pixels, .. } => pixels,
        ImageSource::SvgBytes(bytes) | ImageSource::Bytes(bytes) => bytes,
        _ => unreachable!("only in-memory sources here"),
    }
}

/// Two images side by side, `red` and then `green`, each 40 across: each is
/// drawn as itself, and from a texture of its own.
fn two_sources_are_two_images(red: ImageSource, green: ImageSource) {
    assert_differ_off_the_samples(bytes(&red), bytes(&green));
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (surface, _) = surface(&mut app, vec![red, green], 40);

    app.step();

    let (left, right) = (
        app.read_pixel(surface, 10, 10),
        app.read_pixel(surface, 50, 10),
    );
    assert!(
        is_red(left),
        "the first image draws its own red, got {left:?}"
    );
    assert!(
        is_green(right),
        "the second image draws its own green, got {right:?}"
    );
    assert_eq!(app.image_textures(), 2, "one texture each");
}

/// Two raw buffers equal in every byte the cache hashed are still two images
/// (#606).
///
/// Red before: the texture cache keyed a source by a hash of its start, middle
/// and end, so the second image was drawn from the first's texture.
#[test]
fn raw_pixels_alike_where_sampled_are_two_images() {
    two_sources_are_two_images(one_pixel_rgba([255, 0, 0]), one_pixel_rgba([0, 255, 0]));
}

/// The same of two documents, rasterized inside the frame (#606).
#[cfg(feature = "svg")]
#[test]
fn svgs_alike_where_sampled_are_two_images() {
    two_sources_are_two_images(one_pixel_svg("#f00", 40), one_pixel_svg("#0f0", 40));
}

/// Equal pixels in two allocations are still one texture: identity is the
/// contents, not the buffer.
#[test]
fn raw_pixels_equal_in_two_buffers_share_a_texture() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (first, second) = (one_pixel_rgba([255, 0, 0]), one_pixel_rgba([255, 0, 0]));
    assert!(!std::ptr::eq(bytes(&first), bytes(&second)));
    let (surface, _) = surface(&mut app, vec![first, second], 40);

    app.step();

    for x in [10, 50] {
        assert!(is_red(app.read_pixel(surface, x, 10)), "drawn at x = {x}");
    }
    assert_eq!(app.image_textures(), 1, "one texture for both");
}

/// An image whose source is replaced by one alike where sampled draws the new
/// one (#606).
#[test]
fn a_replaced_source_alike_where_sampled_is_drawn_as_itself() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let source = create_signal(one_pixel_rgba([255, 0, 0]));
    let surface = swappable(&mut app, source.into(), 40);
    app.step();
    assert!(is_red(app.read_pixel(surface, 10, 10)));

    source.set(one_pixel_rgba([0, 255, 0]));
    app.step();

    let pixel = app.read_pixel(surface, 10, 10);
    assert!(is_green(pixel), "the new source is drawn, got {pixel:?}");
}

/// A new document whose raster is on the worker draws nothing rather than
/// another document's raster, however alike the two are where sampled (#606).
///
/// Red before: the new document's texture key was the old one's, so the old
/// raster was found and drawn as if it were the new one's.
#[cfg(feature = "svg")]
#[test]
fn a_new_svg_does_not_borrow_another_sources_raster() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (red, green) = (one_pixel_svg("#f00", LARGE), one_pixel_svg("#0f0", LARGE));
    assert_differ_off_the_samples(bytes(&red), bytes(&green));
    let source = create_signal(red);
    let surface = swappable(&mut app, source.into(), LARGE);
    app.step();
    app.wait_for_image_decodes();
    app.step();
    assert!(
        is_red(app.read_pixel(surface, 10, 10)),
        "the first is drawn"
    );

    let hold = app.hold_image_decodes();
    source.set(green);
    app.step();
    let pixel = app.read_pixel(surface, 10, 10);
    assert!(
        !is_red(pixel),
        "the new document is not drawn from the old one's raster, got {pixel:?}"
    );

    hold.release();
    app.wait_for_image_decodes();
    app.step();
    let pixel = app.read_pixel(surface, 10, 10);
    assert!(
        is_green(pixel),
        "and is drawn once its own lands, got {pixel:?}"
    );
}

/// Raw pixels the application has let go are let go by the cache too, under
/// the budget as well as over it, by the frame after the one that replaced
/// them: that one still holds the old draw command in its paint cache.
///
/// The texture cache keys raw pixels by their buffer, so without this the key
/// alone would keep a replaced buffer alive until its texture was evicted.
#[test]
fn replaced_raw_pixels_are_not_kept_by_the_cache() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let red = one_pixel_rgba([255, 0, 0]);
    let ImageSource::Rgba { pixels, .. } = &red else {
        unreachable!()
    };
    let replaced = Arc::downgrade(pixels);
    let source = create_signal(red);
    let surface = swappable(&mut app, source.into(), 40);
    app.step();

    source.set(one_pixel_rgba([0, 255, 0]));
    app.step();
    source.set(one_pixel_rgba([255, 255, 255]));
    app.step();

    let pixel = app.read_pixel(surface, 10, 10);
    assert_eq!(pixel, [255, 255, 255, 255], "the last one is drawn");
    assert!(
        replaced.upgrade().is_none(),
        "nothing holds the first buffer"
    );
    assert_eq!(
        app.image_textures(),
        2,
        "nor its texture: the second's and the last's are left"
    );
}

/// Raw pixels handed over again in a new buffer, equal to the last, keep
/// their texture: the cache moves its key to the new buffer once nothing else
/// holds the old one, rather than sweeping a texture that is still drawn.
///
/// `set_always`, because `set` drops a value equal to the one it holds and
/// the image would never see the new buffer.
#[test]
fn equal_raw_pixels_in_a_new_buffer_keep_their_texture() {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let source = create_signal(one_pixel_rgba([255, 0, 0]));
    let surface = swappable(&mut app, source.into(), 40);
    app.step();

    for frame in 0..4 {
        source.set_always(one_pixel_rgba([255, 0, 0]));
        app.step();
        assert!(is_red(app.read_pixel(surface, 10, 10)), "frame {frame}");
        assert_eq!(
            app.image_textures(),
            1,
            "frame {frame}: the texture drawn is the one kept"
        );
    }
}
