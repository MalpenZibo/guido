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

/// What one of the 20×20 PNGs above costs as a texture: width × height × 4.
const IMAGE_BYTES: usize = 20 * 20 * 4;

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
