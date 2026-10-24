//! A child that declares `natural()` on an axis keeps its content's size there
//! whatever its parent offers, and runs past the parent's edge (#782).
//!
//! Every case is a parent narrower than the child's content, which is where
//! the length makes a difference: without it the child is cut to what the
//! parent offers, as an exact length is.

use guido::prelude::*;
use guido::widgets::Rect;

mod common;
use common::Harness;

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");

/// A 160×20 box inside a child sized by `width`.
fn content(width: Length) -> Container {
    container()
        .width(width)
        .child(container().width(160.0).height(20.0))
}

/// The bounds of the root's `index`th child.
fn child(h: &Harness, index: usize) -> Rect {
    at(h, &[index])
}

/// The bounds of the widget `path` leads to from the root, one child index
/// per level.
fn at(h: &Harness, path: &[usize]) -> Rect {
    let id = path
        .iter()
        .fold(h.root, |id, &index| h.tree.get_children(id)[index]);
    h.tree.get_bounds(id).expect("laid out")
}

fn clipping<M>(width: impl IntoAnimated<Length, M>, child: Container) -> Container {
    container()
        .width(width)
        .height(40.0)
        .overflow(Overflow::Hidden)
        .child(child)
}

/// The issue's own: 160 inside 100 with `natural()`, and 100 without it.
#[test]
fn a_natural_child_keeps_its_width_inside_a_narrower_clipping_parent() {
    let h = Harness::laid_out(clipping(100.0, content(natural())), 400.0, 400.0);
    assert_eq!(child(&h, 0).width, 160.0);

    let h = Harness::laid_out(clipping(100.0, content(Length::default())), 400.0, 400.0);
    assert_eq!(child(&h, 0).width, 100.0, "an undeclared length is cut");
}

/// The parent stepping down from 200 to 80, frame by frame, past the content's
/// width: the child is laid out 160 wide at every one of them.
#[test]
fn a_shrinking_parent_never_changes_a_natural_childs_width() {
    let width = create_signal(200.0);
    let mut h = Harness::laid_out(clipping(width, content(natural())), 400.0, 400.0);
    let mut widths = Vec::new();
    for w in (80..=200).rev().step_by(10) {
        width.set(w as f32);
        h.tree.mark_needs_layout(h.root);
        h.lay_out(400.0, 400.0);
        widths.push(child(&h, 0).width);
    }
    assert!(widths.iter().all(|&w| w == 160.0), "{widths:?}");
}

/// A bar across the top, for the cases that need the real loop.
#[cfg(feature = "testing")]
fn bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(100)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
}

/// The same shrink as a transition, through the real loop: the children are
/// laid out against the width in flight rather than the declared one.
#[cfg(feature = "testing")]
#[test]
fn a_parent_in_flight_never_changes_a_natural_childs_width() {
    use std::time::{Duration, Instant};
    let Some(mut app) = common::headless() else {
        return;
    };
    let (parent, inner) = (create_widget_ref(), create_widget_ref());
    let narrow = create_signal(false);
    let surface = app.surface(bar(), move || {
        container().child(
            clipping(
                (move || if narrow.get() { 80.0 } else { 200.0 }).transition(160.0),
                content(natural()).widget_ref(inner),
            )
            .widget_ref(parent),
        )
    });
    app.configure(surface, 400, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    assert_eq!(inner.layout_rect().get_untracked().width, 160.0);

    narrow.set(true);
    let mut frames = Vec::new();
    for _ in 0..20 {
        at += Duration::from_millis(16);
        app.step_at(at);
        frames.push((
            parent.layout_rect().get_untracked().width,
            inner.layout_rect().get_untracked().width,
        ));
    }
    assert!(
        frames
            .iter()
            .any(|&(parent, _)| parent > 80.0 && parent < 200.0),
        "the parent was in flight: {frames:?}"
    );
    assert_eq!(frames.last().map(|f| f.0), Some(80.0), "and settled");
    assert!(
        frames.iter().all(|&(_, child)| child == 160.0),
        "{frames:?}"
    );
}

/// A label measured with no maximum neither wraps nor ellipsizes: it is one
/// line, as wide as it is on a surface with room for it.
#[test]
fn a_natural_label_stays_on_one_line() {
    let label = || mono("a label longer than its parent");
    let free = Harness::laid_out(container().child(label()), 1000.0, 400.0);
    let one_line = child(&free, 0);

    let h = Harness::laid_out(
        clipping(
            60.0,
            container()
                .width(natural())
                .height(natural())
                .child(label()),
        ),
        400.0,
        400.0,
    );
    let laid = at(&h, &[0, 0]);
    assert_eq!((laid.width, laid.height), (one_line.width, one_line.height));
}

/// Its own bounds still hold it: the content's size, up to the maximum it
/// declares, and at least the minimum.
#[test]
fn a_natural_length_keeps_its_own_bounds() {
    for (length, width) in [
        (natural().at_most(120.0), 120.0),
        (natural().at_least(200.0), 200.0),
    ] {
        let h = Harness::laid_out(clipping(100.0, content(length)), 400.0, 400.0);
        assert_eq!(child(&h, 0).width, width, "{length:?}");
    }
}

/// A parent that stretches its children does not stretch a natural one,
/// which is the content's size and no more, as an exact length is.
#[test]
fn a_natural_child_is_not_raised_to_a_stretching_parent() {
    let h = Harness::laid_out(
        container()
            .width(300.0)
            .layout(Flex::column())
            .child(content(natural())),
        400.0,
        400.0,
    );
    assert_eq!(child(&h, 0).width, 160.0);
}

/// A label in the vendored face, so its width is the same on every machine.
fn mono(content: &'static str) -> Text {
    guido::load_font(FONT.to_vec());
    text(content)
        .font_size(14.0)
        .font_family(FontFamily::name("DejaVu Sans Mono"))
}

fn ellipsized(content: &'static str) -> Text {
    mono(content).nowrap().overflow(TextOverflow::Ellipsis)
}

/// A row too short for its children takes the overflow from those that can
/// give it — a label that ellipsizes can — but a natural child is held at its
/// size like an exact one, whatever it holds.
#[test]
fn a_row_too_short_holds_a_natural_child_at_its_size() {
    let row = |first: Length| {
        Harness::laid_out(
            container()
                .width(200.0)
                .overflow(Overflow::Hidden)
                .layout(Flex::row())
                .child(
                    container()
                        .width(first)
                        .child(ellipsized("held at its size")),
                )
                .child(ellipsized("gives the room up")),
            400.0,
            400.0,
        )
    };
    let free = Harness::laid_out(
        container().child(ellipsized("held at its size")),
        1000.0,
        400.0,
    );
    let whole = child(&free, 0).width;

    let held = row(natural());
    assert_eq!(child(&held, 0).width, whole);
    assert!(
        child(&held, 1).width < 200.0 - whole,
        "{:?}",
        child(&held, 1)
    );

    let shared = row(Length::default());
    assert!(
        child(&shared, 0).width < whole,
        "without it, the first gives room up too: {:?}",
        child(&shared, 0)
    );
}

/// The parent's size is its own: a natural child running past it does not
/// widen it, clipped or not.
#[test]
fn a_natural_child_does_not_widen_its_parent() {
    for overflow in [Overflow::Hidden, Overflow::Visible] {
        let h = Harness::laid_out(
            container()
                .width(100.0)
                .overflow(overflow)
                .child(content(natural())),
            400.0,
            400.0,
        );
        let root = h.tree.get_bounds(h.root).expect("laid out");
        assert_eq!(root.width, 100.0, "{overflow:?}");
        assert_eq!(child(&h, 0).width, 160.0, "{overflow:?}");
    }
}

/// Offered a tight box, a natural child is still the size of what it holds,
/// so a change inside it has to reach the parent that places it: the stack
/// centres it again at its new width. Through the real loop, because what is
/// watched is how far the change travels up the tree.
#[cfg(feature = "testing")]
#[test]
fn a_natural_child_that_changes_is_placed_again() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let placed = create_widget_ref();
    let wide = create_signal(160.0);
    let surface = app.surface(bar(), move || {
        container()
            .width(100.0)
            .height(60.0)
            .overflow(Overflow::Hidden)
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(
                container()
                    .width(natural())
                    .height(natural())
                    .widget_ref(placed)
                    .child(container().width(wide).height(20.0)),
            )
    });
    app.configure(surface, 400, 100, 1.0);
    app.step();
    assert_eq!(
        placed.layout_rect().get_untracked(),
        Rect::new(-30.0, 20.0, 160.0, 20.0)
    );
    wide.set(120.0);
    app.step();
    assert_eq!(
        placed.layout_rect().get_untracked(),
        Rect::new(-10.0, 20.0, 120.0, 20.0)
    );
}

/// The two ends a bound moves between.
#[cfg(feature = "testing")]
const NARROW: f32 = 60.0;
#[cfg(feature = "testing")]
const WIDE: f32 = 400.0;

/// The bound at one end, eased to the other: a natural width's laid-out size
/// at every 16 ms frame for 480 ms after it moves to `WIDE` if `widen`, or to
/// `NARROW` otherwise, through the real loop. Nothing on a machine with no
/// adapter.
#[cfg(feature = "testing")]
fn eased_bound(
    bound: fn(f32) -> Length,
    content_width: f32,
    widen: bool,
    transition: Transition,
) -> Option<Vec<f32>> {
    use std::time::{Duration, Instant};
    let mut app = common::headless()?;
    let placed = create_widget_ref();
    let wide = create_signal(!widen);
    let surface = app.surface(bar(), move || {
        container().child(
            container()
                .height(40.0)
                .width(
                    (move || bound(if wide.get() { WIDE } else { NARROW }))
                        .transition(transition.clone()),
                )
                .widget_ref(placed)
                .child(container().width(content_width).height(40.0)),
        )
    });
    app.configure(surface, 800, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    wide.set(widen);
    let frames = (0..30)
        .map(|_| {
            at += Duration::from_millis(16);
            app.step_at(at);
            placed.layout_rect().get_untracked().width
        })
        .collect();
    Some(frames)
}

#[cfg(feature = "testing")]
fn ease_out() -> Transition {
    Transition::new(400.0, TimingFunction::EaseOut)
}

/// A bound is still a bound in flight: a spring that overshoots a capped
/// width on its way up to the cap stops at the cap, rather than running past
/// it for the frames its wobble lasts. On its way down there is no floor, and
/// it wobbles below the cap as an exact width wobbles below its value.
#[cfg(feature = "testing")]
#[test]
fn a_spring_does_not_carry_a_natural_width_past_its_cap() {
    for widen in [false, true] {
        let Some(frames) = eased_bound(
            |cap| natural().at_most(cap),
            WIDE,
            widen,
            Transition::spring(SpringConfig::BOUNCY),
        ) else {
            return;
        };
        assert!(
            frames.iter().all(|&w| w <= WIDE),
            "widen {widen}: {frames:?}"
        );
    }
}

/// Whether `frames` travel to the end `widen` names through the widths
/// between, rather than arriving there on the first frame.
#[cfg(feature = "testing")]
fn eases(frames: &[f32], widen: bool) -> bool {
    let to = if widen { WIDE } else { NARROW };
    frames.first().is_some_and(|&w| w != to)
        && frames.iter().filter(|&&w| w > NARROW && w < WIDE).count() > 5
        && frames.last() == Some(&to)
}

/// A capped natural width eases to a new cap as an exact one does, both
/// ways — the shrink is the one that snapped (#884), because the cap was
/// applied to the width in flight as well as to where it was heading.
#[cfg(feature = "testing")]
#[test]
fn a_capped_natural_width_eases_to_its_new_cap() {
    for widen in [false, true] {
        let Some(frames) = eased_bound(|cap| natural().at_most(cap), WIDE, widen, ease_out())
        else {
            return;
        };
        assert!(eases(&frames, widen), "widen {widen}: {frames:?}");
    }
}

/// And its mirror: a floor raised past the content eases up to it, rather
/// than lifting the width there on the first frame.
#[cfg(feature = "testing")]
#[test]
fn a_floored_natural_width_eases_to_its_new_floor() {
    for widen in [true, false] {
        let Some(frames) = eased_bound(|floor| natural().at_least(floor), 40.0, widen, ease_out())
        else {
            return;
        };
        assert!(eases(&frames, widen), "widen {widen}: {frames:?}");
    }
}
