//! Test TextInput widget with transforms
//!
//! Tests that text input works correctly when inside transformed containers:
//! - Click-to-position cursor works with rotation, scale, and translation
//! - Selection works correctly
//! - Visual rendering is correct

use guido::prelude::*;

fn main() {
    let input1 = create_signal(String::from("Normal input"));
    let input2 = create_signal(String::from("Rotated 15°"));
    let input3 = create_signal(String::from("Scaled 1.2x"));
    let input4 = create_signal(String::from("Translated"));
    let input5 = create_signal(String::from("All transforms"));

    let view = container()
        .background(Color::rgb(0.1, 0.1, 0.15))
        .padding(32.0)
        .layout(Flex::column().spacing(32.0))
        .child(
            text("TextInput Transform Test")
                .color(Color::WHITE)
                .font_size(18.0),
        )
        // 1. Normal (no transform) - baseline
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("No transform (baseline)")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.3, 0.8, 0.3))
                        .corner_radius(6.0)
                        .child(
                            text_input(input1)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .font_size(14.0),
                        ),
                ),
        )
        // 2. Rotated container
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Rotated 15°")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.8, 0.5, 0.3))
                        .corner_radius(6.0)
                        .rotate(15.0)
                        .child(
                            text_input(input2)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .font_size(14.0),
                        ),
                ),
        )
        // 3. Scaled container
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Scaled 1.2x")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(250.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.3, 0.5, 0.8))
                        .corner_radius(6.0)
                        .scale(1.2)
                        .child(
                            text_input(input3)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .font_size(14.0),
                        ),
                ),
        )
        // 4. Translated container
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Translated (50, 10)")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.8, 0.8, 0.3))
                        .corner_radius(6.0)
                        .translate(50.0, 10.0)
                        .child(
                            text_input(input4)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .font_size(14.0),
                        ),
                ),
        )
        // 5. All transforms combined: translate + rotate + scale
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Translate(20,5) + Rotate(-10°) + Scale(0.9)")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.8, 0.3, 0.8))
                        .corner_radius(6.0)
                        .transform(
                            Transform::translate(20.0, 5.0)
                                .then(&Transform::rotate_degrees(-10.0))
                                .then(&Transform::scale(0.9)),
                        )
                        .child(
                            text_input(input5)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .font_size(14.0),
                        ),
                ),
        )
        // Instructions
        .child(
            text("Click in each input to test cursor positioning")
                .color(Color::rgb(0.5, 0.5, 0.6))
                .font_size(11.0),
        );

    App::new()
        .width(550)
        .height(680)
        .anchor(Anchor::TOP | Anchor::LEFT)
        .layer(Layer::Top)
        .namespace("text-input-transform-test")
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
