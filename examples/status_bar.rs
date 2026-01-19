use guido::prelude::*;

fn main() {
    let view = row![
        container()
            .padding(8.0)
            .background(Color::rgb(0.2, 0.2, 0.3))
            .corner_radius(4.0)
            .child(text("Guido")),
        container().padding(8.0).child(text("Hello World!")),
        container()
            .padding(8.0)
            .background(Color::rgb(0.3, 0.2, 0.2))
            .corner_radius(4.0)
            .child(text("Status Bar")),
    ]
    .spacing(8.0)
    .main_axis_alignment(MainAxisAlignment::SpaceBetween);

    App::new()
        .height(32)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
