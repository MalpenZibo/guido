//! A child on its way out of a `ZStack` no longer leads the stack's size, so a
//! shape that sizes to its content starts moving toward the arriving view on
//! the first frame after the swap rather than after the exit (#790).
//!
//! The issue's own stack: a shape with 400 ms linear size transitions that
//! clips, a stack aligned top centre, and one keyed view, 300×40 or 100×30,
//! that fades in and out over 300 ms. The loop is the real one, stepped 16 ms
//! at a time, so a frame's numbers are the line from 300×40 to 100×30.
#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::widgets::Rect;

mod common;

const FRAME: Duration = Duration::from_millis(16);

fn linear(ms: f32) -> Transition {
    Transition::new(ms, TimingFunction::Linear)
}

/// The view `key` builds: a white box of its size, fading in and out, at its
/// natural size so a shape in flight never cuts it.
fn view(wide: bool, widget_ref: WidgetRef) -> Container {
    let (w, h) = if wide { (300.0, 40.0) } else { (100.0, 30.0) };
    container()
        .width(natural())
        .height(natural())
        .opacity(
            1.0.transition(linear(300.0))
                .entering_from(0.0)
                .exiting_to(0.0),
        )
        .child(container().width(w).height(h).background(Color::WHITE))
        .widget_ref(widget_ref)
}

/// The shape, holding the views `views` names — `true` for the wide one — each
/// with a `fill()` background behind it that leaves with its key. It starts on
/// the wide view, settled.
struct Island {
    app: guido::testing::Headless,
    surface: guido::surface::SurfaceId,
    views: RwSignal<Vec<bool>>,
    shape: WidgetRef,
    wide: WidgetRef,
    wide_background: WidgetRef,
    narrow: WidgetRef,
    at: Instant,
}

impl Island {
    fn new() -> Option<Self> {
        let mut app = common::headless()?;
        let views = create_signal(vec![true]);
        let shape = create_widget_ref();
        let wide = create_widget_ref();
        let wide_background = create_widget_ref();
        let narrow = create_widget_ref();
        let background = move |wide: bool| {
            let background = container()
                .width(fill())
                .height(fill())
                .opacity(1.0.transition(linear(300.0)).exiting_to(0.0));
            if wide {
                background.widget_ref(wide_background)
            } else {
                background
            }
        };
        let surface = app.surface(
            SurfaceConfig::new()
                .height(100)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                container().child(
                    container()
                        .width(at_least(0.0).transition(linear(400.0)))
                        .height(at_least(0.0).transition(linear(400.0)))
                        .overflow(Overflow::Hidden)
                        .layout(ZStack::new().align(Alignment::TOP_CENTER))
                        .children(keyed(
                            move || {
                                views
                                    .get()
                                    .into_iter()
                                    .flat_map(|w| [(w, true), (w, false)])
                                    .collect::<Vec<_>>()
                            },
                            |&key| key,
                            move |(w, is_background)| {
                                if is_background {
                                    background(w)
                                } else {
                                    view(w, if w { wide } else { narrow })
                                }
                            },
                        ))
                        .widget_ref(shape),
                )
            },
        );
        app.configure(surface, 400, 100, 1.0);
        let at = Instant::now();
        app.step_at(at);
        let mut island = Self {
            app,
            surface,
            views,
            shape,
            wide,
            wide_background,
            narrow,
            at,
        };
        island.run_until(at, 400);
        Some(island)
    }

    fn frame(&mut self) {
        self.at += FRAME;
        self.app.step_at(self.at);
    }

    /// Steps until `ms` have passed since `since`.
    fn run_until(&mut self, since: Instant, ms: u64) {
        while self.at < since + Duration::from_millis(ms) {
            self.frame();
        }
    }

    /// Show `views` instead, and step the frame that sees the change: the one
    /// its transitions start from, which this answers.
    fn swap(&mut self, views: Vec<bool>) -> Instant {
        self.views.set(views);
        self.frame();
        self.at
    }

    /// The shape's size in flight.
    fn shape(&self) -> (f32, f32) {
        let r = self.shape.layout_rect().get_untracked();
        (r.width, r.height)
    }

    /// Where `r` is laid out against the shape's origin, and how big.
    fn in_shape(&self, r: WidgetRef) -> Rect {
        let s = self.shape.layout_rect().get_untracked();
        r.layout_rect().get_untracked().offset(-s.x, -s.y)
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

/// The issue's acceptance: on the frame after the swap the shape is already on
/// its way to 100×30 — at 300×40 the frame before and 100×30 400 ms later —
/// while the wide view is still laid out 300×40, top centre in a stack of
/// 100×30, and still painted.
#[test]
fn a_leaving_view_does_not_hold_the_shape_open() {
    let Some(mut island) = Island::new() else {
        return;
    };
    assert_eq!(island.shape(), (300.0, 40.0));

    let swapped = island.swap(vec![false]);
    island.frame();
    let (w, h) = island.shape();
    assert!(
        w < 300.0 && h < 40.0,
        "the frame after the swap the shape has started toward 100x30: {w}x{h}"
    );
    assert_eq!(
        island.in_shape(island.wide),
        Rect::new(-100.0, 0.0, 300.0, 40.0),
        "laid out at its own size, top centre in a stack of 100x30"
    );
    assert_eq!(
        island.in_shape(island.narrow),
        Rect::new(0.0, 0.0, 100.0, 30.0)
    );

    // Inside the shape, past the narrow view's right edge and below it: only
    // the leaving view is there, and it is still drawn.
    let [r, g, b, _] = island.app.read_pixel(island.surface, 160, 34);
    assert!(
        r > 128 && g > 128 && b > 128,
        "the leaving view is painted: {:?}",
        [r, g, b]
    );

    // The line from 300x40 to 100x30, a frame at a time, through the exit
    // settling at 300 ms: nothing jumps when the leaving view goes.
    let mut frames = Vec::new();
    while island.at < swapped + Duration::from_millis(400) {
        island.frame();
        let ms = (island.at - swapped).as_secs_f32() * 1000.0;
        let (w, h) = island.shape();
        frames.push((ms, w, h, island.in_shape(island.narrow)));
    }
    for &(ms, w, h, narrow) in &frames {
        let t = (ms / 400.0).min(1.0);
        assert!(
            close(w, 300.0 - 200.0 * t) && close(h, 40.0 - 10.0 * t),
            "at {ms} ms the shape is {w}x{h}: {frames:?}"
        );
        assert_eq!(narrow, Rect::new(0.0, 0.0, 100.0, 30.0), "at {ms} ms");
    }
    assert_eq!(island.shape(), (100.0, 30.0));
    assert!(
        island.wide.widget().is_none(),
        "and the leaving view is gone by now"
    );
}

/// A `fill()` background leaving with its key is laid out at the stack's new
/// size, so it shrinks with the shape rather than holding the old one.
#[test]
fn a_leaving_background_follows_the_new_size() {
    let Some(mut island) = Island::new() else {
        return;
    };
    assert_eq!(
        island.in_shape(island.wide_background),
        Rect::new(0.0, 0.0, 300.0, 40.0)
    );
    island.swap(vec![false]);
    island.frame();
    assert_eq!(
        island.in_shape(island.wide_background),
        Rect::new(0.0, 0.0, 100.0, 30.0)
    );
}

/// Removed with nothing in its place, the view is all the stack has to be
/// sized by: the shape keeps its size while the view fades out in it.
#[test]
fn a_view_removed_with_nothing_in_its_place_fades_out_where_it_was() {
    let Some(mut island) = Island::new() else {
        return;
    };
    let removed = island.swap(Vec::new());
    while island.at < removed + Duration::from_millis(280) {
        island.frame();
        assert_eq!(island.shape(), (300.0, 40.0));
        assert!(island.wide.widget().is_some());
    }
}

/// The wide key coming back while it leaves stays, and leads again: the shape
/// turns round toward 300×40 on the next frame. The narrow view stays beside
/// it, so a wide view still taken for a leaving one would leave the narrow
/// one leading, and the shape on its way down.
#[test]
fn a_key_that_comes_back_mid_exit_leads_again() {
    let Some(mut island) = Island::new() else {
        return;
    };
    let swapped = island.swap(vec![false]);
    island.run_until(swapped, 100);
    assert!(island.shape().0 < 300.0, "on its way down");

    island.swap(vec![false, true]);
    let (turned, _) = island.shape();
    island.frame();
    let (after, _) = island.shape();
    assert!(
        after > turned,
        "back on its way up to the wide view: {turned} then {after}"
    );
    assert_eq!(
        island.in_shape(island.wide),
        Rect::new(0.0, 0.0, 300.0, 40.0)
    );
}
