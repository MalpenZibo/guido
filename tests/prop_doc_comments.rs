//! A prop's `///` documents the builder method a caller sets it with.
//!
//! Rust refuses a doc comment on a function parameter, but only after macro
//! expansion, so `#[component]` sees them first. It used to drop them with the
//! rest of the function, and the methods a caller reads had no documentation:
//! the module below compiled, and denying `missing_docs` over it failed on
//! every setter, on the struct and on the constructor. The macro writes no doc
//! of its own on a prop's setter, so the only one that can be there is the
//! one the parameter carried.

use guido::prelude::*;

mod common;
use common::Harness;

/// Public and denied `missing_docs`, so every item the macro emits needs a doc.
#[deny(missing_docs)]
pub mod documented {
    use guido::prelude::*;

    /// A titled box with a badge, and whatever its caller puts inside.
    #[component]
    pub fn panel(
        /// The text across the top.
        title: String,
        /// How far the contents sit from the edge.
        #[prop(default = 4.0)]
        inset: f32,
        /// Called when the panel is clicked.
        #[prop(callback)]
        on_open: (),
        /// What sits beside the title.
        #[prop(slot)]
        badge: (),
        /// The rows under the title.
        #[prop(children)]
        children: (),
    ) -> impl Widget {
        container()
            .padding(inset)
            .on_click(on_open)
            .layout(Flex::column())
            .child(text(title))
            .children(badge)
            .children(children)
    }
}

#[test]
fn a_documented_prop_is_still_a_prop() {
    let mut h = Harness::laid_out(
        documented::panel()
            .title("Settings")
            .inset(8.0)
            .on_open(|| {})
            .badge(
                container()
                    .width(10.0)
                    .height(10.0)
                    .background(Color::WHITE),
            )
            .child(
                container()
                    .width(20.0)
                    .height(10.0)
                    .background(Color::WHITE),
            ),
        200.0,
        200.0,
    );
    let widths: Vec<f32> = h.painted_rects().iter().map(|r| r.width).collect();
    assert_eq!(widths, vec![10.0, 20.0]);
}
