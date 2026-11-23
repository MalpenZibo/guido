#![cfg(feature = "testing")]
//! A widget written outside the crate gets the frames its animation needs, and
//! only those (#956).
//!
//! Two things have to be true for that. A widget has to be able to ask for the
//! first frame — a retarget happens in `layout`, and nothing public could queue
//! the call to `advance_animations` that moves it. And once it is moving, what
//! `advance_animations` returns has to decide whether the next frame comes:
//! the trait always said so, and the loop threw the answer away.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;
use guido::widget_prelude::*;
use guido::widgets::container::AnimationState;

fn fixed_bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(50)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
}

/// A frame's worth of time after `t0`, `i` times over.
fn frame(t0: Instant, i: u64) -> Instant {
    t0 + Duration::from_millis(16 * i)
}

/// A square painted in a colour that eases to whatever its signal says.
///
/// Everything here is public: the motion is an `AnimationState`, pointed at
/// its target in `layout`, moved on in `advance_animations`, and read in
/// `paint`.
struct Swatch {
    color: Signal<Color>,
    motion: AnimationState<Color>,
    /// How many times the loop has asked it to advance.
    advanced: Rc<Cell<u32>>,
}

impl Widget for Swatch {
    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        let target = self.color.get();
        if self.motion.is_initial() {
            self.motion.set_immediate(ctx, target);
        } else {
            let now = ctx.tree_ref().frame_instant();
            self.motion.animate_to(target, now);
            if self.motion.is_animating() {
                ctx.request_animation_frame();
            }
        }
        constraints.constrain(Size::new(20.0, 20.0))
    }

    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        self.advanced.set(self.advanced.get() + 1);
        // A frame is not a repaint: the value moved, so the widget says so.
        if self.motion.advance(tree.frame_instant()).is_changed() {
            tree.mark_needs_paint(id);
        }
        self.motion.is_animating()
    }

    fn paint(&self, ctx: &mut PaintContext) {
        ctx.draw_rounded_rect(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            self.motion.displayed(),
            0.0,
        );
    }
}

/// The acceptance criterion of #956, in the order it is written.
#[test]
fn a_colour_a_custom_widget_retargets_in_layout_eases_there_and_stops() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let color = create_signal(Color::rgb(1.0, 0.0, 0.0));
    let advanced = Rc::new(Cell::new(0));
    let counted = advanced.clone();
    let surface = app.surface(fixed_bar(), move || {
        container().child(Swatch {
            color: color.into(),
            motion: AnimationState::new(
                Color::TRANSPARENT,
                Transition::new(160.0, TimingFunction::Linear),
            ),
            advanced: counted.clone(),
        })
    });
    app.configure(surface, 200, 50, 1.0);
    let t0 = Instant::now();
    app.step_at(frame(t0, 0));
    app.step_at(frame(t0, 1));
    assert_eq!(app.read_pixel(surface, 10, 10), [255, 0, 0, 255], "at rest");
    assert_eq!(advanced.get(), 0, "nothing moving, nothing advanced");

    color.set(Color::rgb(0.0, 0.0, 1.0));
    // The retarget, and the frames it asks for.
    for i in 2..7 {
        app.step_at(frame(t0, i));
    }
    let [r, g, b, a] = app.read_pixel(surface, 10, 10);
    assert!(
        r > 0 && r < 255 && b > 0 && b < 255 && g == 0 && a == 255,
        "mid-way, between the two: {:?}",
        [r, g, b, a]
    );

    for i in 7..20 {
        app.step_at(frame(t0, i));
    }
    assert_eq!(
        app.read_pixel(surface, 10, 10),
        [0, 0, 255, 255],
        "and it arrives"
    );

    let settled = advanced.get();
    assert!(settled > 1, "eased over several frames, not one");
    assert_eq!(app.next_wait(), None, "stopped, it asks for nothing more");
    for i in 20..30 {
        app.step_at(frame(t0, i));
    }
    assert_eq!(advanced.get(), settled, "and is not advanced again");
}

/// Advanced while it says it needs to be, for as many frames as it says.
///
/// Started without `request_animation_frame`, by a signal read under the
/// widget's Animation job — so what this holds is the return value alone.
struct Countdown {
    kick: Signal<u32>,
    /// Frames it still wants after this one; `None` for ever.
    left: Option<u32>,
    advanced: Rc<Cell<u32>>,
}

impl Widget for Countdown {
    fn layout(&mut self, ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        with_signal_tracking(ctx.id(), JobType::Animation, || self.kick.get());
        Size::new(10.0, 10.0)
    }

    fn advance_animations(&mut self, _tree: &mut Tree, _id: WidgetId) -> bool {
        self.advanced.set(self.advanced.get() + 1);
        match &mut self.left {
            None => true,
            Some(0) => false,
            Some(left) => {
                *left -= 1;
                true
            }
        }
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

#[test]
fn true_from_advance_animations_is_another_frame_and_false_is_none() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let kick = create_signal(0u32);
    let advanced = Rc::new(Cell::new(0));
    let counted = advanced.clone();
    let surface = app.surface(fixed_bar(), move || {
        container().child(Countdown {
            kick: kick.into(),
            left: Some(3),
            advanced: counted.clone(),
        })
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();
    kick.set(1);
    for _ in 0..10 {
        app.step();
    }
    assert_eq!(
        advanced.get(),
        4,
        "advanced once for the kick, then once for each `true`, and never after the `false`"
    );
    assert_eq!(app.next_wait(), None, "and the loop sleeps");
}

/// A widget that never stops asking does not run inside a hidden panel: the
/// check before the call is what keeps its `true` from being a frame.
#[test]
fn a_widget_that_always_wants_a_frame_gets_none_inside_a_hidden_panel() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let shown = create_signal(true);
    let kick = create_signal(0u32);
    let advanced = Rc::new(Cell::new(0));
    let counted = advanced.clone();
    let surface = app.surface(fixed_bar(), move || {
        container().child(container().visible(shown).child(Countdown {
            kick: kick.into(),
            left: None,
            advanced: counted.clone(),
        }))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();
    kick.set(1);
    for _ in 0..3 {
        app.step();
    }
    assert!(advanced.get() >= 3, "running, it is advanced every frame");

    shown.set(false);
    app.step();
    app.step();
    let hidden = advanced.get();
    for _ in 0..10 {
        app.step();
    }
    assert_eq!(advanced.get(), hidden, "hidden, it is not advanced");
    assert_eq!(app.next_wait(), None, "and asks for no frame");

    shown.set(true);
    for _ in 0..3 {
        app.step();
    }
    assert!(advanced.get() > hidden, "shown again, it runs again");
}
