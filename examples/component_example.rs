use guido::prelude::*;

/// A reusable Button component
#[component]
pub struct Button {
    #[prop]
    label: String,
    #[prop(default = "Color::rgb(0.3, 0.3, 0.4)")]
    background: Color,
    #[prop(default = "8.0")]
    padding: f32,
    #[prop(callback)]
    on_click: (),
}

impl Button {
    fn render(&self) -> impl Widget + use<> {
        container()
            .padding(self.padding.get())
            .background(self.background.clone())
            .corner_radius(6.0)
            .hover_state(|s| s.lighter(0.1))
            .pressed_state(|s| s.ripple())
            .on_click_option(self.on_click.clone())
            .child(text(self.label.clone()).color(Color::WHITE))
    }
}

/// A reusable Card component with children
#[component]
pub struct Card {
    #[prop]
    title: String,
    #[prop(default = "Color::rgb(0.18, 0.18, 0.22)")]
    background: Color,
    #[prop(children)]
    children: (),
}

impl Card {
    fn render(&self) -> impl Widget + use<> {
        container()
            .padding(16.0)
            .background(self.background.get())
            .corner_radius(8.0)
            .layout(Flex::column().spacing(8.0))
            .child(text(self.title.clone()).font_size(18.0).color(Color::WHITE))
            .children_source(self.take_children())
    }
}

fn main() {
    let _ = env_logger::try_init();

    App::new().run(|app| {
        // Create some reactive state
        // No need to clone signals anymore - they implement Copy!
        let count = create_signal(0);

        app.add_surface(
            SurfaceConfig::new()
                .width(600)
                .height(500)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Overlay)
                .namespace("component-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .padding(16.0)
                    .background(Color::rgb(0.1, 0.1, 0.15))
                    .layout(Flex::column().spacing(12.0))
                    .child(
                        text("Component Example")
                            .font_size(24.0)
                            .color(Color::WHITE),
                    )
                    .child(
                        card()
                            .title("Counter")
                            .background(Color::rgb(0.15, 0.2, 0.25))
                            .child(
                                text(move || format!("Count: {}", count.get()))
                                    .font_size(16.0)
                                    .color(Color::WHITE),
                            )
                            .child(
                                container()
                                    .layout(Flex::row().spacing(8.0))
                                    .child(
                                        button()
                                            .label("Increment")
                                            .background(Color::rgb(0.2, 0.6, 0.3))
                                            .on_click(move || count.update(|c| *c += 1)),
                                    )
                                    .child(
                                        button()
                                            .label("Decrement")
                                            .background(Color::rgb(0.6, 0.2, 0.2))
                                            .on_click(move || count.update(|c| *c -= 1)),
                                    )
                                    .child(
                                        button()
                                            .label("Reset")
                                            .background(Color::rgb(0.4, 0.4, 0.5))
                                            .on_click(move || count.set(0)),
                                    ),
                            ),
                    )
                    .child(
                        card()
                            .title("Static Content")
                            .child(
                                text("This is a card with static content.")
                                    .color(Color::rgb(0.8, 0.8, 0.8)),
                            )
                            .child(
                                text("Cards can have multiple children.")
                                    .color(Color::rgb(0.8, 0.8, 0.8)),
                            ),
                    )
                    .child(
                        card().title("Styled Buttons").child(
                            container()
                                .layout(Flex::row().spacing(8.0))
                                .child(
                                    button()
                                        .label("Primary")
                                        .background(Color::rgb(0.2, 0.4, 0.8))
                                        .padding(12.0)
                                        .on_click(|| println!("Primary clicked")),
                                )
                                .child(
                                    button()
                                        .label("Secondary")
                                        .background(Color::rgb(0.5, 0.5, 0.6))
                                        .padding(12.0)
                                        .on_click(|| println!("Secondary clicked")),
                                )
                                .child(
                                    button()
                                        .label("Danger")
                                        .background(Color::rgb(0.8, 0.2, 0.2))
                                        .padding(12.0)
                                        .on_click(|| println!("Danger clicked")),
                                ),
                        ),
                    )
                    .child(
                        card()
                            .title("Reactive Props Example")
                            .background(move || {
                                // Card background changes based on count
                                if count.get() > 5 {
                                    Color::rgb(0.2, 0.3, 0.2)
                                } else if count.get() < -5 {
                                    Color::rgb(0.3, 0.2, 0.2)
                                } else {
                                    Color::rgb(0.18, 0.18, 0.22)
                                }
                            })
                            .child(
                                text("The card background changes color based on the count value.")
                                    .color(Color::rgb(0.8, 0.8, 0.8)),
                            )
                            .child(
                                container().layout(Flex::row().spacing(8.0)).child(
                                    button()
                                        .label(move || format!("Count is {}", count.get()))
                                        .background(move || {
                                            // Button color changes based on count
                                            let c = count.get();
                                            if c % 2 == 0 {
                                                Color::rgb(0.3, 0.5, 0.7)
                                            } else {
                                                Color::rgb(0.7, 0.5, 0.3)
                                            }
                                        })
                                        .on_click(move || count.update(|c| *c += 1)),
                                ),
                            ),
                    )
            },
        );
    });
}
