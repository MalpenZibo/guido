//! Example demonstrating nested transforms - how parent and child transforms combine.
//!
//! This shows that shape transforms work correctly when containers are nested.
//! Note: Text inside transformed containers is not currently transformed.

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(200)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        || {
            container()
                .layout(
                    Flex::row()
                        .spacing(40.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .padding(20.0)
                .children([
                    // Case 0: Parent, child
                    // Expected: Inner box centered
                    container()
                        .width(100.0)
                        .height(100.0)
                        .background(Color::rgba(0.8, 0.3, 0.3, 0.5))
                        .corner_radius(8.0)
                        .child(
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.3, 0.8, 0.3))
                                .corner_radius(4.0),
                        ),
                    // Case 1: Parent rotated 30, child scaled 0.7
                    // Expected: Inner box rotated 30deg AND smaller
                    container()
                        .width(100.0)
                        .height(100.0)
                        .background(Color::rgba(0.8, 0.3, 0.3, 0.5))
                        .corner_radius(8.0)
                        .rotate(30.0)
                        .child(
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.3, 0.8, 0.3))
                                .corner_radius(4.0)
                                .scale(0.7),
                        ),
                    // Case 2: Both parent and child rotated 20deg each
                    // Expected: Inner box rotated 40deg total
                    container()
                        .width(100.0)
                        .height(100.0)
                        .background(Color::rgba(0.3, 0.3, 0.8, 0.5))
                        .corner_radius(8.0)
                        .rotate(20.0)
                        .child(
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.8, 0.8, 0.3))
                                .corner_radius(4.0)
                                .rotate(20.0),
                        ),
                    // Case 3: Parent scaled 1.3, child rotated 45deg
                    // Expected: Inner box rotated AND the whole group larger
                    container()
                        .width(100.0)
                        .height(100.0)
                        .background(Color::rgba(0.8, 0.5, 0.2, 0.5))
                        .corner_radius(8.0)
                        .scale(1.3)
                        .child(
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.5, 0.2, 0.8))
                                .corner_radius(4.0)
                                .rotate(45.0),
                        ),
                    // Case 4: Child with NO transform (inherits parent's rotation)
                    // Expected: Inner box rotated same as parent (30deg)
                    container()
                        .width(100.0)
                        .height(100.0)
                        .background(Color::rgba(0.2, 0.7, 0.7, 0.5))
                        .corner_radius(8.0)
                        .rotate(30.0)
                        .child(
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.7, 0.2, 0.7))
                                .corner_radius(4.0),
                        ),
                ])
        },
    );
    app.run();
}
