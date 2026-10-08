//! A component answers the tree as the widget its body built would.
//!
//! `#[component]` generates a struct that holds that widget, and the tree only
//! ever talks to the struct. Whatever the struct does not pass on is answered
//! with the trait's default: a container that declares an exit, built by a
//! component, would be removed on the spot instead of playing it.

use guido::prelude::*;

mod common;
use common::Harness;

/// A card that slides out to `-100` when it is taken away.
#[component]
fn leaving_card() -> impl Widget {
    container().width(100.0).height(50.0).translate(
        Translate::NONE
            .transition(Transition::new(200.0, TimingFunction::Linear))
            .exiting_to(Translate::new(-100.0, 0.0)),
    )
}

#[test]
fn a_component_plays_the_exit_its_body_declares() {
    let mut h = Harness::laid_out(leaving_card(), 400.0, 400.0);

    let leaving = h
        .tree
        .with_widget_mut(h.root, |w, id, t| w.begin_exit(t, id))
        .expect("mounted");
    assert!(
        leaving,
        "the card declares an exit, so removing the component has to play it \
         rather than take the card away on the spot"
    );
    assert!(
        h.tree
            .with_widget(h.root, |w| w.is_exiting())
            .expect("mounted"),
        "and it is still leaving while the slide plays"
    );

    h.tree
        .with_widget_mut(h.root, |w, id, t| w.cancel_exit(t, id));
    assert!(
        !h.tree
            .with_widget(h.root, |w| w.is_exiting())
            .expect("mounted"),
        "asked for again, it stops leaving"
    );
}

/// What a component owns is the scope its body ran in, and it answers for that
/// one, as an `OwnedWidget` answers for its own. In a dynamic row the row's
/// scope already pauses it when the row leaves, so this is the contract rather
/// than something a leaving row would show.
#[test]
fn a_component_reports_the_scope_its_body_ran_in() {
    let h = Harness::laid_out(leaving_card(), 400.0, 400.0);
    assert!(
        h.tree
            .with_widget(h.root, |w| w.owned_scope())
            .expect("mounted")
            .is_some(),
        "a component owns the scope its body ran in, and has to say so"
    );
}

/// Through a real loop, where removing a row reconciles the list and the
/// frames that follow play the exit, because that is what a user sees.
#[cfg(feature = "testing")]
mod through_a_real_loop {
    use super::*;
    use std::time::{Duration, Instant};

    /// A red card, built by a component, that slides halfway out, to `-50`, over
    /// 200 ms when it is taken away: where it ends still covers what it
    /// covered, so only its going clears it.
    #[component]
    fn red_leaving_card() -> impl Widget {
        container()
            .width(100.0)
            .height(50.0)
            .background(Color::rgb(1.0, 0.0, 0.0))
            .translate(
                Translate::NONE
                    .transition(Transition::new(200.0, TimingFunction::Linear))
                    .exiting_to(Translate::new(-50.0, 0.0)),
            )
    }

    fn red_at(app: &guido::testing::Headless, surface: guido::surface::SurfaceId, x: u32) -> bool {
        let [r, g, b, _] = app.read_pixel(surface, x, 25);
        r > 128 && g < 100 && b < 100
    }

    #[test]
    fn a_removed_component_slides_out_before_it_goes() {
        let Some(mut app) = common::headless() else {
            return;
        };
        let shown = create_signal(true);
        let surface = app.surface(
            SurfaceConfig::new()
                .height(60)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                container().children(move || {
                    if shown.get() {
                        vec![red_leaving_card()]
                    } else {
                        Vec::new()
                    }
                })
            },
        );
        app.configure(surface, 200, 60, 1.0);
        let t0 = Instant::now();
        app.step_at(t0);
        assert!(
            red_at(&app, surface, 80),
            "the card is drawn where it stands"
        );

        shown.set(false);
        app.step_at(t0 + Duration::from_millis(10));
        app.step_at(t0 + Duration::from_millis(110));
        assert!(
            red_at(&app, surface, 20) && !red_at(&app, surface, 80),
            "halfway through its exit the card has slid to -25, so it covers \
             20 and no longer 80; a component that does not pass the exit on \
             is gone at once"
        );

        app.step_at(t0 + Duration::from_millis(240));
        assert!(
            !red_at(&app, surface, 20),
            "once the exit settles the card is gone: one left in the tree at \
             -50 would still cover 20"
        );
    }
}
