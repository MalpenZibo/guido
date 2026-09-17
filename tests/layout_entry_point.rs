//! What happens *around* a child's layout, rather than inside it.
//!
//! A parent used to lay a child out by calling the child's `layout` itself, so
//! everything that should happen around every layout was left to each widget:
//! the skip check, the tracking scope, the pass it is in, the cache write.
//! `Container` and `Text` wrote their own skip check; `TextInput` and `Image`
//! never had one and ran again every time their parent did.
//!
//! These ask the two widgets that had no check of their own, through the
//! public API a caller has: a parent marked dirty lays out again, its children
//! are handed the constraints they already had, and nothing below it runs.
//!
//! The sentinel is the relayout boundary flag, which both of those widgets set
//! at the top of their own `layout`. Set it to the value they never leave
//! behind, and it survives exactly as long as they do not run.

use guido::prelude::*;

mod common;
use common::Harness;

/// The leaf under the root, which is the widget being asked about.
fn leaf(h: &Harness) -> guido::tree::WidgetId {
    h.tree.get_children(h.root)[0]
}

/// A dirty parent lays out again. Its children are handed the same constraints
/// and are not dirty, so nothing below it has anything to do — and a widget
/// that never wrote a skip check of its own should not have to.
#[test]
fn a_text_input_under_a_dirty_parent_is_not_laid_out_again() {
    let value = create_signal(String::from("hello"));
    let mut h = Harness::laid_out(
        container().width(200.0).child(text_input(value)),
        400.0,
        400.0,
    );
    let field = leaf(&h);

    // The value `TextInput::layout` overwrites on its first line, set to the
    // one it never leaves behind.
    h.tree.set_relayout_boundary(field, true);
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 400.0);

    assert!(
        h.tree.is_relayout_boundary(field),
        "the field was laid out again under constraints it already had, with \
         nothing of its own dirty"
    );
}

/// The same for an image, the other widget that had no check of its own.
#[test]
fn an_image_under_a_dirty_parent_is_not_laid_out_again() {
    let mut h = Harness::laid_out(
        container().width(200.0).child(image(ImageSource::Rgba {
            pixels: vec![0u8; 20 * 10 * 4].into(),
            width: 20,
            height: 10,
        })),
        400.0,
        400.0,
    );
    let picture = leaf(&h);

    h.tree.set_relayout_boundary(picture, true);
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 400.0);

    assert!(
        h.tree.is_relayout_boundary(picture),
        "the image was laid out again under constraints it already had, with \
         nothing of its own dirty"
    );
}
