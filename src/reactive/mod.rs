pub mod callback;
pub mod clipboard;
pub mod context;
pub mod cursor;
pub mod diagnostics;
pub mod effect;
pub mod focus;
pub(crate) mod global;
pub(crate) mod guard;
pub mod into_signal;
pub mod invalidation;
pub mod memo;
pub mod owner;
pub mod runtime;
pub mod service;
pub mod signal;
pub mod storage;
mod trigger;

pub(crate) use clipboard::{
    clear_system_clipboard, set_system_clipboard, set_system_primary, take_clipboard_change,
    take_primary_change,
};
pub use clipboard::{
    clipboard_copy, clipboard_has_content, clipboard_paste, primary_copy, primary_paste,
};
pub use context::{
    expect_context, has_context, provide_context, provide_signal_context, use_context, with_context,
};
pub(crate) use cursor::take_cursor_change;
pub use cursor::{CursorIcon, set_cursor};
pub use effect::create_effect;
pub(crate) use focus::{has_focus, release_focus, release_focus_if_within, request_focus};
pub(crate) use into_signal::converts;
#[doc(hidden)]
pub use into_signal::{
    ClosureMarker, ConvertedSignalMarker, LossyMarker, MemoMarker, RwSignalMarker, SignalMarker,
    ValueMarker,
};
pub use into_signal::{IntoSignal, IntoVal};
// The scope a widget opens around its own signal reads. Public because a
// widget written outside the crate needs it: without one, its reads attribute
// to the nearest ancestor that opened a scope — its parent container — so a
// change to its content re-lays-out every one of its siblings. Reactive, but
// imprecise, and silently so.
pub use crate::jobs::JobType;
pub use invalidation::with_signal_tracking;
pub use memo::{Memo, create_memo};
// with_owner and OwnerId are internal and automatically used by the
// dynamic children system; the public dispose_owner is deferred (safe to
// call from anywhere), the synchronous engine stays crate-internal.
pub(crate) use owner::{OwnerId, create_root_owner, dispose_owner_now, under_owner, with_owner};
pub use owner::{dispose_owner, on_cleanup};
pub use trigger::{Trigger, create_trigger};

/// Internal module for macro support. NOT PART OF PUBLIC API.
/// Do not use directly - these are re-exported for the proc macros, and for the
/// doc tests of `context`, which need a scope to declare a value in and have no
/// public way to open one.
#[doc(hidden)]
pub mod __internal {
    pub use super::owner::{OwnerId, dispose_owner_now as dispose_owner, with_owner};
    pub use super::runtime::batch;
}
pub use callback::Callback;
pub(crate) use runtime::flush_bg_writes;
pub use service::{Service, ServiceContext, create_service, create_task};
pub use signal::{
    OptionSignalExt, RwSignal, Signal, WriteSignal, create_derived, create_signal, create_stored,
};

/// Reset all reactive system state.
///
/// Called during `App::drop()` to wipe all thread-local reactive state,
/// enabling clean restart of the application.
pub(crate) fn reset_reactive() {
    owner::reset_owners();
    // Before `reset_storage`, with the same reason the focus release below has:
    // these hold ids into the arena that is about to be replaced.
    global::reset_globals();
    runtime::reset_runtime();
    storage::reset_storage();
    invalidation::reset_invalidation();
    clipboard::reset_clipboard();
    cursor::reset_cursor();
    diagnostics::reset();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `App::drop` is the only caller, and until now nothing said what it is
    /// for: the next `App` on this thread starts on the arena the last one
    /// left, and an id kept across that names a signal somebody else now owns.
    ///
    /// Named by the mutation job on #220's diff — deleting the body of
    /// `reset_reactive` changed nothing any test could see.
    #[test]
    fn resetting_leaves_nothing_for_the_next_app_to_inherit() {
        owner::create_root_owner();
        let _signal = create_signal(1u32);
        provide_context(7u32);
        assert!(
            storage::live_signal_count() > 0,
            "the App under test has state to wipe"
        );

        reset_reactive();

        assert_eq!(
            storage::live_signal_count(),
            0,
            "a signal from the last App is still in the arena the next one allocates from"
        );
        assert!(
            owner::current_owner().is_none(),
            "the next App would file its setup under a scope that no longer exists"
        );

        owner::create_root_owner();
        assert_eq!(
            use_context::<u32>(),
            None,
            "the next App would read the last one's declarations"
        );
    }
}

#[cfg(test)]
mod bench {
    use std::time::{Duration, Instant};

    use super::*;

    /// Best of five: the machine is noisy enough that a single round says
    /// nothing about the one before it.
    fn best_of_five(mut round: impl FnMut() -> Duration) -> Duration {
        (0..5).map(|_| round()).min().expect("five rounds")
    }

    /// What an effect's re-run costs now that it enters the scope it was
    /// created in: a signal written a million times, with one effect reading
    /// it, which is a million scheduled re-runs.
    ///
    /// Twice, because `under_scope` declines to enter a scope already current:
    /// an effect created where the flush runs pays a compare, and one created
    /// in a scope of its own — a row of a list, which is the case this exists
    /// for — pays the swap.
    ///
    /// `cargo test --release --lib bench:: -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn an_effect_rerun() {
        let rounds = 1_000_000u32;
        let time_a_million_reruns = |in_a_scope_of_its_own: bool| -> Duration {
            owner::create_root_owner();
            let source = create_signal(0u32);
            let seen = std::rc::Rc::new(std::cell::Cell::new(0u32));
            let recorder = std::rc::Rc::clone(&seen);

            let effect = move || {
                let recorder = std::rc::Rc::clone(&recorder);
                create_effect(move || recorder.set(source.get()));
            };
            if in_a_scope_of_its_own {
                owner::with_owner(effect);
            } else {
                effect();
            }

            let best = best_of_five(|| {
                let started = Instant::now();
                for round in 0..rounds {
                    source.set(round);
                }
                started.elapsed()
            });
            assert_eq!(seen.get(), rounds - 1, "the effect ran");
            best
        };

        let where_the_flush_is = time_a_million_reruns(false);
        let in_a_scope_of_its_own = time_a_million_reruns(true);
        println!(
            "{rounds} effect re-runs: {where_the_flush_is:?} for an effect of the \
             flush's own scope, {in_a_scope_of_its_own:?} for one in a scope below it"
        );
    }

    /// A frame of a list, which is where the per-read number lands: three
    /// hundred rows of four reactive properties each, laid out and painted the
    /// way `render_surface` does, with a signal moved between frames so the
    /// work is real rather than cached.
    ///
    /// `cargo test --release --lib bench:: -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn a_frame_of_three_hundred_rows() {
        use crate::layout::Constraints;
        use crate::renderer::{PaintContext, RenderNode};
        use crate::tree::Tree;
        use crate::widgets::widget::Color;
        use crate::widgets::{Widget, container};

        owner::create_root_owner();
        let width = create_signal(40.0f32);

        let rows: Vec<_> = (0..300)
            .map(|row| {
                container()
                    .width(move || width.get() + row as f32 % 3.0)
                    .height(move || 12.0 + row as f32 % 2.0)
                    .background(move || Color::rgb(0.1 + width.get() / 400.0, 0.2, 0.3))
                    .padding(move || width.get() / 20.0)
            })
            .collect();

        let mut tree = Tree::new();
        let root = tree.register(Box::new(container().children(rows)) as Box<dyn Widget>);
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let frames = 200u32;
        let best = best_of_five(|| {
            let started = Instant::now();
            for frame in 0..frames {
                width.set(40.0 + (frame % 7) as f32);
                tree.with_widget_mut(root, |w, id, t| {
                    w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 4000.0));
                });
                let mut node = RenderNode::new(root.as_u64());
                tree.with_widget_mut(root, |w, id, t| {
                    let mut ctx = PaintContext::new(&mut node);
                    w.paint(t, id, &mut ctx);
                });
                std::hint::black_box(&node);
            }
            started.elapsed() / frames
        });

        println!("a frame of 300 rows: {best:?}");
    }

    /// Not a test: the number for #335, which is what a derived read costs
    /// now that it enters the scope it was written in.
    /// `cargo test --release --lib bench:: -- --ignored --nocapture`
    ///
    /// Best of five in one process: the machine is noisy enough that two
    /// separate runs say nothing about each other.
    #[test]
    #[ignore]
    fn a_derived_read() {
        owner::create_root_owner();
        let source = create_signal(1u32);
        let derived = create_derived(move || source.get() + 1);

        let rounds = 1_000_000u32;
        let best = |derived: Signal<u32>| -> Duration {
            best_of_five(|| {
                let started = Instant::now();
                for _ in 0..rounds {
                    std::hint::black_box(derived.get());
                }
                started.elapsed()
            })
        };

        // Warm everything: the closure, the subscription bookkeeping, the caches.
        for _ in 0..100_000 {
            std::hint::black_box(derived.get());
        }

        // Read from the scope the closure was written in — every read during
        // layout, where `OwnedWidget::layout` has already entered the row's
        // scope — and then from another scope, which is every read during
        // paint, where nothing has.
        let from_its_own_scope = best(derived);
        let (from_elsewhere, elsewhere) = owner::with_owner(|| best(derived));
        owner::dispose_owner_now(elsewhere);

        // Reported as the whole loop as well as the quotient: the per-read
        // figure is a whole number of nanoseconds against an effect of one or
        // two, so on its own it can only bound the cost.
        println!(
            "{rounds} derived reads: {from_its_own_scope:?} from their own \
             scope, {from_elsewhere:?} from another"
        );
    }
}
