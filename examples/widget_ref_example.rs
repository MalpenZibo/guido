//! Example demonstrating the WidgetRef API for reactive widget bounds.
//!
//! Attaches a WidgetRef to a container and displays its surface-relative
//! position and size in real time via a reactive text widget.

use guido::prelude::*;

fn main() {
    let module_ref = create_widget_ref();

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(64)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .layout(
                    Flex::row()
                        .spacing(16.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .child(
                    // Spacer
                    container().width(100.0),
                )
                .child(
                    // The measured module
                    container()
                        .widget_ref(module_ref)
                        .padding_xy(16.0, 8.0)
                        .background(Color::rgb(0.25, 0.25, 0.35))
                        .corner_radius(6.0)
                        .child(text("Measured Module").color(Color::WHITE)),
                )
                .child(
                    // Display the bounds reactively
                    container()
                        .padding_xy(16.0, 8.0)
                        .background(Color::rgb(0.15, 0.2, 0.15))
                        .corner_radius(6.0)
                        .child(
                            text(move || {
                                let r = module_ref.rect().get();
                                format!(
                                    "Bounds: x={:.0} y={:.0} w={:.0} h={:.0}",
                                    r.x, r.y, r.width, r.height
                                )
                            })
                            .color(Color::WHITE),
                        ),
                )
        },
    );
    app.run();
}
