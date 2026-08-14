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
    App::new().run(|app| {
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
                        .child(container().text_color(Color::rgb(0.9, 0.9, 1.0)).child(text("1. Static Children (.child)")))
                        .child(container().text_color(Color::WHITE).child(text("These children are fixed at creation time:")))
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Child A"))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.2, 0.3, 0.4))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Child B"))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.4, 0.3, 0.2))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Child C"))
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
                            container().text_color(Color::rgb(1.0, 0.9, 0.9)).child(text("2. .maybe_child() - NOT REACTIVE!"))
                        )
                        .child(
                            container().text_color(Color::rgb(1.0, 0.7, 0.7)).child(text("LIMITATION: Evaluated ONCE at creation"))
                        )
                        .child(
                            container().text_color(Color::WHITE).child(text(move || format!("Signal: {} (but .maybe_child won't react!)", show_optional.get())))
                        )
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(container().text_color(Color::WHITE).child(text("Fixed")))
                                // Evaluated ONCE at creation — it will never
                                // update. Debug builds warn about exactly this
                                // read; the closure below is the fix.
                                .maybe_child(
                                    if show_optional.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.4, 0.2, 0.2))
                                                .corner_radius(4.0)
                                                .text_color(Color::WHITE).child(text("Frozen"))
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
                            container().text_color(Color::rgb(0.9, 1.0, 0.9)).child(text("3. Dynamic Children (.children) - REACTIVE!"))
                        )
                        .child(
                            container().text_color(Color::WHITE).child(text("These react to signal changes with state preservation"))
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
                                        .hover_state(|s| s.lighter(0.1))
                                        .pressed_state(|s| s.ripple())
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
                                        .text_color(Color::WHITE).child(text("Add"))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.4, 0.2, 0.2))
                                        .corner_radius(4.0)
                                        .hover_state(|s| s.lighter(0.1))
                                        .pressed_state(|s| s.ripple())
                                        .on_click(move || {
                                            items.update(|list: &mut Vec<Item>| {
                                                if !list.is_empty() {
                                                    list.pop();
                                                }
                                            });
                                        })
                                        .text_color(Color::WHITE).child(text("Remove"))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.2, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .hover_state(|s| s.lighter(0.1))
                                        .pressed_state(|s| s.ripple())
                                        .on_click(move || {
                                            items.update(|list: &mut Vec<Item>| {
                                                list.reverse();
                                            });
                                        })
                                        .text_color(Color::WHITE).child(text("Reverse"))
                                )
                        )
                        .child(
                            container().text_color(Color::rgb(0.8, 0.8, 0.8)).child(text("Notice: Reversing preserves widget state (animations, etc.)"))
                        )
                        .child(
                            // Dynamic list: key by ID preserves widget state on
                            // reorder; a changed item rebuilds only its row
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .children(keyed(
                                    move || items.get(),
                                    |item| item.id,
                                    |item| container()
                                        .padding(8.0)
                                        .background(item.color)
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text(item.name)),
                                ))
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
                            container().text_color(Color::rgb(0.9, 1.0, 0.9)).child(text("4. NEW! Mixing Static and Dynamic - ANY ORDER!"))
                        )
                        .child(
                            container().text_color(Color::WHITE).child(text("You can now freely mix static and dynamic children!"))
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .hover_state(|s| s.lighter(0.1))
                                .pressed_state(|s| s.ripple())
                                .on_click(move || {
                                    show_optional.update(|v| *v = !*v);
                                })
                                .text_color(Color::WHITE).child(text(move || {
                                        if show_optional.get() {
                                            "Click to Hide Middle".to_string()
                                        } else {
                                            "Click to Show Middle".to_string()
                                        }
                                    }))
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
                                        .text_color(Color::WHITE).child(text("Static Header"))
                                )
                                .child(
                                    // Dynamic child in the middle!
                                    move || {
                                        show_optional.get().then(|| container()
                                            .padding(8.0)
                                            .background(Color::rgb(0.5, 0.3, 0.5))
                                            .corner_radius(4.0)
                                            .text_color(Color::WHITE).child(text("Dynamic Middle!")))
                                    }
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.4, 0.3))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Static Footer"))
                                )
                        )
                        .child(
                            container().text_color(Color::rgb(0.8, 1.0, 0.8)).child(text("This was IMPOSSIBLE before - would panic!"))
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
                            container().text_color(Color::rgb(1.0, 0.9, 1.0)).child(text("5. Complex Mixing Example"))
                        )
                        .child(
                            container().text_color(Color::WHITE).child(text("Multiple static and dynamic children in any order:"))
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .hover_state(|s| s.lighter(0.1))
                                .pressed_state(|s| s.ripple())
                                .on_click(move || {
                                    show_optional2.update(|v| *v = !*v);
                                })
                                .text_color(Color::WHITE).child(text(move || {
                                        if show_optional2.get() {
                                            "Click to Hide Dynamics".to_string()
                                        } else {
                                            "Click to Show Dynamics".to_string()
                                        }
                                    }))
                        )
                        .child(
                            // Complex pattern: S D S D S
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(container().text_color(Color::WHITE).child(text("Static 1")))
                                .child(move || {
                                    show_optional2.get().then(|| container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.5, 0.2, 0.3))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Dynamic 1")))
                                })
                                .child(container().text_color(Color::WHITE).child(text("Static 2")))
                                .child(move || {
                                    show_optional.get().then(|| container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.3, 0.2, 0.5))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text("Dynamic 2")))
                                })
                                .child(container().text_color(Color::WHITE).child(text("Static 3")))
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
                            container().text_color(Color::rgb(0.9, 0.9, 1.0)).child(text("6. .children() - For keyed lists"))
                        )
                        .child(
                            container().text_color(Color::WHITE).child(text("Use this for lists that need state preservation:"))
                        )
                        .child(
                            // Can even mix static before keyed list!
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(container().text_color(Color::rgb(0.8, 0.8, 0.8)).child(text("Static header before list")))
                                .children(keyed(
                                    || vec!["Keyed Item 1", "Keyed Item 2"],
                                    |content| {
                                        let mut hasher = DefaultHasher::new();
                                        content.hash(&mut hasher);
                                        hasher.finish()
                                    },
                                    |content| container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.5, 0.3, 0.5))
                                        .corner_radius(4.0)
                                        .text_color(Color::WHITE).child(text(content)),
                                ))
                                .child(container().text_color(Color::rgb(0.8, 0.8, 0.8)).child(text("Static footer after list")))
                        )
                )
        )
        );

    app.add_surface(
        SurfaceConfig::new()
            .width(1800)
            .height(450)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || view,
    );
    });
}
