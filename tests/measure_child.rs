//! A layout that reads a child's natural size before it places it (#927).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use guido::prelude::*;
use guido::tree::LayoutCtx;
use guido::widget_prelude::{Constraints, Layout, WidgetId};

mod common;
use common::Harness;

/// Reads its first child unbounded, then lays it out in half of that; lays
/// out the rest, if any, as they ask.
struct HalfOfNatural;

impl Layout for HalfOfNatural {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        _origin: (f32, f32),
    ) -> Size {
        let child = children[0];
        let natural = ctx
            .measure_child(child, unbounded(constraints))
            .unwrap_or_default();
        let half = Constraints::new(0.0, 0.0, natural.width / 2.0, constraints.max_height);
        let size = ctx.layout_child(child, half).unwrap_or_default();
        for &rest in &children[1..] {
            ctx.layout_child(rest, constraints);
        }
        size
    }
}

/// Counts the layouts of the container it is given to.
struct Counted(Rc<Cell<u32>>);

impl Layout for Counted {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        _origin: (f32, f32),
    ) -> Size {
        self.0.set(self.0.get() + 1);
        let mut size = Size::zero();
        for &child in children {
            let own = ctx.layout_child(child, constraints).unwrap_or_default();
            size.width = size.width.max(own.width);
            size.height = size.height.max(own.height);
        }
        size
    }
}

#[test]
fn a_settled_child_is_not_laid_out_again_by_a_layout_that_reads_it_first() {
    let runs = Rc::new(Cell::new(0));
    let row = container().layout(HalfOfNatural).child(
        container()
            .layout(Counted(runs.clone()))
            .child(container().width(40.0).height(10.0)),
    );
    let mut h = Harness::laid_out(row, 400.0, 100.0);
    assert!(runs.get() > 0, "laid out at least once");

    // The row lays itself out again; nothing below it has changed.
    runs.set(0);
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 100.0);
    assert_eq!(runs.get(), 0, "the settled child was laid out again");
}

/// Reads its child in all the room there is, then places it in 100.
struct NarrowAfterReading;

impl Layout for NarrowAfterReading {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let child = children[0];
        ctx.measure_child(child, constraints);
        let size = ctx
            .layout_child(
                child,
                Constraints::new(0.0, 0.0, 100.0, constraints.max_height),
            )
            .unwrap_or_default();
        ctx.tree().set_origin(child, origin.0, origin.1);
        size
    }
}

/// What a child holds stands where the layout that placed the child put it,
/// though the read before it put it somewhere else: on the pass that lays the
/// child out again, the placement is run again too.
#[test]
fn a_child_read_and_then_placed_stands_where_it_was_placed() {
    let row = container().layout(NarrowAfterReading).child(
        container()
            .width(fill())
            .height(10.0)
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(container().width(20.0).height(10.0)),
    );
    let mut h = Harness::laid_out(row, 400.0, 100.0);
    let child = h.tree.get_children(h.root)[0];
    let inner = h.tree.get_children(child)[0];
    let centred = |h: &Harness| {
        let outer = h.tree.get_bounds(child).expect("laid out");
        let inner = h.tree.get_bounds(inner).expect("laid out");
        (outer.width, inner.x - outer.x)
    };
    assert_eq!(centred(&h), (100.0, 40.0), "placed in 100");

    // Settled: neither the read nor the placement runs.
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 100.0);
    assert_eq!(centred(&h), (100.0, 40.0), "on a pass that skips the child");

    // Marked: the read runs and centres it in 400, and the placement has to
    // run again though it is asked what it was asked before.
    h.tree.mark_needs_layout(child);
    h.lay_out(400.0, 100.0);
    assert_eq!(
        centred(&h),
        (100.0, 40.0),
        "on a pass that lays the child out"
    );
}

/// Reads its child in 50, then places it in all the room there is.
struct WideAfterReading;

impl Layout for WideAfterReading {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let child = children[0];
        ctx.measure_child(
            child,
            Constraints::new(0.0, 0.0, 50.0, constraints.max_height),
        );
        let size = ctx.layout_child(child, constraints).unwrap_or_default();
        ctx.tree().set_origin(child, origin.0, origin.1);
        size
    }
}

/// A size that follows its content with a transition is pointed where the
/// placement finds the content, never where the read before it did: a read
/// that pointed it would start a transition from the read's width on the
/// child's first appearance, and again every time it is laid out.
#[test]
fn a_read_does_not_point_a_size_that_follows_its_content() {
    let row = container().layout(WideAfterReading).child(
        container()
            .width(at_least(0.0).transition(400.0))
            .height(10.0)
            .child(container().width(fill()).height(10.0)),
    );
    let mut h = Harness::laid_out(row, 400.0, 100.0);
    let child = h.tree.get_children(h.root)[0];
    let width = |h: &Harness| h.tree.get_bounds(child).expect("laid out").width;
    assert_eq!(width(&h), 400.0, "on its first appearance");

    h.tree.mark_needs_layout(child);
    h.lay_out(400.0, 100.0);
    assert_eq!(width(&h), 400.0, "laid out again where nothing changed");
}

/// A size that follows its content below a child read before it is placed
/// still follows it, in the real loop: given the same room by the read and
/// the placement, it is run by the read first, and the placement has to run
/// it again to point it.
#[cfg(feature = "testing")]
#[test]
fn a_size_below_a_read_child_follows_its_content() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let content = create_signal(10.0f32);
    let follows = create_widget_ref();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(100)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().layout(WideAfterReading).child(
                // Content-sized on one axis, so not a boundary the loop
                // lays out again on its own: the row is, and the read with it.
                container()
                    .width(30.0)
                    .layout(ZStack::new().align(Alignment::TOP_START))
                    .child(
                        container()
                            .width(at_least(0.0).transition(400.0))
                            .height(10.0)
                            .child(container().width(content).height(10.0))
                            .widget_ref(follows),
                    ),
            )
        },
    );
    app.configure(surface, 400, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    let width = || follows.layout_rect().get_untracked().width;
    assert_eq!(width(), 10.0, "where it starts");

    content.set(20.0);
    let mut widths = Vec::new();
    for _ in 0..30 {
        at += Duration::from_millis(16);
        app.step_at(at);
        widths.push(width());
    }
    assert!(
        widths.iter().any(|&w| w > 10.0 && w < 20.0),
        "it moved through the transition rather than jumping: {widths:?}"
    );
    assert_eq!(width(), 20.0, "once its transition has had the time");
}

/// A wrapping row reads each child at its natural length before it breaks
/// the lines, then places it in its line: laid out again with nothing below
/// it changed, it lays none of them out again.
#[test]
fn a_settled_wrapping_row_does_not_lay_its_children_out_again() {
    let runs = Rc::new(Cell::new(0));
    let chip = |runs: &Rc<Cell<u32>>| {
        container()
            .width(fill())
            .layout(Counted(runs.clone()))
            .child(container().width(100.0).height(10.0))
    };
    let row = container()
        .width(250.0)
        .layout(Flex::row().spacing(4.0).wrap(true))
        .children((0..3).map(|_| chip(&runs)).collect::<Vec<_>>());
    let mut h = Harness::laid_out(row, 400.0, 100.0);
    assert!(runs.get() > 0, "laid out at least once");

    runs.set(0);
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 100.0);
    assert_eq!(runs.get(), 0, "a settled chip was laid out again");
}

/// What is below a child read first stands where the measure's placement put
/// it, after a measure as after a layout: a read that ran under other room
/// than last time moved it, and the placement answered from the measure's
/// cache would have left it there.
#[test]
fn a_child_read_in_a_measure_stands_where_the_measure_placed_it() {
    let row = container().layout(NarrowAfterReading).child(
        container()
            .width(fill())
            .height(10.0)
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(container().width(20.0).height(10.0)),
    );
    let mut h = Harness::new(row);
    let child = h.tree.get_children(h.root)[0];
    let inner = h.tree.get_children(child)[0];
    let centred = |h: &Harness| {
        let outer = h.tree.get_bounds(child).expect("measured");
        let inner = h.tree.get_bounds(inner).expect("measured");
        (outer.width, inner.x - outer.x)
    };
    let root = h.root;
    h.tree
        .measure_widget(root, Constraints::new(0.0, 0.0, 400.0, 100.0));
    assert_eq!(centred(&h), (100.0, 40.0), "the first measure");

    // Read in 300 now, placed in 100 as before.
    h.tree
        .measure_widget(root, Constraints::new(0.0, 0.0, 300.0, 100.0));
    assert_eq!(centred(&h), (100.0, 40.0), "a measure under other room");
}

/// A content-sized surface measures its root with no maximum on its content
/// axis and lays it out with one, so a layout that reads a child under the
/// height it is offered reads it under other constraints in each pass: each
/// pass keeps its own read, and a settled child is laid out by neither, frame
/// after frame, while something beside it changes.
#[cfg(feature = "testing")]
#[test]
fn a_settled_child_read_on_a_content_sized_surface_is_laid_out_by_neither_pass() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let runs = Rc::new(Cell::new(0));
    let grows = create_signal(1.0f32);
    let counted = runs.clone();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(content())
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .layout(HalfOfNatural)
                .child(
                    container()
                        .layout(Counted(counted.clone()))
                        .child(container().width(fill()).height(10.0)),
                )
                .child(container().width(grows).height(10.0))
        },
    );
    app.configure(surface, 400, 30, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    for _ in 0..3 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }

    runs.set(0);
    for i in 0..5 {
        grows.set(2.0 + i as f32);
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    assert_eq!(runs.get(), 0, "a settled child was laid out again");
}

/// Reads its only child in all the room there is, then places it in 100,
/// and makes it as tall as `tick` says, so a signal can lay it out again.
#[cfg(feature = "testing")]
fn sliding_row(tick: RwSignal<f32>, end: RwSignal<bool>, sliding: WidgetRef) -> Container {
    container().layout(NarrowAfterReading).child(
        container()
            .width(fill())
            .height(20.0)
            .layout(ZStack::new())
            .child(
                container()
                    .width(20.0)
                    .height(10.0)
                    .align_self(move || {
                        if end.get() {
                            Alignment::TOP_END
                        } else {
                            Alignment::TOP_START
                        }
                    })
                    .animate_position(Transition::new(160.0, TimingFunction::Linear))
                    .widget_ref(sliding),
            )
            .child(container().width(tick).height(1.0)),
    )
}

/// A child that slides to where layout moves it, inside a child read before
/// it is placed, is moved by the read too — somewhere the placement then
/// takes it back from. That move is no slide: a read that re-pointed it, on
/// every frame the read runs, would start its slide over each time, and it
/// would never arrive.
#[cfg(feature = "testing")]
#[test]
fn a_read_does_not_restart_a_slide() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let tick = create_signal(1.0f32);
    let end = create_signal(false);
    let sliding = create_widget_ref();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(50)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || sliding_row(tick, end, sliding),
    );
    app.configure(surface, 400, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    assert_eq!(sliding.rect().get_untracked().x, 0.0, "where it starts");

    // To the end of the 100 it is placed in, while something beside it lays
    // it out again on every frame of the slide.
    end.set(true);
    for i in 0..14 {
        tick.set(2.0 + (i % 2) as f32);
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    assert_eq!(
        sliding.rect().get_untracked().x,
        80.0,
        "arrived once its 160 ms had passed"
    );
}

/// No maximum on the width, the height `constraints` allow.
fn unbounded(constraints: Constraints) -> Constraints {
    Constraints::new(0.0, 0.0, f32::INFINITY, constraints.max_height)
}

/// What a layout read, one answer per run of its layout.
type Reads = Rc<RefCell<Vec<Size>>>;

/// Reads its first child unbounded and records what it answered — with
/// `layout_child` instead of `measure_child` when `twin` says so, which is
/// what a read answers like — then places it in `room`, or all there is.
/// Lays out the rest, if any, as they ask.
struct ReadsThenPlaces {
    reads: Reads,
    room: Option<f32>,
    twin: bool,
}

impl ReadsThenPlaces {
    fn new(reads: &Reads) -> Self {
        Self {
            reads: reads.clone(),
            room: None,
            twin: false,
        }
    }
}

impl Layout for ReadsThenPlaces {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        _origin: (f32, f32),
    ) -> Size {
        let child = children[0];
        let read = if self.twin {
            ctx.layout_child(child, unbounded(constraints))
        } else {
            ctx.measure_child(child, unbounded(constraints))
        };
        self.reads.borrow_mut().push(read.unwrap_or_default());
        let room = match self.room {
            Some(width) => Constraints::new(0.0, 0.0, width, constraints.max_height),
            None => constraints,
        };
        let size = ctx.layout_child(child, room).unwrap_or_default();
        for &rest in &children[1..] {
            ctx.layout_child(rest, constraints);
        }
        size
    }
}

/// A box that clips its content and eases a size that follows it — an
/// island — read before anything has placed it answers its content, as
/// `layout_child` does, and goes on answering it once settled (#930).
#[test]
fn a_read_before_the_first_placement_answers_the_content() {
    let reads = Reads::default();
    let row = container().layout(ReadsThenPlaces::new(&reads)).child(
        container()
            .width(at_least(0.0).transition(200.0))
            .height(at_least(0.0).transition(200.0))
            .overflow(Overflow::Hidden)
            .child(container().width(40.0).height(10.0)),
    );
    let mut h = Harness::laid_out(row, 400.0, 100.0);
    h.tree.mark_needs_layout(h.root);
    h.lay_out(400.0, 100.0);
    assert_eq!(
        *reads.borrow(),
        vec![Size::new(40.0, 10.0); 2],
        "the first read, and the settled one after it"
    );
}

/// A box that eases its width in from an enter, read before anything has
/// placed it, answers where that placement starts it: the enter's `from`.
#[test]
fn a_read_before_the_first_placement_answers_where_the_enter_starts() {
    let reads = Reads::default();
    let row = container().layout(ReadsThenPlaces::new(&reads)).child(
        container()
            .width(
                at_least(0.0)
                    .transition(200.0)
                    .entering_from(Length::exact(12.0)),
            )
            .height(10.0)
            .overflow(Overflow::Hidden)
            .child(container().width(40.0).height(10.0)),
    );
    let _h = Harness::laid_out(row, 400.0, 100.0);
    assert_eq!(reads.borrow().first(), Some(&Size::new(12.0, 10.0)));
}

/// On every pass, a read of a filling row of islands — boxes that clip what
/// they hold and ease a width that follows it — answers what `layout_child`
/// answers under the same constraints, though the row is then placed
/// narrower: from the first frame, while one island's content grows and the
/// island eases after it, and once it has settled (#930).
#[cfg(feature = "testing")]
#[test]
fn a_read_answers_what_layout_child_does_frame_after_frame() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let grows = create_signal(20.0f32);
    let island = |content: RwSignal<f32>| {
        container()
            .width(natural().transition(200.0))
            .height(36.0)
            .overflow(Overflow::Hidden)
            .child(container().width(content).height(36.0))
    };
    let end = move |reads: &Reads, twin: bool| {
        container()
            .height(40.0)
            .layout(ReadsThenPlaces {
                room: Some(100.0),
                twin,
                ..ReadsThenPlaces::new(reads)
            })
            .child(
                container()
                    .width(fill())
                    .layout(Flex::row().spacing(4.0))
                    .child(island(create_signal(30.0)))
                    .child(island(grows))
                    .child(island(create_signal(40.0))),
            )
    };
    let (reads, twin) = (Reads::default(), Reads::default());
    let (a, b) = (reads.clone(), twin.clone());
    let surface = app.surface(
        SurfaceConfig::new()
            .height(80)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .layout(Flex::column())
                .child(end(&a, false))
                .child(end(&b, true))
        },
    );
    app.configure(surface, 400, 80, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    for _ in 0..3 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    grows.set(60.0);
    for _ in 0..30 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }

    let (reads, twin) = (reads.borrow(), twin.borrow());
    assert_eq!(*reads, *twin, "pass by pass");
}

/// What a read answers for a size nothing has placed is where the
/// placement would put it in the read's room; a placement in other room puts
/// it elsewhere, and the read is laid out again: from the next frame on it
/// reads the size where the placement put it, though a sibling lays the row
/// out on every frame and nothing below it changes (#930).
#[cfg(feature = "testing")]
#[test]
fn a_read_after_the_first_placement_reads_where_it_put_the_size() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let reads = Reads::default();
    let recorded = reads.clone();
    let tick = create_signal(1.0f32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .layout(ReadsThenPlaces {
                    room: Some(100.0),
                    ..ReadsThenPlaces::new(&recorded)
                })
                .child(
                    container()
                        .width(at_least(0.0).transition(200.0))
                        .height(10.0)
                        .overflow(Overflow::Hidden)
                        .child(container().width(fill()).height(10.0)),
                )
                .child(container().width(tick).height(10.0))
        },
    );
    app.configure(surface, 400, 20, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    for i in 0..4 {
        tick.set(2.0 + i as f32);
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    let reads = reads.borrow();
    assert_eq!(
        reads.first(),
        Some(&Size::new(0.0, 10.0)),
        "before: {reads:?}"
    );
    assert_eq!(
        reads.last(),
        Some(&Size::new(100.0, 10.0)),
        "after: {reads:?}"
    );
}
