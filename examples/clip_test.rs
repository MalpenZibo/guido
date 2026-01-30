//! Test: Clip region handling with transforms
//!
//! This test verifies that clipping works correctly with transformed shapes.
//! Run with: cargo run --example clip_test --features renderer_v2
//!
//! Row 1: Simple clip (no transforms) - parent clips child
//! Row 2: Clip on rotated shape - rotated container with clipping children
//! Row 3: Clip on scaled shape - scaled container with clipping children
//! Row 4: Nested transforms with clip - 3-level nesting with intermediate clip

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(480)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.12, 0.12, 0.16)),
        move || {
            container()
                .layout(Flex::column().spacing(20.0))
                .padding(20.0)
                .children([
                    // Row 1: Simple clip (no transforms)
                    simple_clip_row(),
                    // Row 2: Clip on rotated shape
                    rotated_clip_row(),
                    // Row 3: Clip on scaled shape
                    scaled_clip_row(),
                    // Row 4: Nested transforms with clip
                    nested_clip_row(),
                ])
        },
    );
    app.run();
}

/// Row 1: Simple clip - parent clips child that extends beyond bounds
fn simple_clip_row() -> Container {
    container().layout(Flex::row().spacing(40.0)).children([
        // Without clip - child extends beyond parent (reference)
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .child(
                container()
                    .width(60.0)
                    .height(60.0)
                    .background(Color::rgb(0.8, 0.3, 0.3))
                    .corner_radius(4.0),
            ),
        // With clip - child is clipped to parent bounds
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .child(
                container()
                    .width(120.0)
                    .height(120.0)
                    .background(Color::rgb(0.3, 0.8, 0.3))
                    .corner_radius(4.0),
            ),
        // Rounded clip with larger child
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(20.0)
            .overflow(Overflow::Hidden)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.3, 0.3, 0.8)),
            ),
    ])
}

/// Row 2: Clip on rotated container
fn rotated_clip_row() -> Container {
    container().layout(Flex::row().spacing(40.0)).children([
        // Rotated 15° with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .rotate(15.0)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.8, 0.5, 0.2)),
            ),
        // Rotated 30° with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .rotate(30.0)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.5, 0.8, 0.2)),
            ),
        // Rotated 45° with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .rotate(45.0)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.2, 0.5, 0.8)),
            ),
        // Rotated -30° with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .rotate(-30.0)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.8, 0.2, 0.5)),
            ),
    ])
}

/// Row 3: Clip on scaled container
fn scaled_clip_row() -> Container {
    container().layout(Flex::row().spacing(40.0)).children([
        // Scale 0.8x with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .scale(0.8)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.8, 0.8, 0.2)),
            ),
        // Scale 1.2x with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .scale(1.2)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.2, 0.8, 0.8)),
            ),
        // Non-uniform scale with clip
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(8.0)
            .overflow(Overflow::Hidden)
            .scale_xy(1.5, 0.7)
            .child(
                container()
                    .width(100.0)
                    .height(100.0)
                    .background(Color::rgb(0.8, 0.2, 0.8)),
            ),
    ])
}

/// Row 4: Nested transforms with clip at intermediate level
fn nested_clip_row() -> Container {
    container().layout(Flex::row().spacing(40.0)).children([
        // Level 1: rotate 15°, Level 2: clip, Level 3: child
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corner_radius(4.0)
            .rotate(15.0)
            .child(
                // Middle container with clip
                container()
                    .width(60.0)
                    .height(60.0)
                    .background(Color::rgb(0.2, 0.2, 0.3))
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    .child(
                        // Inner child that should be clipped
                        container()
                            .width(80.0)
                            .height(80.0)
                            .background(Color::rgb(0.9, 0.4, 0.1)),
                    ),
            ),
        // Level 1: scale, Level 2: rotate + clip, Level 3: child
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corner_radius(4.0)
            .scale(0.9)
            .child(
                container()
                    .width(60.0)
                    .height(60.0)
                    .background(Color::rgb(0.2, 0.2, 0.3))
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    .rotate(20.0)
                    .child(
                        container()
                            .width(80.0)
                            .height(80.0)
                            .background(Color::rgb(0.1, 0.7, 0.4)),
                    ),
            ),
        // Level 1: rotate, Level 2: scale + clip, Level 3: rotate
        container()
            .width(80.0)
            .height(80.0)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corner_radius(4.0)
            .rotate(-10.0)
            .child(
                container()
                    .width(60.0)
                    .height(60.0)
                    .background(Color::rgb(0.2, 0.2, 0.3))
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    .scale(1.1)
                    .child(
                        container()
                            .width(50.0)
                            .height(50.0)
                            .background(Color::rgb(0.4, 0.2, 0.9))
                            .rotate(30.0),
                    ),
            ),
    ])
}
