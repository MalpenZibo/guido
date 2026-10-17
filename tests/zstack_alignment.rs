//! A `ZStack` places each child by an alignment: the stack's own, or the one
//! the child declares for itself (#781).
//!
//! The alignment cases are a 100×60 stack, so the free space on each axis is
//! the stack's size less the child's, and an alignment takes a share of it:
//! none at the start, half at the centre, all of it at the end. A child larger than
//! the stack leaves negative free space, and the share of that is an overhang.

use guido::prelude::*;
use guido::tree::WidgetId;
use guido::widgets::Rect;

mod common;
use common::Harness;

fn stack(layout: ZStack, child: Container) -> Container {
    container()
        .width(100.0)
        .height(60.0)
        .layout(layout)
        .child(child)
}

/// Where a 40×20 child of a stack aligned by `alignment` is placed.
fn placed(alignment: Alignment) -> (f32, f32) {
    child_at(stack(
        ZStack::new().align(alignment),
        container().width(40.0).height(20.0),
    ))
}

fn child_at(widget: Container) -> (f32, f32) {
    let h = Harness::laid_out(widget, 400.0, 400.0);
    let child = h.tree.get_children(h.root)[0];
    let Rect { x, y, .. } = h.tree.get_bounds(child).expect("laid out");
    (x, y)
}

/// A child 140 wide at its natural size, which a 100-wide stack does not cut.
fn wider_than_the_stack() -> Container {
    container()
        .width(natural())
        .height(natural())
        .child(container().width(140.0).height(20.0))
}

/// Flutter's `Stack(alignment:)` and SwiftUI's `ZStack(alignment:)` do the
/// same: the overflow is shared as the free space would have been.
#[test]
fn a_child_wider_than_the_stack_overhangs_both_sides_equally() {
    // At the end, all of it past the start.
    for (alignment, at) in [
        (Alignment::CENTER, (-20.0, 20.0)),
        (Alignment::CENTER_END, (-40.0, 20.0)),
    ] {
        assert_eq!(
            child_at(stack(
                ZStack::new().align(alignment),
                wider_than_the_stack(),
            )),
            at,
            "{alignment:?}"
        );
    }
}

/// What every existing stack was written against.
#[test]
fn a_stack_that_declares_nothing_aligns_to_the_top_start() {
    assert_eq!(
        child_at(stack(ZStack::new(), container().width(40.0).height(20.0))),
        (0.0, 0.0)
    );
}

#[test]
fn a_child_that_declares_its_own_alignment_overrides_the_stacks() {
    assert_eq!(
        child_at(stack(
            ZStack::new().align(Alignment::CENTER),
            container()
                .width(40.0)
                .height(20.0)
                .align_self(Alignment::BOTTOM_END),
        )),
        (60.0, 40.0)
    );
}

/// Every one of the nine, so a constant pointing at the wrong corner is seen
/// — the issue's own two among them: `CENTER` at (30, 20), `TOP_END` at
/// (60, 0).
#[test]
fn each_constant_names_its_place() {
    let cases = [
        (Alignment::TOP_START, (0.0, 0.0)),
        (Alignment::TOP_CENTER, (30.0, 0.0)),
        (Alignment::TOP_END, (60.0, 0.0)),
        (Alignment::CENTER_START, (0.0, 20.0)),
        (Alignment::CENTER, (30.0, 20.0)),
        (Alignment::CENTER_END, (60.0, 20.0)),
        (Alignment::BOTTOM_START, (0.0, 40.0)),
        (Alignment::BOTTOM_CENTER, (30.0, 40.0)),
        (Alignment::BOTTOM_END, (60.0, 40.0)),
    ];
    for (alignment, at) in cases {
        assert_eq!(placed(alignment), at, "{alignment:?}");
    }
}

/// A follower is laid out at the stack's size on the axes it fills, so there
/// is nothing to share there and it stays on the stack: aligning the stack
/// moves the leader, never the background that follows it.
#[test]
fn a_follower_keeps_the_stacks_size_and_place() {
    let h = Harness::laid_out(
        container()
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(container().width(fill()).height(fill()))
            .child(container().width(80.0).height(20.0))
            .child(container().width(40.0).height(10.0)),
        400.0,
        400.0,
    );
    let bounds = |i: usize| {
        h.tree
            .get_bounds(h.tree.get_children(h.root)[i])
            .expect("laid out")
    };
    assert_eq!(bounds(0), Rect::new(0.0, 0.0, 80.0, 20.0));
    assert_eq!(bounds(1), Rect::new(0.0, 0.0, 80.0, 20.0));
    assert_eq!(bounds(2), Rect::new(20.0, 5.0, 40.0, 10.0));
}

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");

/// The column from the issue: two labels, so it is as big as they are and
/// no bigger, measured with the vendored font.
fn now_playing() -> Container {
    guido::load_font(FONT.to_vec());
    let label = |s: &'static str, size: f32| {
        text(s)
            .font_size(size)
            .font_family(FontFamily::name("DejaVu Sans Mono"))
    };
    container()
        .layout(Flex::column().spacing(6.0))
        .child(label("Now playing", 16.0))
        .child(label("Blurs out growing", 11.5))
}

/// A 280×110 stage centring whatever it is given: its children are offered
/// up to its size, not held to it, as Flutter's `StackFit.loose`.
const STAGE: Size = Size::new(280.0, 110.0);

fn stage() -> Container {
    container()
        .width(STAGE.width)
        .height(STAGE.height)
        .layout(ZStack::new().align(Alignment::CENTER))
}

/// The stage laid out under a surface's root, as the issue's repro has it,
/// and the stage's id.
fn staged(stage: Container) -> (Harness, WidgetId) {
    let h = Harness::laid_out(container().child(stage), 400.0, 300.0);
    let stage = h.tree.get_children(h.root)[0];
    (h, stage)
}

/// The bounds of the stage's child at `index`.
fn on_stage(stage: Container, index: usize) -> Rect {
    let (h, stage) = staged(stage);
    h.tree
        .get_bounds(h.tree.get_children(stage)[index])
        .expect("laid out")
}

/// The column alone, laid out where nothing holds it to a size.
fn natural_size() -> Size {
    let h = Harness::laid_out(now_playing(), 400.0, 300.0);
    h.tree.cached_size(h.root).expect("laid out")
}

fn centred(size: Size) -> Rect {
    Rect::new(
        (STAGE.width - size.width) / 2.0,
        (STAGE.height - size.height) / 2.0,
        size.width,
        size.height,
    )
}

/// The issue's table: a child sized by its content keeps that size in an
/// exact-size stack, and the alignment has the rest to share.
#[test]
fn a_content_sized_child_keeps_its_size_in_an_exact_stack() {
    let size = natural_size();
    assert!(
        size.width < STAGE.width && size.height < STAGE.height,
        "{size:?}"
    );
    assert_eq!(on_stage(stage().child(now_playing()), 0), centred(size));
}

#[test]
fn a_dynamic_child_keeps_its_size_in_an_exact_stack() {
    assert_eq!(
        on_stage(stage().child(now_playing), 0),
        centred(natural_size())
    );
}

#[test]
fn a_child_beside_an_absent_one_keeps_its_size_in_an_exact_stack() {
    assert_eq!(
        on_stage(
            stage()
                .child(|| None::<Container>)
                .child(|| Some(now_playing())),
            0,
        ),
        centred(natural_size())
    );
}

/// The stack is still the size it was given, and a follower still takes
/// that size: only the leaders are loosened.
#[test]
fn an_exact_stack_keeps_its_size_and_its_followers_fill_it() {
    let (h, stage) = staged(
        stage()
            .child(container().width(fill()).height(fill()))
            .child(now_playing()),
    );
    assert_eq!(h.tree.cached_size(stage), Some(STAGE));
    assert_eq!(
        h.tree.get_bounds(h.tree.get_children(stage)[0]),
        Some(Rect::new(0.0, 0.0, STAGE.width, STAGE.height))
    );
}
