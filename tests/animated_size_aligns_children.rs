//! A box whose content-sized width is in flight aligns its children against
//! the width it has on that frame, not the one it is heading to (#792).
//!
//! The issue's own shape: centred in a column that fills the surface, its
//! width following its content with a 400 ms linear transition, clipping, and
//! holding one view at its natural size whose inner width is a signal. The
//! loop is the real one, stepped 16 ms at a time, and every frame of the
//! transition is asked where the view is, for a shrink and for a grow — and
//! for a `Flex` column centring across as well as for a `ZStack`, and for a
//! height as well as a width.
#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::widgets::Rect;

mod common;

const FRAME: Duration = Duration::from_millis(16);

/// How the shape lays out the one view it holds, and along which axis the two
/// of them change size.
#[derive(Clone, Copy)]
enum Aligner {
    ZStack,
    FlexColumn,
    /// A stack aligned centre start whose height follows its content.
    ZStackDown,
}

impl Aligner {
    fn vertical(self) -> bool {
        matches!(self, Aligner::ZStackDown)
    }
}

struct Shape {
    app: guido::testing::Headless,
    aligner: Aligner,
    extent: RwSignal<f32>,
    shape: WidgetRef,
    view: WidgetRef,
    at: Instant,
}

impl Shape {
    /// The shape holding a view `extent` long along the axis it changes on,
    /// settled.
    fn new(aligner: Aligner, extent: f32) -> Option<Self> {
        let mut app = common::headless()?;
        let w = create_signal(extent);
        let shape = create_widget_ref();
        let view = create_widget_ref();
        let surface = app.surface(
            SurfaceConfig::new()
                .height(100)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                let follows =
                    at_least(0.0).transition(Transition::new(400.0, TimingFunction::Linear));
                let inner = container().overflow(Overflow::Hidden);
                let inner = match aligner {
                    Aligner::ZStack => inner
                        .width(follows)
                        .layout(ZStack::new().align(Alignment::TOP_CENTER)),
                    Aligner::FlexColumn => inner
                        .width(follows)
                        .layout(Flex::column().cross_alignment(CrossAlignment::Center)),
                    Aligner::ZStackDown => inner
                        .height(follows)
                        .layout(ZStack::new().align(Alignment::CENTER_START)),
                };
                let content = if aligner.vertical() {
                    container().width(100.0).height(move || w.get())
                } else {
                    container().width(move || w.get()).height(30.0)
                };
                container()
                    .width(fill())
                    .layout(Flex::column().cross_alignment(CrossAlignment::Center))
                    .child(
                        inner
                            .child(
                                container()
                                    .width(natural())
                                    .height(natural())
                                    .child(content)
                                    .widget_ref(view),
                            )
                            .widget_ref(shape),
                    )
            },
        );
        app.configure(surface, 400, 100, 1.0);
        let at = Instant::now();
        app.step_at(at);
        let mut this = Self {
            app,
            aligner,
            extent: w,
            shape,
            view,
            at,
        };
        this.run_for(500);
        Some(this)
    }

    fn frame(&mut self) {
        self.at += FRAME;
        self.app.step_at(self.at);
    }

    fn run_for(&mut self, ms: u64) {
        let until = self.at + Duration::from_millis(ms);
        while self.at < until {
            self.frame();
        }
    }

    /// Where the shape and the view start along the axis they change on, and
    /// how long each is there.
    fn along(&self) -> ((f32, f32), (f32, f32)) {
        let along = |r: Rect| {
            if self.aligner.vertical() {
                (r.y, r.height)
            } else {
                (r.x, r.width)
            }
        };
        (
            along(self.shape.layout_rect().get_untracked()),
            along(self.view.layout_rect().get_untracked()),
        )
    }

    /// Change the view's length, and check every frame of the transition that
    /// follows: the view is centred in the shape as the shape is then, and
    /// the shape is actually in flight on the frames in between.
    fn assert_centred_through(&mut self, to: f32) {
        let ((_, from), _) = self.along();
        self.extent.set(to);
        let start = self.at;
        let mut frames = Vec::new();
        while self.at < start + Duration::from_millis(450) {
            self.frame();
            frames.push(((self.at - start).as_millis(), self.along()));
        }
        let in_flight = frames
            .iter()
            .filter(|(_, ((_, len), _))| (len - from).abs() > 1.0 && (len - to).abs() > 1.0)
            .count();
        assert!(
            in_flight > 10,
            "the shape animates from {from} to {to}: {frames:?}"
        );
        for &(ms, ((shape_at, shape_len), (view_at, view_len))) in &frames {
            let centred = shape_at + (shape_len - view_len) / 2.0;
            assert!(
                (view_at - centred).abs() < 0.5,
                "at {ms} ms the view is at {view_at}, centred in the shape would be {centred}: \
                 {frames:?}"
            );
        }
        let ((_, shape), (_, view)) = self.along();
        assert_eq!((shape, view), (to, to), "and it settles");
    }
}

#[test]
fn a_zstack_centres_its_child_while_shrinking() {
    let Some(mut shape) = Shape::new(Aligner::ZStack, 300.0) else {
        return;
    };
    shape.assert_centred_through(100.0);
}

#[test]
fn a_zstack_centres_its_child_while_growing() {
    let Some(mut shape) = Shape::new(Aligner::ZStack, 100.0) else {
        return;
    };
    shape.assert_centred_through(300.0);
}

#[test]
fn a_flex_column_centres_its_child_while_shrinking() {
    let Some(mut shape) = Shape::new(Aligner::FlexColumn, 300.0) else {
        return;
    };
    shape.assert_centred_through(100.0);
}

#[test]
fn a_flex_column_centres_its_child_while_growing() {
    let Some(mut shape) = Shape::new(Aligner::FlexColumn, 100.0) else {
        return;
    };
    shape.assert_centred_through(300.0);
}

#[test]
fn a_zstack_centres_its_child_while_its_height_shrinks() {
    let Some(mut shape) = Shape::new(Aligner::ZStackDown, 80.0) else {
        return;
    };
    shape.assert_centred_through(20.0);
}

#[test]
fn a_zstack_centres_its_child_while_its_height_grows() {
    let Some(mut shape) = Shape::new(Aligner::ZStackDown, 20.0) else {
        return;
    };
    shape.assert_centred_through(80.0);
}
