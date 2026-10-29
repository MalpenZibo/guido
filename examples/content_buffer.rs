//! A bar on the bottom edge that opens a panel above itself, on a surface the
//! compositor stretches over the whole screen, with a buffer only as large as
//! what it draws (`BufferExtent::Content`).
//!
//! Click "Panel" to open it, and anywhere outside it to close it. Run with
//! `WAYLAND_DEBUG=1` to see the subsurface move and resize with the panel.

use guido::prelude::*;

const BAR: f32 = 40.0;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(BAR as u32)
                .exclusive_edge(Edge::Bottom)
                .keyboard_interactivity(KeyboardInteractivity::None)
                .background_color(Color::TRANSPARENT)
                .buffer(BufferExtent::Content),
            || {
                let open = create_signal(false);
                container()
                    .width(fill())
                    .height(fill())
                    .takes_input(false)
                    .layout(Flex::column().main_alignment(MainAlignment::End))
                    // Takes the click that closes the panel, anywhere above it,
                    // and draws nothing.
                    .child(
                        container()
                            .width(fill())
                            .height(fill())
                            .takes_input(move || open.get())
                            .on_click(move || open.set(false)),
                    )
                    .child(
                        container().width(fill()).padding([0.0, 16.0]).child(
                            container()
                                .width(fill())
                                .height(
                                    (move || if open.get() { 300.0 } else { 0.0 }).transition(
                                        Transition::new(200.0, TimingFunction::EaseOut),
                                    ),
                                )
                                .overflow(Overflow::Hidden)
                                .background(Color::rgb(0.18, 0.18, 0.26))
                                .corners(12.0)
                                .takes_input(move || open.get())
                                .child(container().padding(16.0).child(text("A panel"))),
                        ),
                    )
                    .child(
                        container()
                            .width(fill())
                            .height(BAR)
                            .background(Color::rgb(0.1, 0.1, 0.15))
                            .takes_input(true)
                            .layout(Flex::row().main_alignment(MainAlignment::SpaceBetween))
                            .padding(6.0)
                            .child(
                                container()
                                    .padding([4.0, 12.0])
                                    .background(Color::rgb(0.25, 0.25, 0.4))
                                    .corners(6.0)
                                    .on_click(move || open.update(|o| *o = !*o))
                                    .child(text("Panel")),
                            )
                            .child(container().padding([4.0, 12.0]).child(text("guido"))),
                    )
            },
        );
    });
}
