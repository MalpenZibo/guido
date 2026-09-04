//! Does the library trip its own "signal read with no reactive scope" warning?
//!
//! That diagnostic is guido's answer to the one mistake the type system cannot
//! catch: reading a signal where nothing can subscribe to it, producing a value
//! that silently never updates. It only does its job if people trust it, and
//! they only trust it if it stays quiet on correct code — a warning that fires
//! on the library's own paths is a warning nobody reads.
//!
//! So: every widget, every reactive property driven by a closure (a *stored*
//! value is exempt from the check by construction, so a static one would prove
//! nothing), put through every method of the `Widget` trait. Any read the
//! library performs outside a tracking scope and outside a `snapshot_zone`
//! shows up here.
//!
//! A failure is not necessarily a missing subscription — it may be a legitimate
//! snapshot that simply never said so. Both are worth fixing, and the fix
//! differs: establish the scope, or mark the region. Re-run with `--nocapture`
//! to see which file and line the diagnostic names.

#![cfg(debug_assertions)]

use crate::animation::{Animate, TimingFunction, Transition};
use crate::layout::{Constraints, Flex};
use crate::reactive::create_signal;
use crate::reactive::diagnostics::report_count;
use crate::renderer::{PaintContext, RenderNode, Shadow};
use crate::tree::Tree;
use crate::widgets::widget::{Event, Key, Modifiers, MouseButton, ScrollSource};
use crate::widgets::{Color, ImageSource, LinearGradient, Overflow, Scroll};
use crate::widgets::{InputStyled, Stateful, Widget, container, image, text, text_input};

/// Put a widget through the whole `Widget` trait and return how many reads the
/// library performed with no reactive scope.
fn diagnostics_from_full_lifecycle(widget: impl Widget + 'static) -> u64 {
    let before = report_count();

    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

    // layout_hints is what a parent asks before laying a child out
    tree.with_widget(root, |w| w.layout_hints());

    let constraints = Constraints::new(0.0, 0.0, 300.0, 300.0);
    tree.with_widget_mut(root, |w, id, t| w.layout(t, id, constraints));

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

    // Animations advance between layout and the next paint
    tree.with_widget_mut(root, |w, id, t| {
        w.advance_animations(t, id);
    });
    tree.with_widget_mut(root, |w, id, t| {
        w.reconcile_children(t, id);
    });

    // Event dispatch runs inside a snapshot zone in the real loop
    // (`render_surface`, lib.rs), so it is wrapped here too. The audit answers
    // "what will a user see in their console", and that question is only
    // meaningful against the call sites the library actually uses.
    crate::reactive::diagnostics::snapshot_zone(|| {
        for event in [
            Event::mouse_enter(10.0, 10.0),
            Event::mouse_move(10.0, 10.0),
            Event::mouse_down(10.0, 10.0, MouseButton::Left),
            Event::mouse_up(10.0, 10.0, MouseButton::Left),
            Event::scroll(10.0, 10.0, 0.0, 10.0, ScrollSource::Wheel),
            Event::KeyDown {
                key: Key::Char('a'),
                modifiers: Modifiers::default(),
            },
            Event::MouseLeave,
        ] {
            tree.with_widget_mut(root, |w, id, t| w.event(t, id, &event));
        }
    });

    // A second pass: the first one seeded caches, this one exercises the
    // steady-state paths (early-outs, paint-cache reuse, target re-sync).
    tree.with_widget_mut(root, |w, id, t| w.layout(t, id, constraints));
    let mut node2 = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node2);
        w.paint(t, id, &mut ctx);
    });

    report_count() - before
}

#[track_caller]
fn assert_quiet(what: &str, count: u64) {
    assert_eq!(
        count, 0,
        "{what} made {count} signal read(s) the diagnostic considers \
         non-reactive. Re-run with `--nocapture` to see the file and line. \
         Either the read needs a tracking scope, or it is a legitimate \
         snapshot and needs to say so with `snapshot_zone`."
    );
}

/// Everything a container can animate or react to, all at once.
#[test]
fn a_fully_reactive_container_is_quiet() {
    let n = create_signal(0.0f32);
    let flag = create_signal(false);
    let t = || Transition::new(200, TimingFunction::EaseOut);

    let widget = container()
        .layout(Flex::row().spacing(move || n.get()))
        .padding((move || n.get()).transition(t()))
        .background((move || if flag.get() { Color::RED } else { Color::BLUE }).transition(t()))
        .border(
            (move || n.get() + 1.0).transition(t()),
            Color::WHITE.transition(t()),
        )
        .corners(
            (move || crate::widgets::Corners::superellipse(n.get() + 4.0, n.get() + 1.0))
                .transition(t()),
        )
        .shadow(
            (move || Shadow::new((0.0, n.get()), n.get() * 2.0, 0.0, Color::BLACK)).transition(t()),
        )
        .width((move || n.get() + 100.0).transition(t()))
        .height((move || n.get() + 100.0).transition(t()))
        .visible(move || !flag.get())
        // All three, not just the one: each reaches its target through its
        // own signal and its own converting derivation, so a tracking
        // regression in `translate` or `scale` would leave this green.
        .rotate((move || n.get()).transition(t()))
        .translate(move || (n.get(), 0.0))
        .scale(move || n.get())
        .gradient(move || {
            Some(LinearGradient::horizontal(
                if flag.get() { Color::RED } else { Color::BLUE },
                Color::WHITE,
            ))
        })
        .backdrop_blur(move || n.get() + 8.0)
        .overflow(move || {
            if flag.get() {
                Overflow::Hidden
            } else {
                Overflow::Visible
            }
        })
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
        .when_focused(|s| s.border(2.0, Color::WHITE))
        .on_click(|| {})
        .child(container().width(20.0).height(20.0));

    assert_quiet(
        "a fully reactive container",
        diagnostics_from_full_lifecycle(widget),
    );
}

#[test]
fn a_scrollable_container_is_quiet() {
    let n = create_signal(0.0f32);
    let widget = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::both())
        .padding(move || n.get())
        .child(container().width(500.0).height(500.0));

    assert_quiet(
        "a scrollable container",
        diagnostics_from_full_lifecycle(widget),
    );
}

#[test]
fn a_container_with_reactive_children_is_quiet() {
    let items = create_signal(vec![1u64, 2, 3]);
    let show = create_signal(true);

    let widget = container()
        .children(crate::widgets::keyed(
            move || items.get(),
            |i| *i,
            |_| container().width(20.0).height(20.0),
        ))
        .child(move || show.get().then(|| container().width(10.0).height(10.0)));

    assert_quiet(
        "a container with reactive children",
        diagnostics_from_full_lifecycle(widget),
    );
}

/// The same properties declared on the text itself rather than above it: the
/// closures have to be read inside the text's own scope too, or a leaf that
/// styles itself would be quietly unreactive.
#[test]
fn a_text_that_styles_itself_is_quiet() {
    let n = create_signal(0.0f32);

    let widget = container().child(
        text("ciao")
            .color(move || {
                if n.get() > 0.0 {
                    Color::RED
                } else {
                    Color::WHITE
                }
            })
            .font_size(move || n.get() + 12.0),
    );

    assert_quiet(
        "a text that declares its own style",
        diagnostics_from_full_lifecycle(widget),
    );
}

/// And the furniture only an input draws, declared on the input.
#[test]
fn an_input_that_styles_itself_is_quiet() {
    let n = create_signal(0.0f32);
    let value = create_signal(String::new());

    let widget = container().child(
        text_input(value)
            .placeholder("prompt")
            .placeholder_color(move || {
                if n.get() > 0.0 {
                    Color::RED
                } else {
                    Color::WHITE
                }
            })
            .cursor_color(move || {
                if n.get() > 0.0 {
                    Color::RED
                } else {
                    Color::WHITE
                }
            }),
    );

    assert_quiet(
        "an input that declares its own caret and placeholder",
        diagnostics_from_full_lifecycle(widget),
    );
}

/// State overrides on a leaf: the control's flags, the focus path and an
/// app-owned condition are all signal reads, and all three have to happen
/// inside the leaf's own scope.
#[test]
fn a_text_with_state_overrides_is_quiet() {
    let wrong = create_signal(false);

    let widget = container().control().on_click(|| {}).child(
        text("ciao")
            .color(Color::WHITE)
            .when_hovered(|s| s.color(Color::RED))
            .when_focused(|s| s.color(Color::GREEN))
            .state(wrong, |s| s.color(Color::BLUE)),
    );

    assert_quiet(
        "a text with state overrides",
        diagnostics_from_full_lifecycle(widget),
    );
}

/// The same with nothing marked above it, which takes the other branch: the
/// text is its own unit and reads its own hover instead of a control's.
#[test]
fn a_text_that_is_its_own_unit_is_quiet() {
    let widget = container().child(
        text("ciao")
            .color(Color::WHITE)
            .when_hovered(|s| s.color(Color::RED)),
    );

    assert_quiet(
        "a text with no control above it",
        diagnostics_from_full_lifecycle(widget),
    );
}

#[test]
fn a_fully_reactive_text_input_is_quiet() {
    let value = create_signal(String::from("ciao"));
    let n = create_signal(0.0f32);

    let widget = container().child(
        text_input(value)
            .color(move || {
                if n.get() > 0.0 {
                    Color::RED
                } else {
                    Color::WHITE
                }
            })
            .cursor_color(move || Color::WHITE.with_alpha(n.get().max(0.5)))
            .font_size(move || n.get() + 12.0),
    );

    assert_quiet(
        "a fully reactive text input",
        diagnostics_from_full_lifecycle(widget),
    );
}

#[test]
fn an_image_is_quiet() {
    // Raw pixels: no file to find, no decoder to reach for.
    let widget = container()
        .width(20.0)
        .height(20.0)
        .child(image(ImageSource::Rgba {
            width: 2,
            height: 2,
            pixels: std::sync::Arc::from(vec![255u8; 2 * 2 * 4].into_boxed_slice()),
        }));

    assert_quiet("an image", diagnostics_from_full_lifecycle(widget));
}
