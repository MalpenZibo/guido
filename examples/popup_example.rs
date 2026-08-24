//! xdg popups anchored to a bar: real menu semantics.
//!
//! Clicking the button opens a grabbed popup under it — the compositor
//! positions it (flipping/sliding at screen edges) and dismisses it when
//! you click anywhere outside. No fullscreen overlay involved.
//!
//! "Settings ▸" opens a **nested** popup, parented to the first one rather than
//! to the bar. That is the case with the sharpest failure: a popup must be
//! destroyed before its parent or the compositor raises `not_the_topmost_popup`
//! and closes the connection, so getting the teardown order wrong does not draw
//! something odd, it kills the application. Worth reaching by hand — dismiss the
//! pair with one outside click, and close the parent while the child is open.

use std::cell::RefCell;
use std::rc::Rc;

use guido::prelude::*;

fn menu_entry(label: &str) -> Container {
    container()
        .padding([8.0, 12.0])
        .corners(6.0)
        .when_hovered(|s| s.lighter(0.12))
        .child(text(label).font_size(13))
}

/// The entry that opens a popup parented to the popup it lives in.
///
/// The parent arrives through a slot rather than by value: the factory that
/// builds a popup's content runs *before* `spawn_popup` hands back its handle,
/// so at the time this entry is built its own parent does not exist yet.
fn submenu_entry(parent: Rc<RefCell<Option<PopupHandle>>>, open: RwSignal<bool>) -> Container {
    let entry_ref = create_widget_ref();
    let child_slot: Rc<RefCell<Option<PopupHandle>>> = Rc::new(RefCell::new(None));

    container()
        .widget_ref(entry_ref)
        .padding([8.0, 12.0])
        .corners(6.0)
        .when_hovered(|s| s.lighter(0.12))
        .on_click(move || {
            if open.get() {
                if let Some(child) = child_slot.borrow_mut().take() {
                    child.close();
                }
                return;
            }

            let Some(parent_id) = parent.borrow().as_ref().map(|p| p.id()) else {
                return;
            };

            // Parented to the popup, not to the bar: that is what makes it a
            // nested one, and what puts it above its parent in the grab chain.
            let child = spawn_popup(
                parent_id,
                PopupConfig::new(180)
                    .anchor_rect(entry_ref.rect().get())
                    .anchor(PopupAnchor::Right)
                    .gravity(PopupGravity::Right)
                    .grab()
                    .background_color(Color::TRANSPARENT),
                || {
                    container()
                        .width(fill())
                        .background(Color::rgb(0.16, 0.16, 0.24))
                        .corners(10.0)
                        .padding(8.0)
                        .layout(Flex::column().spacing(2.0))
                        .child(menu_entry("Appearance"))
                        .child(menu_entry("Keyboard"))
                        .child(menu_entry("Network"))
                },
            );
            open.set(true);

            create_effect(move || {
                if child.dismissed() {
                    open.set(false);
                }
            });

            *child_slot.borrow_mut() = Some(child);
        })
        .child(
            text(move || {
                if open.get() {
                    "Settings ◂".to_string()
                } else {
                    "Settings ▸".to_string()
                }
            })
            .font_size(13),
        )
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let button_ref = create_widget_ref();
        let menu_open = create_signal(false);
        let submenu_open = create_signal(false);
        let popup_slot: Rc<RefCell<Option<PopupHandle>>> = Rc::new(RefCell::new(None));
        // The bar id is only known after add_surface returns; the click
        // handler runs later, so a slot is enough.
        let bar_id_slot: Rc<RefCell<Option<SurfaceId>>> = Rc::new(RefCell::new(None));
        let bar_id_for_click = bar_id_slot.clone();

        let bar_id = app.add_surface(
            SurfaceConfig::new()
                .height(36)
                .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                let popup_slot = popup_slot.clone();
                // The same slot the parent handle lands in: the submenu reads
                // it at click time, by which point it holds its parent.
                let popup_slot_for_child = popup_slot.clone();
                container()
                    .width(fill())
                    .layout(
                        Flex::row()
                            .spacing(12.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        container()
                            .widget_ref(button_ref)
                            .padding([6.0, 16.0])
                            .background(move || {
                                if menu_open.get() {
                                    Color::rgb(0.35, 0.35, 0.5)
                                } else {
                                    Color::rgb(0.25, 0.25, 0.35)
                                }
                            })
                            .corners(6.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .on_click(move || {
                                if menu_open.get() {
                                    // Toggle closed
                                    if let Some(popup) = popup_slot.borrow_mut().take() {
                                        popup.close();
                                    }
                                    return;
                                }

                                let popup = spawn_popup(
                                    bar_id_for_click.borrow().unwrap(),
                                    PopupConfig::new(220)
                                        .anchor_rect(button_ref.rect().get())
                                        .anchor(PopupAnchor::Top)
                                        .gravity(PopupGravity::Top)
                                        .grab()
                                        .background_color(Color::TRANSPARENT),
                                    {
                                        let submenu_parent = popup_slot_for_child.clone();
                                        move || {
                                            // Auto-height popup: the content
                                            // wraps, no fill-height here
                                            container()
                                                .width(fill())
                                                .background(Color::rgb(0.13, 0.13, 0.2))
                                                .corners(10.0)
                                                .padding(8.0)
                                                .layout(Flex::column().spacing(2.0))
                                                .child(menu_entry("Profile"))
                                                .child(submenu_entry(submenu_parent, submenu_open))
                                                .child(menu_entry("About"))
                                                .child(menu_entry("Quit"))
                                        }
                                    },
                                );
                                menu_open.set(true);

                                // Reset state when the compositor dismisses
                                // the popup (outside click) or close() runs
                                create_effect(move || {
                                    if popup.dismissed() {
                                        menu_open.set(false);
                                    }
                                });

                                *popup_slot.borrow_mut() = Some(popup);
                            })
                            .child(text(move || {
                                if menu_open.get() {
                                    "Menu ▾ (click outside to dismiss)".to_string()
                                } else {
                                    "Menu ▸".to_string()
                                }
                            })),
                    )
            },
        );
        *bar_id_slot.borrow_mut() = Some(bar_id);
    });
}
