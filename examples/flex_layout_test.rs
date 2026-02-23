//! Flex Layout Test Example
//!
//! This example demonstrates and tests all flex layout alignment options:
//! - MainAlignment: Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly
//! - CrossAlignment: Start, Center, End, Stretch
//!
//! Run with: cargo run --example flex_layout_test

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(1000)
                .height(400)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.08, 0.08, 0.1)),
            || {
                container()
                    .layout(Flex::column().spacing(8.0))
                    .padding(8.0)
                    // Row tests side by side
                    .child(
                        container()
                            .layout(Flex::row().spacing(16.0))
                            .child(
                                container()
                                    .layout(Flex::column().spacing(3.0))
                                    .child(section_title("Row - MainAlignment"))
                                    .child(row_main_axis_tests()),
                            )
                            .child(
                                container()
                                    .layout(Flex::column().spacing(3.0))
                                    .child(section_title("Row - CrossAlignment"))
                                    .child(row_cross_axis_tests()),
                            )
                            .child(
                                container()
                                    .layout(Flex::column().spacing(3.0))
                                    .child(section_title("Center Test"))
                                    .child(center_test()),
                            ),
                    )
                    // Column tests side by side
                    .child(
                        container()
                            .layout(Flex::row().spacing(16.0))
                            .child(
                                container()
                                    .layout(Flex::column().spacing(3.0))
                                    .child(section_title("Column - MainAlignment"))
                                    .child(column_main_axis_tests()),
                            )
                            .child(
                                container()
                                    .layout(Flex::column().spacing(3.0))
                                    .child(section_title("Column - CrossAlignment"))
                                    .child(column_cross_axis_tests()),
                            ),
                    )
            },
        );
    });
}

fn section_title(title: &'static str) -> impl Widget {
    text(title).color(Color::rgb(0.7, 0.7, 0.8)).font_size(11.0)
}

fn label(s: &'static str) -> impl Widget {
    text(s).color(Color::rgb(0.6, 0.6, 0.7)).font_size(9.0)
}

fn test_box(color: Color) -> Container {
    container()
        .width(24.0)
        .height(16.0)
        .background(color)
        .corner_radius(2.0)
}

fn test_box_varied(width: f32, height: f32, color: Color) -> Container {
    container()
        .width(width)
        .height(height)
        .background(color)
        .corner_radius(2.0)
}

/// Test box with minimum height (for row cross-axis tests)
/// Height can stretch beyond min_height when CrossAlignment::Stretch is used
fn test_box_min_height(width: f32, min_height: f32, color: Color) -> Container {
    container()
        .width(width)
        .height(at_least(min_height))
        .background(color)
        .corner_radius(2.0)
}

/// Test box with minimum width (for column cross-axis tests)
/// Width can stretch beyond min_width when CrossAlignment::Stretch is used
fn test_box_min_width(min_width: f32, height: f32, color: Color) -> Container {
    container()
        .width(at_least(min_width))
        .height(height)
        .background(color)
        .corner_radius(2.0)
}

// =============================================================================
// Row MainAlignment Tests
// =============================================================================

fn row_main_axis_tests() -> impl Widget {
    container()
        .layout(Flex::column().spacing(2.0))
        .child(row_main_axis_row("Start", MainAlignment::Start))
        .child(row_main_axis_row("Center", MainAlignment::Center))
        .child(row_main_axis_row("End", MainAlignment::End))
        .child(row_main_axis_row("Between", MainAlignment::SpaceBetween))
        .child(row_main_axis_row("Around", MainAlignment::SpaceAround))
        .child(row_main_axis_row("Evenly", MainAlignment::SpaceEvenly))
}

fn row_main_axis_row(name: &'static str, alignment: MainAlignment) -> impl Widget {
    container()
        .layout(Flex::row().spacing(4.0))
        .child(container().width(42.0).child(label(name)))
        .child(
            container()
                .width(200.0)
                .height(22.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(3.0)
                .layout(Flex::row().spacing(3.0).main_alignment(alignment))
                .child(test_box(Color::rgb(0.8, 0.3, 0.3)))
                .child(test_box(Color::rgb(0.3, 0.8, 0.3)))
                .child(test_box(Color::rgb(0.3, 0.3, 0.8))),
        )
}

// =============================================================================
// Row CrossAlignment Tests
// =============================================================================

fn row_cross_axis_tests() -> impl Widget {
    container()
        .layout(Flex::column().spacing(2.0))
        .child(row_cross_axis_row("Start", CrossAlignment::Start))
        .child(row_cross_axis_row("Center", CrossAlignment::Center))
        .child(row_cross_axis_row("End", CrossAlignment::End))
        .child(row_cross_axis_row("Stretch", CrossAlignment::Stretch))
}

fn row_cross_axis_row(name: &'static str, alignment: CrossAlignment) -> impl Widget {
    container()
        .layout(Flex::row().spacing(4.0))
        .child(container().width(42.0).child(label(name)))
        .child(
            container()
                .width(200.0)
                .height(36.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(3.0)
                .layout(Flex::row().spacing(3.0).cross_alignment(alignment))
                // Use at_least() for height (cross-axis in row) so Stretch can expand them
                .child(test_box_min_height(24.0, 12.0, Color::rgb(0.8, 0.3, 0.3)))
                .child(test_box_min_height(24.0, 26.0, Color::rgb(0.3, 0.8, 0.3)))
                .child(test_box_min_height(24.0, 18.0, Color::rgb(0.3, 0.3, 0.8))),
        )
}

// =============================================================================
// Column MainAlignment Tests
// =============================================================================

fn column_main_axis_tests() -> impl Widget {
    container()
        .layout(Flex::row().spacing(4.0))
        .child(column_main_axis_col("Start", MainAlignment::Start))
        .child(column_main_axis_col("Center", MainAlignment::Center))
        .child(column_main_axis_col("End", MainAlignment::End))
        .child(column_main_axis_col("Between", MainAlignment::SpaceBetween))
        .child(column_main_axis_col("Around", MainAlignment::SpaceAround))
        .child(column_main_axis_col("Evenly", MainAlignment::SpaceEvenly))
}

fn column_main_axis_col(name: &'static str, alignment: MainAlignment) -> impl Widget {
    container()
        .layout(Flex::column().spacing(2.0))
        .child(label(name))
        .child(
            container()
                .width(48.0)
                .height(80.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(3.0)
                .layout(Flex::column().spacing(2.0).main_alignment(alignment))
                .child(test_box_varied(32.0, 12.0, Color::rgb(0.8, 0.5, 0.3)))
                .child(test_box_varied(32.0, 12.0, Color::rgb(0.5, 0.8, 0.3)))
                .child(test_box_varied(32.0, 12.0, Color::rgb(0.3, 0.5, 0.8))),
        )
}

// =============================================================================
// Column CrossAlignment Tests
// =============================================================================

fn column_cross_axis_tests() -> impl Widget {
    container()
        .layout(Flex::row().spacing(4.0))
        .child(column_cross_axis_col("Start", CrossAlignment::Start))
        .child(column_cross_axis_col("Center", CrossAlignment::Center))
        .child(column_cross_axis_col("End", CrossAlignment::End))
        .child(column_cross_axis_col("Stretch", CrossAlignment::Stretch))
}

fn column_cross_axis_col(name: &'static str, alignment: CrossAlignment) -> impl Widget {
    container()
        .layout(Flex::column().spacing(2.0))
        .child(label(name))
        .child(
            container()
                .width(56.0)
                .height(80.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(3.0)
                .layout(Flex::column().spacing(2.0).cross_alignment(alignment))
                // Use at_least() for width (cross-axis in column) so Stretch can expand them
                .child(test_box_min_width(16.0, 12.0, Color::rgb(0.8, 0.5, 0.3)))
                .child(test_box_min_width(38.0, 12.0, Color::rgb(0.5, 0.8, 0.3)))
                .child(test_box_min_width(26.0, 12.0, Color::rgb(0.3, 0.5, 0.8))),
        )
}

// =============================================================================
// Center Test - Verify centering works correctly
// =============================================================================

fn center_test() -> impl Widget {
    container()
        .layout(Flex::column().spacing(4.0))
        // Row with H+V centering
        .child(
            container()
                .layout(Flex::row().spacing(6.0))
                .child(
                    container()
                        .width(70.0)
                        .height(60.0)
                        .background(Color::rgb(0.2, 0.15, 0.25))
                        .corner_radius(4.0)
                        .layout(
                            Flex::row()
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(test_box(Color::rgb(0.8, 0.4, 0.4))),
                )
                .child(
                    container()
                        .width(70.0)
                        .height(60.0)
                        .background(Color::rgb(0.15, 0.2, 0.25))
                        .corner_radius(4.0)
                        .layout(
                            Flex::column()
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(test_box(Color::rgb(0.4, 0.8, 0.4))),
                ),
        )
        // Single box - should center
        .child(
            container()
                .layout(
                    Flex::row()
                        .spacing(6.0)
                        .cross_alignment(CrossAlignment::Start),
                )
                .child(container().width(36.0).child(label("Single")))
                .child(
                    container()
                        .width(70.0)
                        .height(50.0)
                        .background(Color::rgb(0.2, 0.25, 0.2))
                        .corner_radius(4.0)
                        .layout(
                            Flex::row()
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(test_box(Color::rgb(0.8, 0.4, 0.4))),
                ),
        )
        // Multiple boxes - should also center
        .child(
            container()
                .layout(
                    Flex::row()
                        .spacing(6.0)
                        .cross_alignment(CrossAlignment::Start),
                )
                .child(container().width(36.0).child(label("Multi")))
                .child(
                    container()
                        .width(100.0)
                        .height(50.0)
                        .background(Color::rgb(0.15, 0.25, 0.2))
                        .corner_radius(4.0)
                        .layout(
                            Flex::row()
                                .spacing(6.0)
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(test_box(Color::rgb(0.8, 0.4, 0.4)))
                        .child(test_box(Color::rgb(0.4, 0.8, 0.4))),
                ),
        )
}
