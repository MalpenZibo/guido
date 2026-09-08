//! Values declared for a scope, read by the scopes below it.
//!
//! A value is keyed by its type and lives on the reactive scope that declared
//! it — the application's setup, a surface, a popup, a dynamic list. A read
//! walks from the current scope upward and takes the nearest declaration, so an
//! inner one shadows an outer one for its own subtree and leaves the rest of the
//! tree alone. Leptos, SolidJS, Floem and Dioxus all resolve context this way,
//! and #220 is why we do now: in one process-wide bag, two declarations of a
//! type overwrote each other silently, and a value declared inside a popup went
//! on being found after the popup was gone.
//!
//! ## Where a read resolves
//!
//! In the body of the factory that builds the widgets — that is where the scope
//! the value was declared for is the scope that is current, so it is where the
//! library knows who is asking.
//!
//! An event handler, a property closure and a spawned task open no scope of
//! their own, so a read inside one resolves against whatever is current. For a
//! handler that is the root — `App::run` enters the root scope before the setup
//! closure and never leaves it — so the application's declarations are readable
//! from one and a surface's are not. For a property closure it depends on when
//! the closure is read: laying out a dynamic list re-enters that row's scope,
//! painting does not, so the same read can resolve against two different scopes
//! on two phases of one frame.
//!
//! Read the value once in the factory body and capture the handle into the
//! closure, and none of that arises:
//!
//! ```
//! # use guido::prelude::*;
//! # use guido::reactive::__internal::with_owner;
//! # #[derive(Clone, Copy, PartialEq, Default)]
//! # struct Theme { bg: Color }
//! # let (_, _scope) = with_owner(|| {
//! # provide_signal_context(Theme::default());
//! fn themed_box() -> Container {
//!     let theme = expect_context::<RwSignal<Theme>>();   // here: a scope is current
//!     container().background(move || theme.get().bg)     // not here: the handle is captured
//! }
//! # themed_box();
//! # });
//! ```
//!
//! ## The scopes are one level deep
//!
//! A surface, a popup and a surface spawned at runtime are all children of the
//! root: the loop opens each one's scope from the root rather than from the
//! surface it belongs to. So a popup does not read what the surface that opened
//! it declared — only what the application did — and two surfaces are siblings
//! that cannot see each other's declarations.
//!
//! ## A declaration belongs to a scope, not to a widget
//!
//! Scopes branch where guido invokes a closure: a surface, a popup, a dynamic
//! list. A widget factory is an ordinary function call the caller makes, so a
//! [`provide_context`] written inside one declares its value for the
//! *surrounding* scope — the whole surface, not the subtree being built. Per
//! container declarations are not part of this.
//!
//! ## Storage
//!
//! A `Vec<(TypeId, Rc<dyn Any>)>` per scope, scanned linearly: a scope declares
//! a handful at most, a `TypeId` comparison is two words, and a miss walks the
//! two or three parents between a widget and the root.

use std::any::{TypeId, type_name};
use std::rc::Rc;

use super::owner::{nearest_declaration, with_scope_declarations};
use super::signal::{RwSignal, create_signal};

/// Declare a value for the current scope, keyed by its type.
///
/// Every scope below this one reads it, until one of them declares the same
/// type and shadows it.
///
/// # Panics
///
/// If no scope is current at all, or if this scope already declares the type —
/// two of a type is what shadowing is for, and shadowing needs two scopes.
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # #[derive(Clone, PartialEq)]
/// # struct MyConfig { warn_threshold: u32 }
/// # let (_, _scope) = with_owner(|| {
/// // In `App::run`'s setup, or in a surface's factory:
/// provide_context(MyConfig { warn_threshold: 80 });
/// assert_eq!(expect_context::<MyConfig>().warn_threshold, 80);
/// # });
/// ```
pub fn provide_context<T: 'static>(value: T) {
    let type_id = TypeId::of::<T>();
    let declared = with_scope_declarations(move |declarations| {
        assert!(
            !declarations
                .iter()
                .any(|(declared, _)| *declared == type_id),
            "`{}` is already declared in this scope.\n\
             A scope below this one shadows it, which is how two of a type \
             coexist; a second declaration here would replace the first with \
             nothing to say it had.",
            type_name::<T>()
        );
        declarations.push((type_id, Rc::new(value)));
    });
    assert!(
        declared.is_some(),
        "`{}` was declared where no scope is current.\n\
         A value belongs to a scope — the application's setup, a surface, a \
         popup, a dynamic list — and an event handler, a property closure or a \
         spawned task is none of them. Declare it where the tree is built.",
        type_name::<T>()
    );
}

/// Read the nearest declaration of `T`, or `None` if there is none.
///
/// Clones the value — for a large struct use [`with_context`] to borrow it, or
/// declare a signal and clone the handle instead.
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # #[derive(Clone, PartialEq)]
/// # struct MyConfig { warn_threshold: u32 }
/// # let (_, _scope) = with_owner(|| {
/// provide_context(MyConfig { warn_threshold: 80 });
/// if let Some(cfg) = use_context::<MyConfig>() {
///     assert_eq!(cfg.warn_threshold, 80);
/// }
/// assert!(use_context::<u32>().is_none());
/// # });
/// ```
pub fn use_context<T: Clone + 'static>() -> Option<T> {
    with_context::<T, T>(Clone::clone)
}

/// Read the nearest declaration of `T`, panicking if there is none.
///
/// Use it where the value is required and its absence is a programming error.
///
/// # Panics
///
/// Naming the type, and both of the reasons it can be missing: nothing declared
/// it, or the read happened where no scope is current.
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # #[derive(Clone, PartialEq, Debug)]
/// # struct MyConfig { warn_threshold: u32 }
/// # let (_, _scope) = with_owner(|| {
/// provide_context(MyConfig { warn_threshold: 80 });
/// let cfg = expect_context::<MyConfig>();
/// assert_eq!(cfg.warn_threshold, 80);
/// # });
/// ```
pub fn expect_context<T: Clone + 'static>() -> T {
    use_context::<T>().unwrap_or_else(|| {
        panic!(
            "No value of type `{}` is declared in this scope or any above it.\n\
             Either nothing declared it, or the read happened outside the scope \
             it was declared for — an event handler, a property closure and a \
             spawned task read against the root. Read it in the factory body and \
             capture the value into the closure.",
            type_name::<T>()
        )
    })
}

/// Borrow the nearest declaration of `T` without cloning it.
///
/// `None` if there is none. Use it for a large struct when one field is all the
/// reader needs.
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # struct Config { warn_threshold: u32 }
/// # let (_, _scope) = with_owner(|| {
/// provide_context(Config { warn_threshold: 80 });
/// let threshold = with_context::<Config, _>(|cfg| cfg.warn_threshold);
/// assert_eq!(threshold, Some(80));
/// # });
/// ```
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R> {
    let value = nearest_declaration(TypeId::of::<T>())?;
    let value = value
        .downcast_ref::<T>()
        .expect("a declaration is keyed by the type it holds");
    Some(f(value))
}

/// Whether any scope at or above this one declares `T`.
///
/// For an optional feature: "if a logger was declared, log through it".
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # #[derive(Clone, PartialEq)]
/// # struct Logger;
/// # let (_, _scope) = with_owner(|| {
/// assert!(!has_context::<Logger>());
/// provide_context(Logger);
/// assert!(has_context::<Logger>());
/// # });
/// ```
pub fn has_context<T: 'static>() -> bool {
    nearest_declaration(TypeId::of::<T>()).is_some()
}

/// Declare an `RwSignal<T>` holding `value`, and hand it back.
///
/// The pattern for mutable shared state: the signal is declared for the scope,
/// and a widget that reads it during paint or layout tracks it, so a write
/// anywhere repaints everything that reads it.
///
/// What is declared is an `RwSignal<T>` — that is the type to ask for, and
/// asking for a `Signal<T>` finds nothing, because a different type is a
/// different key.
///
/// # Example
///
/// ```
/// # use guido::prelude::*;
/// # use guido::reactive::__internal::with_owner;
/// # #[derive(Clone, Copy, PartialEq, Debug, Default)]
/// # struct Theme { bg: f32 }
/// # let (_, _scope) = with_owner(|| {
/// let theme = provide_signal_context(Theme::default());
///
/// // In any factory below it:
/// let read_back = expect_context::<RwSignal<Theme>>();
/// theme.set(Theme { bg: 1.0 });
/// assert_eq!(read_back.get().bg, 1.0);
/// # });
/// ```
pub fn provide_signal_context<T: Clone + PartialEq + Send + 'static>(value: T) -> RwSignal<T> {
    let signal = create_signal(value);
    provide_context(signal);
    signal
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::layout::Constraints;
    use crate::reactive::owner::{create_root_owner, dispose_owner_now, under_owner, with_owner};
    use crate::reactive::signal::create_signal;
    use crate::tree::Tree;
    use crate::widgets::container;
    use crate::widgets::widget::{Event, MouseButton};

    /// Run `f` in a scope below the current one, and take the scope away
    /// again — what a surface, a popup or a row of a list is, for a test.
    fn in_a_child_scope<T>(f: impl FnOnce() -> T) -> T {
        let (value, scope) = with_owner(f);
        dispose_owner_now(scope);
        value
    }

    /// The half that already worked: a scope reads what a scope above it
    /// declared, and the value it gets back is the one that was put there.
    #[test]
    fn a_scope_reads_what_an_ancestor_declared() {
        create_root_owner();
        provide_context(7u32);

        assert_eq!(in_a_child_scope(use_context::<u32>), Some(7));
    }

    /// A declaration belongs to the scope that made it. A sibling is not below
    /// it, so a sibling cannot see it — which is the whole difference between a
    /// scoped store and a bag keyed by type.
    #[test]
    fn a_sibling_scope_does_not_see_the_declaration() {
        create_root_owner();

        let ((), declaring) = with_owner(|| provide_context(42u32));
        let (seen, reading) = with_owner(use_context::<u32>);

        assert_eq!(
            seen, None,
            "a value declared beside this scope is not above it"
        );
        dispose_owner_now(declaring);
        dispose_owner_now(reading);
    }

    /// Shadowing, which is what makes two providers of one type safe: the inner
    /// declaration wins below itself and leaves everything else alone.
    #[test]
    fn an_inner_declaration_shadows_an_outer_one_for_its_own_scope() {
        create_root_owner();
        provide_context(1u32);

        let inner = in_a_child_scope(|| {
            provide_context(2u32);
            use_context::<u32>()
        });

        assert_eq!(inner, Some(2), "the nearest declaration wins");
        assert_eq!(
            use_context::<u32>(),
            Some(1),
            "and the outer scope is untouched by it"
        );
    }

    /// Defect 2 of #220: two scopes each declaring their own `RwSignal` used to
    /// overwrite one another in a process-wide bag, last writer winning. Both
    /// declarations happen before either read, which is what the old store
    /// could not survive.
    #[test]
    fn two_scopes_declaring_one_type_each_read_their_own() {
        create_root_owner();

        let ((), first) = with_owner(|| provide_context(create_signal(1i32)));
        let ((), second) = with_owner(|| provide_context(create_signal(2i32)));

        let from_first = under_owner(first, || expect_context::<RwSignal<i32>>().get());
        let from_second = under_owner(second, || expect_context::<RwSignal<i32>>().get());

        assert_eq!((from_first, from_second), (1, 2));
        dispose_owner_now(first);
        dispose_owner_now(second);
    }

    /// Defect 3 of #220: a signal declared in a scope that dies used to leave a
    /// live-looking handle in the bag — `use_context` answered `Some`, and the
    /// `.get()` after it panicked with "signal was disposed". The declaration
    /// now dies with the scope, so the answer is `None` and there is nothing to
    /// call `.get()` on.
    #[test]
    fn a_declaration_dies_with_the_scope_that_made_it() {
        create_root_owner();

        let ((), popup) = with_owner(|| provide_context(create_signal(1i32)));
        dispose_owner_now(popup);

        assert!(
            use_context::<RwSignal<i32>>().is_none(),
            "the handle went with the scope rather than outliving it"
        );
    }

    /// A declaration needs somewhere to live. With no scope current — an event
    /// handler, a property closure, a spawned task — there is none, and the
    /// value would go nowhere rather than everywhere.
    #[test]
    #[should_panic(expected = "no scope is current")]
    fn declaring_with_no_scope_current_panics() {
        provide_context(1u32);
    }

    /// Two declarations of one type in one scope is the overwrite that defect 2
    /// was about, minus the distance that made it invisible. Shadowing needs two
    /// scopes; this is one, so it is a mistake and says so.
    #[test]
    #[should_panic(expected = "already declared in this scope")]
    fn declaring_one_type_twice_in_a_scope_panics() {
        create_root_owner();
        provide_context(1u32);
        provide_context(2u32);
    }

    /// Where a read resolves, stated as a test rather than as a promise in the
    /// docs.
    ///
    /// An event handler runs with no scope of its own — `OwnedWidget::event`
    /// declines to re-enter one, deliberately — so it reads against whatever is
    /// current, and in a running application that is the root: `App::run`
    /// creates the root scope before the setup closure and never leaves it. So
    /// the application's own declarations are readable from a handler, and a
    /// surface's are not. The rule for a caller is the same either way: read it
    /// in the factory body and capture the value.
    ///
    /// It builds a tree because that is the only way to make the claim about
    /// handlers rather than about closures in general, and it lives here
    /// because the rule is this module's and only a test inside the crate can
    /// open a scope.
    #[test]
    fn an_event_handler_reads_against_the_root_and_not_against_the_surface() {
        create_root_owner();
        provide_context(42u32);

        let seen = Rc::new(Cell::new(None));
        let from_root = Rc::clone(&seen);
        let from_surface = Rc::clone(&seen);

        // The surface's scope, declaring something of its own.
        let (widget, surface) = with_owner(move || {
            provide_context("the surface's".to_string());
            assert_eq!(
                use_context::<String>().as_deref(),
                Some("the surface's"),
                "the factory body is where a read resolves"
            );
            container().width(80.0).height(20.0).on_click(move || {
                from_root.set(Some((use_context::<u32>(), use_context::<String>())));
            })
        });

        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, tree| w.register_children(tree, id));
        tree.with_widget_mut(root, |w, id, tree| {
            w.layout(tree, id, Constraints::new(0.0, 0.0, 200.0, 100.0));
        });
        for event in [
            Event::mouse_down(10.0, 10.0, MouseButton::Left),
            Event::mouse_up(10.0, 10.0, MouseButton::Left),
        ] {
            tree.with_widget_mut(root, |w, id, tree| w.event(tree, id, &event));
        }

        assert_eq!(
            seen.take(),
            Some((Some(42), None)),
            "the handler read against the root: the application's value is \
             there, the surface's is not"
        );
        drop(from_surface);
        dispose_owner_now(surface);
    }

    /// Borrowing rather than cloning, over the scope chain like every other
    /// read.
    #[test]
    fn a_borrowed_read_walks_the_same_chain() {
        create_root_owner();
        provide_context(vec![1, 2, 3]);

        let sum = in_a_child_scope(|| with_context::<Vec<i32>, _>(|v| v.iter().sum::<i32>()));
        assert_eq!(sum, Some(6));
    }

    #[test]
    fn asking_whether_a_type_is_declared_walks_it_too() {
        create_root_owner();
        assert!(!has_context::<u64>());
        provide_context(99u64);

        assert!(in_a_child_scope(has_context::<u64>));
    }

    #[test]
    fn a_type_nothing_declared_is_not_found() {
        create_root_owner();
        assert_eq!(use_context::<String>(), None);
        assert_eq!(with_context::<Vec<i32>, _>(|v| v.len()), None);
    }

    #[test]
    #[should_panic(expected = "No value of type")]
    fn expecting_a_type_nothing_declared_panics() {
        create_root_owner();
        expect_context::<f64>();
    }

    #[test]
    fn types_do_not_collide() {
        create_root_owner();
        provide_context(42u32);
        provide_context("hello".to_string());
        provide_context(2.72f64);

        assert_eq!(use_context::<u32>(), Some(42));
        assert_eq!(use_context::<String>(), Some("hello".to_string()));
        assert_eq!(use_context::<f64>(), Some(2.72));
    }

    /// `provide_signal_context` deposits an `RwSignal<T>`, which is what the
    /// docs have to say it deposits: the example that asked for a `Signal<T>`
    /// was five months wrong because nothing ran it.
    #[test]
    fn a_provided_signal_is_read_back_as_the_read_write_handle() {
        create_root_owner();
        let signal = provide_signal_context(100i32);

        let retrieved = expect_context::<RwSignal<i32>>();
        assert_eq!(retrieved.get(), 100);

        signal.set(200);
        assert_eq!(retrieved.get(), 200);
    }
}
