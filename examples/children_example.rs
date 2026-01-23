//! Example demonstrating static and dynamic children.
//!
//! This example shows:
//! 1. Static children with .child() - Fixed at creation
//! 2. Conditional static with .maybe_child() - NOT reactive (evaluated once)
//! 3. Dynamic list with .children() - Fully reactive with keyed reconciliation
//! 4. NEW: Mixing static and dynamic children - Now works in any order!
//! 5. Unified .child() and .children() APIs - Accept both static and dynamic

use guido::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq)]
struct Item {
    id: u64,
    name: String,
    color: Color,
}

fn main() {
    // === Signals for reactive state ===
    // No need to clone signals anymore - they implement Copy!
    let show_optional = create_signal(true);
    let show_optional2 = create_signal(true);
    let items = create_signal(vec![
        Item {
            id: 1,
            name: "Item 1".to_string(),
            color: Color::rgb(0.8, 0.3, 0.3),
        },
        Item {
            id: 2,
            name: "Item 2".to_string(),
            color: Color::rgb(0.3, 0.8, 0.3),
        },
        Item {
            id: 3,
            name: "Item 3".to_string(),
            color: Color::rgb(0.3, 0.3, 0.8),
        },
    ]);

    let view = container()
        .layout(Flex::row().spacing(12.0))
        .padding(12.0)
        .child(
            // First column - sections 1-2
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    // === SECTION 1: Static children example ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(text("1. Static Children (.child)").color(Color::rgb(0.9, 0.9, 1.0)))
                        .child(text("These children are fixed at creation time:").color(Color::WHITE))
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .child(text("Child A").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.2, 0.3, 0.4))
                                        .corner_radius(4.0)
                                        .child(text("Child B").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.4, 0.3, 0.2))
                                        .corner_radius(4.0)
                                        .child(text("Child C").color(Color::WHITE))
                                )
                        )
                )
                )
                .child(
                    // === SECTION 2: Conditional static children (NOT REACTIVE) ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.2, 0.15, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("2. .maybe_child() - NOT REACTIVE!")
                                .color(Color::rgb(1.0, 0.9, 0.9))
                        )
                        .child(
                            text("LIMITATION: Evaluated ONCE at creation")
                                .color(Color::rgb(1.0, 0.7, 0.7))
                        )
                        .child(
                            text(move || format!("Signal: {} (but .maybe_child won't react!)", show_optional.get()))
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(text("Fixed").color(Color::WHITE))
                                // This is evaluated ONCE at creation - won't update!
                                .maybe_child(
                                    if show_optional.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.4, 0.2, 0.2))
                                                .corner_radius(4.0)
                                                .child(text("Frozen").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                )
                        )
                )
                )
        )
        .child(
            // Second column - sections 3-4
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    // === SECTION 3: Dynamic children (FULLY REACTIVE) ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.2, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("3. Dynamic Children (.children) - REACTIVE!")
                                .color(Color::rgb(0.9, 1.0, 0.9))
                        )
                        .child(
                            text("These react to signal changes with state preservation")
                                .color(Color::WHITE)
                        )
                        .child(
                            // Control buttons
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.2, 0.4, 0.2))
                                        .corner_radius(4.0)
                                        .on_click(move || {
                                            items.update(|list: &mut Vec<Item>| {
                                                let id = list.len() as u64 + 1;
                                                list.push(Item {
                                                    id,
                                                    name: format!("Item {}", id),
                                                    color: Color::rgb(
                                                        0.5 + (id as f32 * 0.3) % 0.5,
                                                        0.5 + (id as f32 * 0.5) % 0.5,
                                                        0.5 + (id as f32 * 0.7) % 0.5,
                                                    ),
                                                });
                                            });
                                        })
                                        .child(text("Add").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.4, 0.2, 0.2))
                                        .corner_radius(4.0)
                                        .on_click(move || {
                                            items.update(|list: &mut Vec<Item>| {
                                                if !list.is_empty() {
                                                    list.pop();
                                                }
                                            });
                                        })
                                        .child(text("Remove").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.2, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .on_click(move || {
                                            items.update(|list: &mut Vec<Item>| {
                                                list.reverse();
                                            });
                                        })
                                        .child(text("Reverse").color(Color::WHITE))
                                )
                        )
                        .child(
                            text("Notice: Reversing preserves widget state (animations, etc.)")
                                .color(Color::rgb(0.8, 0.8, 0.8))
                        )
                        .child(
                            // Dynamic list with keyed reconciliation
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .children(move || {
                                    items.get().into_iter().map(|item| {
                                        // Key by ID - preserves widget state on reorder!
                                        (item.id, container()
                                            .padding(8.0)
                                            .background(item.color)
                                            .corner_radius(4.0)
                                            .child(text(item.name).color(Color::WHITE)))
                                    })
                                })
                        )
                )
                )
                .child(
                    // === SECTION 4: NEW! Mixing static and dynamic children ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.25, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("4. NEW! Mixing Static and Dynamic - ANY ORDER!")
                                .color(Color::rgb(0.9, 1.0, 0.9))
                        )
                        .child(
                            text("You can now freely mix static and dynamic children!")
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .on_click(move || {
                                    show_optional.update(|v| *v = !*v);
                                })
                                .child(
                                    text(move || {
                                        if show_optional.get() {
                                            "Click to Hide Middle".to_string()
                                        } else {
                                            "Click to Show Middle".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Demonstrate mixing: static -> dynamic -> static
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.4, 0.3))
                                        .corner_radius(4.0)
                                        .child(text("Static Header").color(Color::WHITE))
                                )
                                .child(
                                    // Dynamic child in the middle!
                                    move || {
                                        if show_optional.get() {
                                            Some(
                                                container()
                                                    .padding(8.0)
                                                    .background(Color::rgb(0.5, 0.3, 0.5))
                                                    .corner_radius(4.0)
                                                    .child(text("Dynamic Middle!").color(Color::WHITE))
                                            )
                                        } else {
                                            None
                                        }
                                    }
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.4, 0.3))
                                        .corner_radius(4.0)
                                        .child(text("Static Footer").color(Color::WHITE))
                                )
                        )
                        .child(
                            text("This was IMPOSSIBLE before - would panic!")
                                .color(Color::rgb(0.8, 1.0, 0.8))
                        )
                )
                )
        )
        .child(
            // Third column - sections 5-6
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    // === SECTION 5: Complex mixing example ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.18, 0.15, 0.25))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("5. Complex Mixing Example")
                                .color(Color::rgb(1.0, 0.9, 1.0))
                        )
                        .child(
                            text("Multiple static and dynamic children in any order:")
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .on_click(move || {
                                    show_optional2.update(|v| *v = !*v);
                                })
                                .child(
                                    text(move || {
                                        if show_optional2.get() {
                                            "Click to Hide Dynamics".to_string()
                                        } else {
                                            "Click to Show Dynamics".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Complex pattern: S D S D S
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(text("Static 1").color(Color::WHITE))
                                .child(move || {
                                    if show_optional2.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.5, 0.2, 0.3))
                                                .corner_radius(4.0)
                                                .child(text("Dynamic 1").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .child(text("Static 2").color(Color::WHITE))
                                .child(move || {
                                    if show_optional.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.3, 0.2, 0.5))
                                                .corner_radius(4.0)
                                                .child(text("Dynamic 2").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .child(text("Static 3").color(Color::WHITE))
                        )
                )
                )
                .child(
                    // === SECTION 6: .children() for keyed lists ===
                    container()
                .padding(12.0)
                .background(Color::rgb(0.18, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("6. .children() - For keyed lists")
                                .color(Color::rgb(0.9, 0.9, 1.0))
                        )
                        .child(
                            text("Use this for lists that need state preservation:")
                                .color(Color::WHITE)
                        )
                        .child(
                            // Can even mix static before keyed list!
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(text("Static header before list").color(Color::rgb(0.8, 0.8, 0.8)))
                                .children(move || {
                                    vec!["Keyed Item 1", "Keyed Item 2"].into_iter().map(|content| {
                                        let mut hasher = DefaultHasher::new();
                                        content.hash(&mut hasher);
                                        let key = hasher.finish();
                                        (key, container()
                                            .padding(8.0)
                                            .background(Color::rgb(0.5, 0.3, 0.5))
                                            .corner_radius(4.0)
                                            .child(text(content).color(Color::WHITE)))
                                    })
                                })
                                .child(text("Static footer after list").color(Color::rgb(0.8, 0.8, 0.8)))
                        )
                )
        )
        );

    App::new()
        .width(1800)
        .height(450)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
