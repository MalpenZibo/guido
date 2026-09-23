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
//! thread steps, and every `Headless` dropped retires the writes queued before
//! it — so beside tests that step and drop in parallel, a decode written by one
//! test's worker would be applied to another test's thread, or thrown away.
//! An application has one loop in its process; a test binary has as many as it
//! has threads.

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

fn png_file(name: &str, rgb: [u8; 3]) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, png(rgb)).expect("the PNG is written");
    path
}

/// A strip of 20×20 images side by side on the backdrop, starting at x = 0.
fn strip(sources: Vec<ImageSource>, ready: Rc<Cell<Option<Signal<bool>>>>) -> Container {
    container()
        .layout(Flex::row())
        .children(sources.into_iter().map(move |source| {
            let image = image(source).content_fit(ContentFit::Fill);
            ready.set(Some(image.ready()));
            container().width(20.0).height(20.0).child(image)
        }))
}

fn surface(app: &mut Headless, sources: Vec<ImageSource>) -> (SurfaceId, Signal<bool>) {
    let ready = Rc::new(Cell::new(None));
    let captured = ready.clone();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(BACKDROP),
        move || strip(sources, captured),
    );
    app.configure(surface, 60, 20, 1.0);
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
fn a_pending_image_is_drawn_on_a_later_frame(source: ImageSource) {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let hold = app.hold_image_decodes();
    let (surface, ready) = surface(&mut app, vec![source]);

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
    a_pending_image_is_drawn_on_a_later_frame(ImageSource::Path(png_file(
        "decoded-later.png",
        [255, 0, 0],
    )));
}

#[test]
fn image_bytes_are_drawn_on_the_frame_after_their_decode() {
    a_pending_image_is_drawn_on_a_later_frame(ImageSource::Bytes(png([255, 0, 0]).into()));
}

/// Two images of one source share one decode, and both are drawn by it.
fn two_images_of_one_source_decode_it_once(first: ImageSource, second: ImageSource) {
    let _serial = serial();
    let Some(mut app) = headless() else { return };
    let (surface, _) = surface(&mut app, vec![first, second]);

    app.step();
    app.wait_for_image_decodes();
    app.step();

    assert_eq!(app.image_decodes_started(), 1, "one decode for both");
    for x in [10, 30] {
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
    );
}

/// Two buffers, not one `Arc` handed twice: equal bytes are one source
/// whoever allocated them.
#[test]
fn two_images_of_equal_bytes_decode_them_once() {
    let first: Arc<[u8]> = png([255, 0, 0]).into();
    let second: Arc<[u8]> = png([255, 0, 0]).into();
    assert!(!Arc::ptr_eq(&first, &second));
    two_images_of_one_source_decode_it_once(ImageSource::Bytes(first), ImageSource::Bytes(second));
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
    let (surface, ready) = surface(&mut app, vec![ImageSource::Path(path)]);

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

/// More images in view than the renderer's texture cache holds stay drawn,
/// and are decoded once each.
///
/// Red before: eviction took the least recently used textures whether or not
/// the frame before had drawn them. While the pixels stayed in the cache that
/// cost an upload; once an upload drops them, every evicted image in view went
/// blank and back to the worker, frame after frame, for as long as anything
/// asked for frames.
#[test]
fn more_images_in_view_than_the_texture_cache_holds_stay_drawn() {
    const IMAGES: u8 = 70;
    let _serial = serial();
    let Some(mut app) = headless() else { return };
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
                .layout(Flex::row())
                .children((0..IMAGES).map(|i| {
                    let source = ImageSource::Bytes(png([200, i, 0]).into());
                    container()
                        .width(4.0)
                        .height(4.0)
                        .child(image(source).content_fit(ContentFit::Fill))
                }))
        },
    );
    app.configure(surface, u32::from(IMAGES) * 4, 4, 1.0);
    app.step();
    app.wait_for_image_decodes();
    app.step();

    for frame in 0..4 {
        backdrop.set(Color::rgb(0.0, 0.0, if frame % 2 == 0 { 0.9 } else { 1.0 }));
        app.step();
        app.wait_for_image_decodes();
        app.step();
        let blank: Vec<u32> = (0..u32::from(IMAGES))
            .filter(|i| app.read_pixel(surface, i * 4 + 2, 2)[2] > 150)
            .collect();
        assert!(
            blank.is_empty(),
            "frame {frame}: images {blank:?} went blank"
        );
    }
    assert_eq!(
        app.image_decodes_started(),
        u64::from(IMAGES),
        "each decoded once"
    );
}
