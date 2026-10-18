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
        Self::build(aligner, extent, false)
    }

    /// The same, with the view's own length animated by the transition the
    /// shape's is (#794).
    fn with_view_in_flight(extent: f32) -> Option<Self> {
        Self::build(Aligner::ZStack, extent, true)
    }

    fn build(aligner: Aligner, extent: f32, view_animates: bool) -> Option<Self> {
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
                let length = move || {
                    let length = move || w.get();
                    if view_animates {
                        length.transition(Transition::new(400.0, TimingFunction::Linear))
                    } else {
                        length.into_animated()
                    }
                };
                let content = if aligner.vertical() {
                    container().width(100.0).height(length())
                } else {
                    container().width(length()).height(30.0)
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

/// A box whose width and height both follow their content, at most 100 wide,
/// holding [`Wrapping`] words `words` wide.
struct TwoAxes {
    app: guido::testing::Headless,
    words: RwSignal<f32>,
    shape: WidgetRef,
    at: Instant,
}

/// Lines `LINE` tall across whatever width it is offered, as a label wraps:
/// as wide as its words where it can be, and a line more each time the width
/// falls under another share of them.
struct Wrapping {
    words: Signal<f32>,
}

const LINE: f32 = 20.0;

impl Widget for Wrapping {
    fn layout(
        &mut self,
        _: &mut guido::widget_prelude::LayoutCtx,
        offered: guido::layout::Constraints,
    ) -> Size {
        let words = self.words.get();
        let width = words.min(offered.max_width).max(1.0);
        Size::new(width, (words / width).ceil() * LINE)
    }

    fn paint(&self, _: &mut guido::widget_prelude::PaintContext) {}
}

impl TwoAxes {
    fn new(words: f32) -> Option<Self> {
        let mut app = common::headless()?;
        let w = create_signal(words);
        let shape = create_widget_ref();
        let surface = app.surface(
            SurfaceConfig::new()
                .height(200)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            move || {
                let follows = Transition::new(400.0, TimingFunction::Linear);
                container()
                    .width(at_most(100.0).transition(follows.clone()))
                    .height(at_least(0.0).transition(follows))
                    .child(Wrapping { words: w.into() })
                    .widget_ref(shape)
            },
        );
        app.configure(surface, 400, 200, 1.0);
        let at = Instant::now();
        app.step_at(at);
        let mut this = Self {
            app,
            words: w,
            shape,
            at,
        };
        for _ in 0..40 {
            this.frame();
        }
        Some(this)
    }

    fn frame(&mut self) -> Rect {
        self.at += FRAME;
        self.app.step_at(self.at);
        self.shape.layout_rect().get_untracked()
    }

    /// Change the words, and answer with the box on every frame of the
    /// transition that follows.
    fn transition_to(&mut self, words: f32) -> Vec<(f32, f32)> {
        self.words.set(words);
        (0..30)
            .map(|_| {
                let r = self.frame();
                (r.width, r.height)
            })
            .collect()
    }
}

/// While its width is in flight a box does not follow the height of content
/// laid out in the width it is passing through: words that fit one line at
/// either end keep the box one line tall all the way, though they wrap onto
/// more in between. Following them, the box jumped five lines tall and back.
#[test]
fn a_height_that_has_nowhere_to_go_holds_while_the_width_moves() {
    let Some(mut shape) = TwoAxes::new(20.0) else {
        return;
    };
    let frames = shape.transition_to(90.0);
    assert!(
        frames
            .iter()
            .filter(|(w, _)| *w > 21.0 && *w < 89.0)
            .count()
            > 10,
        "the width is in flight: {frames:?}"
    );
    assert!(
        frames.iter().all(|&(_, h)| h == LINE),
        "the height holds at one line: {frames:?}"
    );
    assert_eq!(frames.last(), Some(&(90.0, LINE)), "and it settles");
}

/// Where the height moves as well, it moves where its own animation has it,
/// never past either end, though the content in between is taller.
#[test]
fn a_height_in_flight_beside_a_width_goes_only_where_it_is_animated() {
    let Some(mut shape) = TwoAxes::new(20.0) else {
        return;
    };
    let frames = shape.transition_to(150.0);
    let rising = frames.windows(2).all(|p| p[0].1 <= p[1].1);
    assert!(
        rising
            && frames
                .iter()
                .all(|&(_, h)| (LINE..=2.0 * LINE).contains(&h)),
        "the height goes from one line to two and no further: {frames:?}"
    );
    assert_eq!(frames.last(), Some(&(100.0, 2.0 * LINE)), "and it settles");
}

/// A view that changes while the shape is in flight is laid out again on the
/// next frame. The shape measures it at the box it is heading to before
/// laying it out in the one it has, and the measure is what runs it first: a
/// layout after it at the constraints the last layout had is no reason to
/// hand back what that layout answered.
#[test]
fn a_view_that_changes_in_flight_is_laid_out_again() {
    let Some(mut shape) = Shape::new(Aligner::ZStack, 300.0) else {
        return;
    };
    shape.extent.set(100.0);
    for _ in 0..10 {
        shape.frame();
    }
    shape.extent.set(200.0);
    shape.frame();
    let ((_, at), (_, view)) = shape.along();
    assert!(at > 200.0, "the shape is still in flight, at {at}");
    assert_eq!(view, 200.0, "the view is as wide as it now is");
}

/// A transition after another heads for the content as it is now, not for
/// what was measured on the way to the last one.
#[test]
fn a_second_transition_heads_for_the_content_as_it_is_now() {
    let Some(mut shape) = Shape::new(Aligner::ZStack, 300.0) else {
        return;
    };
    shape.assert_centred_through(100.0);
    shape.assert_centred_through(250.0);
}

/// What a box in flight lays out, and at which width it was offered it.
#[derive(Clone, Default)]
struct Calls(std::rc::Rc<std::cell::RefCell<Vec<f32>>>);

/// A view `width` wide that writes down every width it is laid out in.
struct Probe {
    width: Signal<f32>,
    calls: Calls,
}

impl Widget for Probe {
    fn layout(
        &mut self,
        _: &mut guido::widget_prelude::LayoutCtx,
        offered: guido::layout::Constraints,
    ) -> Size {
        self.calls.0.borrow_mut().push(offered.max_width);
        Size::new(self.width.get(), 30.0)
    }

    fn paint(&self, _: &mut guido::widget_prelude::PaintContext) {}
}

/// A box in flight asks what it holds for the box it is heading to once,
/// not on every frame: the answer is the same until something below changes,
/// and a frame lays the view out only in the box it has then.
#[test]
fn a_box_in_flight_measures_what_it_holds_once() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let width = create_signal(300.0f32);
    let calls = Calls::default();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(100)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        {
            let calls = calls.clone();
            move || {
                container()
                    .width(fill())
                    .layout(Flex::column().cross_alignment(CrossAlignment::Center))
                    .child(
                        container()
                            .width(
                                at_least(0.0)
                                    .transition(Transition::new(400.0, TimingFunction::Linear)),
                            )
                            .layout(ZStack::new().align(Alignment::TOP_CENTER))
                            .child(Probe {
                                width: width.into(),
                                calls: calls.clone(),
                            }),
                    )
            }
        },
    );
    app.configure(surface, 400, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    for _ in 0..40 {
        at += FRAME;
        app.step_at(at);
    }
    let heading_to = 400.0;
    calls.0.borrow_mut().clear();
    width.set(100.0);
    let frames = 30;
    for _ in 0..frames {
        at += FRAME;
        app.step_at(at);
    }
    let calls = calls.0.borrow();
    let in_flight = calls.iter().filter(|&&w| w < heading_to).count();
    assert!(in_flight > 20, "the box is in flight: {calls:?}");
    assert!(
        calls.iter().filter(|&&w| w == heading_to).count() <= 3,
        "the view is laid out where the box is heading on the frame that \
         starts it, measured there on the next, laid out there again once the \
         box arrives, and not otherwise: {calls:?}"
    );
    assert!(
        calls.len() <= frames + 2,
        "no frame lays it out more than once after that: {calls:?}"
    );
}

/// A shape that fits a view animating its own length targets where the view
/// is heading, while the view is drawn where it is (#794). With one
/// transition for both they start on one frame and arrive together, so the
/// shape is exactly as long as the view on every frame; taking the view's
/// in-flight length as its target restarted the shape's transition on every
/// frame, and it covered a third of the way while the view covered all of it.
fn assert_shape_keeps_up_with_the_view(from: f32, to: f32) {
    let Some(mut shape) = Shape::with_view_in_flight(from) else {
        return;
    };
    assert_eq!(shape.along().0.1, from, "at rest before the view moves");
    shape.extent.set(to);
    let mut frames = Vec::new();
    for _ in 0..30 {
        shape.frame();
        frames.push(shape.along());
    }
    let lengths: Vec<_> = frames.iter().map(|((_, s), (_, v))| (*s, *v)).collect();
    let in_flight = lengths
        .iter()
        .filter(|(_, v)| (v - from).abs() > 1.0 && (v - to).abs() > 1.0)
        .count();
    assert!(in_flight > 20, "the view is drawn in flight: {lengths:?}");
    let toward = lengths
        .windows(2)
        .all(|p| (p[1].1 - p[0].1) * (to - from) >= 0.0);
    assert!(toward, "and moves one way: {lengths:?}");
    for (shape, view) in &lengths {
        assert!(
            (shape - view).abs() < 0.5,
            "the shape is the view's length on every frame: {lengths:?}"
        );
    }
    assert_eq!(lengths.last(), Some(&(to, to)), "and they settle together");
}

#[test]
fn a_shape_keeps_up_with_a_view_that_shrinks_itself() {
    assert_shape_keeps_up_with_the_view(300.0, 100.0);
}

#[test]
fn a_shape_keeps_up_with_a_view_that_grows_itself() {
    assert_shape_keeps_up_with_the_view(100.0, 300.0);
}

/// What only paints moves nothing a target is measured from: a view fading
/// and turning in a shape that fits it costs the shape no measure. The view
/// is laid out on every frame, because its height changes, so the shape runs
/// its layout on every frame and would measure on every frame if the fade
/// said its answer depended on the pass. It does not: a measure is asked for
/// by a layout below that read a value in flight, and paint reads none.
#[cfg(feature = "render-stats")]
#[test]
fn a_view_that_only_paints_in_flight_costs_no_measure() {
    let Some(still) = layouts_while_the_height_changes(false) else {
        return;
    };
    let fading = layouts_while_the_height_changes(true).expect("an adapter, as above");
    assert_eq!(
        fading, still,
        "a fading, turning view is laid out as often as a still one"
    );
}

/// The layouts executed and skipped over 25 frames in which the view's height
/// changes on every frame, and its opacity and rotation are in flight where
/// `fading`.
#[cfg(feature = "render-stats")]
fn layouts_while_the_height_changes(fading: bool) -> Option<(u64, u64)> {
    use guido::render_stats::{get_stats, reset_stats};
    let mut app = common::headless()?;
    let shown = create_signal(true);
    let height = create_signal(31.0f32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(100)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            let fade = Transition::new(400.0, TimingFunction::Linear);
            container()
                .width(fill())
                .layout(Flex::column().cross_alignment(CrossAlignment::Center))
                .child(
                    container()
                        .width(at_least(0.0).transition(fade.clone()))
                        .layout(ZStack::new().align(Alignment::TOP_CENTER))
                        .child(
                            container()
                                .width(natural())
                                .opacity(
                                    (move || if shown.get() { 1.0 } else { 0.2 })
                                        .transition(fade.clone()),
                                )
                                .rotate(
                                    (move || if shown.get() { 0.0 } else { 30.0 }).transition(fade),
                                )
                                .child(container().width(120.0).height(move || height.get())),
                        ),
                )
        },
    );
    app.configure(surface, 400, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    for _ in 0..40 {
        at += FRAME;
        app.step_at(at);
    }
    if fading {
        shown.set(false);
    }
    at += FRAME;
    app.step_at(at);
    reset_stats();
    for frame in 0..25 {
        height.set(30.0 + (frame % 2) as f32);
        at += FRAME;
        app.step_at(at);
    }
    let stats = get_stats();
    assert!(stats.layout_executed > 0, "the view is laid out: {stats:?}");
    Some((stats.layout_executed, stats.layout_skipped))
}
