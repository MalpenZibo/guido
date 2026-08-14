//! How an image sizes itself inside the box a container gives it.
//!
//! These are layout-only: they assert the widget's size, never its pixels, so
//! they need no GPU and no fonts. The source is raw RGBA, so the intrinsic
//! size is known without touching the filesystem or a decoder.

use guido::layout::{Constraints, Size};
use guido::prelude::*;
use guido::tree::Tree;
use guido::widgets::Widget;

/// A 200x100 image — 2:1, so an aspect mistake is never a rounding artefact.
fn source() -> ImageSource {
    ImageSource::Rgba {
        pixels: vec![0u8; 200 * 100 * 4].into(),
        width: 200,
        height: 100,
    }
}

fn size_of(widget: impl Widget + 'static, constraints: Constraints) -> Size {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| w.layout(t, id, constraints))
        .expect("root is registered")
}

/// The size the image itself takes inside a box of exactly `w` x `h`.
fn in_box(fit: ContentFit, w: f32, h: f32) -> Size {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(
        container()
            .width(w)
            .height(h)
            .child(image(source()).content_fit(fit)),
    ));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 1000.0, 1000.0))
    });
    let child = tree.get_children(root)[0];
    tree.cached_size(child).expect("the image was laid out")
}

// ---------------------------------------------------------------------------
// Cover
// ---------------------------------------------------------------------------

#[test]
fn cover_fills_a_box_that_is_the_wrong_shape() {
    // The regression this PR exists for: a 2:1 image asked to cover a 16:10
    // box used to shrink itself to 2:1 and letterbox. Covering means taking
    // the whole box; the cropping happens in the pixels, not in the layout.
    assert_eq!(
        in_box(ContentFit::Cover, 1600.0, 1000.0),
        Size::new(1600.0, 1000.0)
    );
}

#[test]
fn cover_fills_a_taller_box_too() {
    assert_eq!(
        in_box(ContentFit::Cover, 400.0, 800.0),
        Size::new(400.0, 800.0)
    );
}

#[test]
fn cover_takes_the_room_it_is_offered_even_when_loose() {
    // A stack child gets loose constraints; "cover" still means "all of it".
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::Cover),
            Constraints::new(0.0, 0.0, 1600.0, 1000.0)
        ),
        Size::new(1600.0, 1000.0)
    );
}

// ---------------------------------------------------------------------------
// Contain
// ---------------------------------------------------------------------------

#[test]
fn contain_fits_inside_the_offered_room() {
    // 2:1 inside 1600x1000: width is the limit.
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::Contain),
            Constraints::new(0.0, 0.0, 1600.0, 1000.0)
        ),
        Size::new(1600.0, 800.0)
    );
}

#[test]
fn contain_fits_inside_when_height_is_the_limit() {
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::Contain),
            Constraints::new(0.0, 0.0, 1000.0, 100.0)
        ),
        Size::new(200.0, 100.0)
    );
}

#[test]
fn contain_shrinks_rather_than_painting_letterbox_bars() {
    // A fixed-size container still offers its child loose constraints, so
    // "contain" reports the 2:1 rect that fits and lets the parent's alignment
    // place it. Nothing is drawn in the empty strip, which is the difference
    // between letterboxing and just being smaller.
    assert_eq!(
        in_box(ContentFit::Contain, 300.0, 300.0),
        Size::new(300.0, 150.0)
    );
}

#[test]
fn contain_takes_a_tight_box_whole() {
    // Told exactly how big to be — a stack child that fills, say — it does not
    // argue, and the letterboxing is drawn inside those bounds instead.
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::Contain),
            Constraints::tight(Size::new(300.0, 300.0))
        ),
        Size::new(300.0, 300.0)
    );
}

#[test]
fn contain_derives_the_loose_axis_from_the_tight_one() {
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::Contain),
            Constraints::new(300.0, 0.0, 300.0, 1000.0)
        ),
        Size::new(300.0, 150.0)
    );
}

// ---------------------------------------------------------------------------
// Fill and None
// ---------------------------------------------------------------------------

#[test]
fn fill_takes_the_whole_box() {
    assert_eq!(
        in_box(ContentFit::Fill, 640.0, 480.0),
        Size::new(640.0, 480.0)
    );
}

#[test]
fn none_uses_the_intrinsic_pixels() {
    assert_eq!(
        size_of(
            image(source()).content_fit(ContentFit::None),
            Constraints::new(0.0, 0.0, 1600.0, 1000.0)
        ),
        Size::new(200.0, 100.0)
    );
}

#[test]
fn none_is_still_clamped_by_a_box_too_small_for_it() {
    assert_eq!(
        in_box(ContentFit::None, 50.0, 50.0),
        Size::new(50.0, 50.0),
        "a widget may not report a size larger than its constraints allow"
    );
}

// ---------------------------------------------------------------------------
// Unbounded
// ---------------------------------------------------------------------------

#[test]
fn unbounded_constraints_fall_back_to_the_intrinsic_size() {
    // Nothing is on offer, so there is nothing to fill or cover. Without this
    // the size would be infinite.
    for fit in [
        ContentFit::Cover,
        ContentFit::Fill,
        ContentFit::Contain,
        ContentFit::None,
    ] {
        assert_eq!(
            size_of(image(source()).content_fit(fit), Constraints::unbounded()),
            Size::new(200.0, 100.0),
            "{fit:?} under unbounded constraints"
        );
    }
}

// ---------------------------------------------------------------------------
// The container decides the box
// ---------------------------------------------------------------------------

#[test]
fn a_filling_container_hands_the_image_the_whole_surface() {
    // What a wallpaper does. Before, this needed the output size threaded into
    // the image by hand because it could not be taken from the parent.
    let mut tree = Tree::new();
    let root = tree.register(Box::new(
        container()
            .width(fill())
            .height(fill())
            .layout(ZStack::new())
            .child(image(source()).content_fit(ContentFit::Cover)),
    ));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(t, id, Constraints::new(0.0, 0.0, 2560.0, 1440.0))
    });

    let child = tree.get_children(root)[0];
    assert_eq!(
        tree.cached_size(child),
        Some(Size::new(2560.0, 1440.0)),
        "the image should cover the whole output"
    );
}
