//! The list itself: a scroller of interactive rows.
//!
//! In a file of its own because two binaries build it. The example beside this
//! puts it on a real surface and a person scrolls it; `benches/scroll_list`
//! plays a scripted gesture over it with no compositor. One definition, so the
//! benchmark's numbers stay comparable with the hand-run history rather than
//! describing a list that has quietly drifted from the one that was measured.

use guido::prelude::*;

/// The viewport the list is measured in, in logical pixels. Shared because it
/// is what decides how far the paint window narrows, and therefore every count
/// the benchmark reports: a benchmark looking at a different viewport from the
/// example is looking at a different workload.
pub const VIEWPORT: (u32, u32) = (600, 800);

/// The page behind the rows.
pub const BACKGROUND: Color = Color::rgb(0.10, 0.10, 0.14);

/// The whole scroller, at `row_count` rows.
pub fn list(row_count: usize) -> Container {
    container()
        .background(BACKGROUND)
        .scroll(Scroll::vertical())
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .padding(8.0)
                .children((0..row_count).map(row)),
        )
}

/// One row: a checkbox composed from a container — guido has no dedicated
/// checkbox widget yet — and a text input.
fn row(i: usize) -> Container {
    let value = create_signal(format!("Item {i}"));
    let checked = create_signal(i.is_multiple_of(5));

    container()
        .layout(Flex::row().spacing(8.0))
        .padding(4.0)
        .background(Color::rgb(0.14, 0.14, 0.19))
        .corners(6.0)
        .child(
            container()
                .width(18.0)
                .height(18.0)
                .corners(4.0)
                .border(1.5, Color::rgb(0.45, 0.45, 0.55))
                .background(move || {
                    if checked.get() {
                        Color::rgb(0.30, 0.55, 0.95)
                    } else {
                        Color::rgb(0.18, 0.18, 0.24)
                    }
                })
                .when_hovered(|s| s.lighter(0.08))
                .on_click(move || checked.update(|c| *c = !*c))
                .child(
                    text(move || (if checked.get() { "x" } else { "" }).to_string())
                        .font_size(12.0)
                        .color(Color::WHITE),
                ),
        )
        .child(
            container()
                .width(fill())
                .padding(6.0)
                .background(Color::rgb(0.18, 0.18, 0.24))
                .corners(4.0)
                .when_focused(|s| s.border(1.5, Color::rgb(0.4, 0.8, 1.0)))
                .child(text_input(value).font_size(13.0).color(Color::WHITE)),
        )
}
