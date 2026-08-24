//! Changing a surface's anchor, size, margin and reservation at runtime.
//!
//! These four are the ones an `ExclusiveZone::Auto` reservation follows: it
//! resolves to the extent of the anchored axis plus that edge's margin, so
//! moving any of them moves it, and the anchor also decides *which* axis it
//! follows and which axes the compositor owns.
//!
//! They are also the ones with the sharp edges. `zwlr_layer_surface_v1` makes a
//! size of zero on an axis not anchored to opposite edges a protocol error, and
//! a non-zero one on an axis that *is* hands back a dimension the compositor
//! owns — so the anchor and the size have to be sent together or the connection
//! is closed at the next commit. A `content()` axis has no size until it has
//! been measured, so a reservation that follows one has to wait for the measure
//! rather than resolve against whatever the compositor last confirmed.
//!
//! Watch the strip of desktop this bar reserves: it should always match the
//! edge the bar is on and the size it is drawn at. **Sweep margin** drives the
//! same command every frame, which is where a missing commit or a missing
//! re-measure shows up as a reservation drifting away from the bar.

use std::time::Duration;

use guido::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Auto,
    Fixed,
    None,
    Ignore,
}

impl Zone {
    fn next(self) -> Self {
        match self {
            Zone::Auto => Zone::Fixed,
            Zone::Fixed => Zone::None,
            Zone::None => Zone::Ignore,
            Zone::Ignore => Zone::Auto,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Zone::Auto => "Auto",
            Zone::Fixed => "Fixed(60)",
            Zone::None => "None",
            Zone::Ignore => "Ignore",
        }
    }

    fn value(self) -> ExclusiveZone {
        match self {
            Zone::Auto => ExclusiveZone::Auto,
            Zone::Fixed => ExclusiveZone::Fixed(60),
            Zone::None => ExclusiveZone::None,
            Zone::Ignore => ExclusiveZone::Ignore,
        }
    }
}

/// The handle for a surface whose id arrives after its widgets are built.
fn handle(surface: RwSignal<Option<SurfaceId>>) -> SurfaceHandle {
    surface_handle(
        surface
            .get_untracked()
            .expect("the surface id is set once `add_surface` returns"),
    )
}

fn button<M>(label: impl IntoSignal<String, M>, on_click: impl Fn() + 'static) -> AnyWidget {
    container()
        .padding([6.0, 12.0])
        .background(Color::rgb(0.22, 0.24, 0.34))
        .corners(6.0)
        .when_hovered(|s| s.lighter(0.10))
        .when_pressed(|s| s.ripple())
        .on_click(on_click)
        .child(text(label).font_size(13.0).color(Color::WHITE))
        .into_any()
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        // What the bar currently is, so the captions can say it and the buttons
        // can toggle it. The surface itself is the source of truth; these mirror
        // what was last asked for.
        let top_bar = create_signal(true);
        let fixed_size = create_signal(true);
        let zone = create_signal(Zone::Auto);
        let margin = create_signal(0i32);
        let sweeping = create_signal(false);
        let tall = create_signal(false);
        // `add_surface` hands the id back, and the widget closure runs later —
        // so the closures reach it through a signal rather than by capture.
        let surface = create_signal(None::<SurfaceId>);

        let id = app.add_surface(
            SurfaceConfig::new()
                // Both axes named, because the anchor decides which one is ours
                // and the other's number is what gets sent when it stops being
                // the compositor's. Leaving one to `SurfaceConfig`'s default
                // means a re-anchoring reserves a width nobody chose.
                .width(220)
                .height(56)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(ExclusiveZone::Auto)
                .namespace("guido-surface-runtime")
                .layer(Layer::Top)
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .background_color(Color::rgb(0.10, 0.11, 0.16)),
            move || {
                let controls = move || {
                    [
                        // The anchor decides which axis the reservation follows
                        // and which axes are the compositor's, so the size has
                        // to travel with it.
                        button(
                            move || {
                                if top_bar.get() {
                                    "anchor: top bar".to_owned()
                                } else {
                                    "anchor: left dock".to_owned()
                                }
                            },
                            move || {
                                top_bar.update(|t| *t = !*t);
                                handle(surface).set_anchor(if top_bar.get() {
                                    Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
                                } else {
                                    Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM
                                });
                            },
                        ),
                        // `content()` has no size until it has been measured,
                        // and asking for its 1px placeholder collapses the
                        // surface with nothing to bring it back.
                        button(
                            move || {
                                if fixed_size.get() {
                                    "size: fixed".to_owned()
                                } else {
                                    "size: content()".to_owned()
                                }
                            },
                            move || {
                                fixed_size.update(|f| *f = !*f);
                                let h = handle(surface);
                                if fixed_size.get() {
                                    h.set_size(220, 56);
                                } else {
                                    h.set_size(content(), content());
                                }
                            },
                        ),
                        // An `Auto` reservation is the extent *plus* this, so
                        // moving it has to move the reservation with it.
                        button(
                            move || format!("margin: {}", margin.get()),
                            move || {
                                margin.update(|m| *m = (*m + 8) % 40);
                                handle(surface).set_margin(margin.get());
                            },
                        ),
                        // Only `Auto` reads a size; the other three are numbers
                        // that must reach the compositor whatever the size is
                        // doing.
                        button(
                            move || format!("zone: {}", zone.get().label()),
                            move || {
                                zone.update(|z| *z = z.next());
                                handle(surface).set_exclusive_zone(zone.get().value());
                            },
                        ),
                        // The same command every frame: where a missing commit
                        // or a missing re-measure shows as the reservation
                        // drifting away from the bar.
                        button(
                            move || {
                                if sweeping.get() {
                                    "sweep margin: on".to_owned()
                                } else {
                                    "sweep margin: off".to_owned()
                                }
                            },
                            move || sweeping.update(|s| *s = !*s),
                        ),
                        // With `size: content()` and `zone: Auto` this is the
                        // question the two of them answer together: the bar's
                        // own content grows over 400ms, so the measure has to
                        // run on frame after frame and the reservation follow it
                        // the whole way rather than jumping at the end.
                        button(
                            move || {
                                if tall.get() {
                                    "grow: 150".to_owned()
                                } else {
                                    "grow: 50".to_owned()
                                }
                            },
                            move || tall.update(|t| *t = !*t),
                        ),
                    ]
                };

                container()
                    // Reactive, because a `content()` surface has nothing for a
                    // `fill()` to fill: it would expand into the whole incoming
                    // constraint and take the surface with it. A default
                    // `Length` is "as large as what is inside".
                    .width(move || {
                        if fixed_size.get() {
                            fill()
                        } else {
                            Length::default()
                        }
                    })
                    .height(move || {
                        if fixed_size.get() {
                            fill()
                        } else {
                            Length::default()
                        }
                    })
                    .padding([6.0, 10.0])
                    // `layout` is a structural declaration and takes no signal —
                    // the rule this library settled on — so a bar that becomes a
                    // dock swaps the container rather than the layout inside it.
                    .child(move || {
                        let row = Flex::row()
                            .spacing(8.0)
                            .cross_alignment(CrossAlignment::Center);
                        let column = Flex::column()
                            .spacing(8.0)
                            .cross_alignment(CrossAlignment::Start);
                        // The animated block sits beside the controls, so a
                        // `content()` surface has something whose size moves.
                        let grown = container()
                            .width(120.0)
                            .height(move || if tall.get() { 150.0 } else { 50.0 })
                            .animate_height(Transition::new(400.0, TimingFunction::EaseOut))
                            .background(Color::rgb(0.30, 0.45, 0.35))
                            .corners(6.0)
                            .into_any();
                        if top_bar.get() {
                            container()
                                .layout(row)
                                .children(controls())
                                .child(grown)
                                .into_any()
                        } else {
                            container()
                                .layout(column)
                                .children(controls())
                                .child(grown)
                                .into_any()
                        }
                    })
            },
        );

        surface.set(Some(id));

        // A background task cannot touch a signal or a handle — its future has
        // to be `Send` and neither is — so it only beats time through a
        // `WriteSignal`, and an effect on the main thread does the driving.
        let tick = create_signal(0u32);
        let tick_w = tick.writer();
        create_task(move |ctx| async move {
            while ctx.is_running() {
                tokio::time::sleep(Duration::from_millis(16)).await;
                tick_w.update(|t| *t = t.wrapping_add(1));
            }
        });

        create_effect(move || {
            let t = tick.get();
            if !sweeping.get() {
                return;
            }
            // A triangle wave, so the margin is always moving and the same
            // command is sent on frame after frame.
            let step = t % 120;
            let m = if step < 60 { step } else { 120 - step } as i32;
            margin.set(m / 2);
            handle(surface).set_margin(margin.get());
        });
    });
}
