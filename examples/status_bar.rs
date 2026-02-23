use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(32)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(8.0)
                            .main_axis_alignment(MainAxisAlignment::SpaceBetween),
                    )
                    .child(
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.2, 0.2, 0.3))
                            .corner_radius(4.0)
                            .child(text("Guido")),
                    )
                    .child(container().padding(8.0).child(text("Hello World!")))
                    .child(
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.3, 0.2, 0.2))
                            .corner_radius(4.0)
                            .child(text("Status Bar")),
                    )
            },
        );
    });
}
