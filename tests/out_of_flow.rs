//! A child out of its parent's flow is laid out, placed, drawn and answers
//! the pointer as any child does, but takes no part in its parent's size
//! (#922). Where it overflows the parent it is still reachable, as GTK's
//! overlay children and Compose's are, and unlike Flutter's `Stack`.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::tree::LayoutCtx;
use guido::widget_prelude::{Constraints, Layout, WidgetId};
use guido::widgets::Rect;

mod common;
use common::Harness;

/// A press on the part of an out-of-flow panel that stands outside the stack
/// holding it reaches the panel.
#[test]
fn a_press_where_an_out_of_flow_child_overflows_reaches_it() {
    let pressed = Rc::new(Cell::new(false));
    let heard = pressed.clone();
    let holder = container()
        .layout(ZStack::new().align(Alignment::TOP_CENTER))
        .child(container().width(100.0).height(30.0))
        .child(
            container()
                .width(300.0)
                .height(200.0)
                .out_of_flow(true)
                .on_click(move || heard.set(true)),
        );
    let mut h = Harness::laid_out(
        container()
            .width(400.0)
            .height(400.0)
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(holder),
        400.0,
        400.0,
    );
    let holder = h.tree.get_children(h.root)[0];
    let Rect { x, y, width, .. } = h.tree.get_bounds(holder).expect("laid out");
    assert_eq!((x, y, width), (150.0, 185.0, 100.0), "the island's box");

    // Left of the island and below it, inside the panel only.
    let (px, py) = (60.0, 300.0);
    h.paint();
    h.click(px, py);
    assert!(
        pressed.get(),
        "the press at ({px}, {py}) did not reach the panel"
    );
}

/// Lays its children out one beside the other and knows nothing of out of
/// flow: what a user writes.
struct Beside;

impl Layout for Beside {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let mut size = Size::zero();
        for &child in children {
            let own = ctx.layout_child(child, constraints).unwrap_or_default();
            ctx.tree()
                .set_origin(child, origin.0 + size.width, origin.1);
            size.width += own.width;
            size.height = size.height.max(own.height);
        }
        size
    }
}

/// A layout written outside the crate leaves a child out of flow out of its
/// size and its sequence without a line for it: the container lays that
/// child out and places it at the top start, the default for a lone child.
#[test]
fn a_user_written_layout_leaves_a_child_out_of_flow_alone() {
    let h = Harness::laid_out(
        container()
            .layout(Beside)
            .child(container().width(40.0).height(20.0))
            .child(container().width(300.0).height(90.0).out_of_flow(true))
            .child(container().width(40.0).height(20.0)),
        400.0,
        400.0,
    );
    let bounds = |h: &Harness, index: usize| {
        let child = h.tree.get_children(h.root)[index];
        h.tree.get_bounds(child).expect("laid out")
    };
    let root = h.tree.get_bounds(h.root).expect("laid out");
    assert_eq!((root.width, root.height), (80.0, 20.0));
    assert_eq!((bounds(&h, 2).x, bounds(&h, 2).y), (40.0, 0.0));
    let out = bounds(&h, 1);
    assert_eq!(
        (out.x, out.y, out.width, out.height),
        (0.0, 0.0, 300.0, 90.0)
    );
}

/// The panel #925 hangs under an island: its top centre on the island's
/// bottom centre.
fn hung_panel() -> Container {
    container()
        .width(300.0)
        .height(200.0)
        .out_of_flow(true)
        .align_self(Alignment::BOTTOM_CENTER.to(Alignment::TOP_CENTER))
}

/// A child out of flow hangs against the outside of its parent's edge: its
/// top on the island's bottom, centred across it.
#[test]
fn an_out_of_flow_child_hangs_from_its_parents_edge() {
    let h = Harness::laid_out(
        container()
            .layout(ZStack::new().align(Alignment::TOP_CENTER))
            .child(container().width(100.0).height(30.0))
            .child(hung_panel()),
        400.0,
        400.0,
    );
    let root = h.tree.get_bounds(h.root).expect("laid out");
    assert_eq!((root.width, root.height), (100.0, 30.0), "the island's box");
    let panel = h.tree.get_children(h.root)[1];
    let Rect { x, y, .. } = h.tree.get_bounds(panel).expect("laid out");
    assert_eq!((x, y), (-100.0, 30.0), "under the island, centred on it");
}

/// The island grows while the panel slides out of it on its own transition:
/// on every frame the panel's box starts where the island's ends, and what
/// the panel's own translate adds only ever grows — the island's layout
/// never restarts it (#925).
#[cfg(feature = "testing")]
#[test]
fn a_hung_child_follows_its_growing_parent_without_restarting_its_own_motion() {
    use std::time::{Duration, Instant};

    let Some(mut app) = common::headless() else {
        return;
    };
    let tall = create_signal(30.0f32);
    let slid = create_signal(Translate::NONE);
    let island = create_widget_ref();
    let panel = create_widget_ref();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(400)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .width(fill())
                .layout(ZStack::new().align(Alignment::TOP_CENTER))
                .child(
                    container()
                        .layout(ZStack::new().align(Alignment::TOP_CENTER))
                        .child(
                            container()
                                .width(100.0)
                                .height(
                                    (move || tall.get())
                                        .transition(Transition::new(400.0, TimingFunction::Linear)),
                                )
                                .widget_ref(island),
                        )
                        .child(
                            hung_panel()
                                .background(Color::WHITE)
                                .translate(
                                    slid.transition(Transition::new(600.0, TimingFunction::Linear)),
                                )
                                .widget_ref(panel),
                        ),
                )
        },
    );
    app.configure(surface, 400, 400, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    tall.set(60.0);
    slid.set(Translate::new(0.0, 8.0));
    let mut frames = Vec::new();
    let start = at;
    while at < start + Duration::from_millis(700) {
        at += Duration::from_millis(16);
        app.step_at(at);
        let island = island.layout_rect().get_untracked();
        let laid = panel.layout_rect().get_untracked();
        let drawn = panel.rect().get_untracked();
        frames.push((
            (at - start).as_millis(),
            island.y + island.height,
            laid.y,
            drawn.y - laid.y,
        ));
    }
    let growing = frames
        .iter()
        .filter(|&&(_, bottom, ..)| bottom > 31.0 && bottom < 59.0)
        .count();
    assert!(growing > 10, "the island grows over the frames: {frames:?}");
    let mut slide = 0.0f32;
    for &(ms, bottom, top, moved) in &frames {
        assert_eq!(
            top, bottom,
            "at {ms} ms the panel's box starts at {top}, the island ends at {bottom}: {frames:?}"
        );
        assert!(
            moved >= slide,
            "at {ms} ms the panel's slide went back from {slide} to {moved}: {frames:?}"
        );
        slide = moved;
    }
    assert_eq!(slide, 8.0, "and the slide finishes: {frames:?}");
}
