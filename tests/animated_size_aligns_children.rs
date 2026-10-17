//! A box whose content-sized width is in flight aligns its children against
//! the width it has on that frame, not the one it is heading to (#792).
//!
//! The issue's own shape: centred in a column that fills the surface, its
//! width following its content with a 400 ms linear transition, clipping, and
//! holding one view at its natural size whose inner width is a signal. The
//! loop is the real one, stepped 16 ms at a time, and every frame of the
//! transition is asked where the view is, for a shrink and for a grow — and
//! for a `Flex` column centring across as well as for a `ZStack`.
#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::widgets::Rect;

mod common;

const FRAME: Duration = Duration::from_millis(16);

/// How the shape lays out the one view it holds.
#[derive(Clone, Copy)]
enum Aligner {
    ZStack,
    FlexColumn,
}

struct Shape {
    app: guido::testing::Headless,
    width: RwSignal<f32>,
    shape: WidgetRef,
    view: WidgetRef,
    at: Instant,
}

impl Shape {
    /// The shape holding a view `width` wide, settled.
    fn new(aligner: Aligner, width: f32) -> Option<Self> {
        let mut app = common::headless()?;
        let w = create_signal(width);
        let shape = create_widget_ref();
        let view = create_widget_ref();
        let surface = app.surface(
            SurfaceConfig::new()
                .height(100)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                let inner = container()
                    .width(at_least(0.0).transition(Transition::new(400.0, TimingFunction::Linear)))
                    .overflow(Overflow::Hidden);
                let inner = match aligner {
                    Aligner::ZStack => inner.layout(ZStack::new().align(Alignment::TOP_CENTER)),
                    Aligner::FlexColumn => {
                        inner.layout(Flex::column().cross_alignment(CrossAlignment::Center))
                    }
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
                                    .child(container().width(move || w.get()).height(30.0))
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
            width: w,
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

    fn rects(&self) -> (Rect, Rect) {
        (
            self.shape.layout_rect().get_untracked(),
            self.view.layout_rect().get_untracked(),
        )
    }

    /// Change the view's width, and check every frame of the transition that
    /// follows: the view is centred in the shape as the shape is then, and
    /// the shape is actually in flight on the frames in between.
    fn assert_centred_through(&mut self, to: f32) {
        let (from_shape, _) = self.rects();
        self.width.set(to);
        let start = self.at;
        let mut frames = Vec::new();
        while self.at < start + Duration::from_millis(450) {
            self.frame();
            let ms = (self.at - start).as_millis();
            let (shape, view) = self.rects();
            frames.push((ms, shape.x, shape.width, view.x, view.width));
        }
        let in_flight = frames
            .iter()
            .filter(|&&(_, _, w, _, _)| (w - from_shape.width).abs() > 1.0 && (w - to).abs() > 1.0)
            .count();
        assert!(
            in_flight > 10,
            "the shape animates from {} to {to}: {frames:?}",
            from_shape.width
        );
        for &(ms, shape_x, shape_w, view_x, view_w) in &frames {
            let centred = shape_x + (shape_w - view_w) / 2.0;
            assert!(
                (view_x - centred).abs() < 0.5,
                "at {ms} ms the view is at {view_x}, centred in the shape would be {centred}: \
                 {frames:?}"
            );
        }
        let (shape, view) = self.rects();
        assert_eq!((shape.width, view.width), (to, to), "and it settles");
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
