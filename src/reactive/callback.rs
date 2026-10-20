//! `Callback` — a `Copy` handle to a closure.
//!
//! An `Rc<dyn Fn(..)>` has to be cloned for every closure that wants to call
//! it, which is how component bodies end up opening with a row of
//! `let on_click = on_click.clone();` and cloning again inside each nested
//! closure. A `Callback` stores the closure in the reactive arena and hands
//! back its id, so the handle is `Copy` and goes wherever it is needed.
//!
//! It is also what keeps a struct that groups handles `Copy` — the same
//! reason [`Signal`] and [`Service`](super::Service) are.
//!
//! The argument list is a tuple, and each arity gets its own `new`/`run` so
//! both ends stay flat:
//!
//! ```no_run
//! # use guido::prelude::*;
//! let greet = Callback::new(|name: String| println!("hi {name}"));
//! greet.run("world".into());
//! ```

use std::rc::Rc;

use super::signal::{Signal, create_stored};

/// A `Copy` handle to a closure, parameterised by its argument tuple and what
/// it returns.
///
/// `Callback<()>` takes no arguments, `Callback<(i32,)>` takes one,
/// `Callback<(f32, f32)>` takes two; each returns `()`. `Callback<(u32,),
/// Container>` takes a `u32` and returns a `Container`, which is what a render
/// prop is. `#[component]` callback props are these.
pub struct Callback<Args = (), Ret = ()> {
    inner: Signal<Rc<dyn Fn(Args) -> Ret>>,
}

// Manual impls: the derives would demand bounds on `Args` and `Ret`, but the
// handle is an arena id whatever the closure takes and gives.
impl<Args, Ret> Clone for Callback<Args, Ret> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Args, Ret> Copy for Callback<Args, Ret> {}

impl<Args: 'static, Ret: 'static> Callback<Args, Ret> {
    pub(crate) fn from_rc(f: Rc<dyn Fn(Args) -> Ret>) -> Self {
        Self {
            inner: create_stored(f),
        }
    }

    /// Wrap a closure. The arity is inferred from the closure itself:
    /// `Callback::new(|| …)`, `Callback::new(|v: i32| …)`,
    /// `Callback::new(|x: f32, y: f32| …)`, and the return type with it.
    pub fn new<F: IntoCallback<Args, Ret>>(f: F) -> Self {
        Self::from_rc(f.into_packed())
    }

    /// Call with the arguments already packed into their tuple.
    ///
    /// Prefer the flat `run` of the matching arity; this is the escape hatch
    /// for generic code that only knows `Args`.
    pub fn run_packed(&self, args: Args) -> Ret {
        self.inner.with_untracked(|f| f(args))
    }
}

/// Closures that can become a [`Callback`], one impl per arity.
///
/// Same shape as [`IntoSignal`](super::IntoSignal): the trait parameter is
/// what lets one constructor accept closures of different arities.
pub trait IntoCallback<Args, Ret = ()> {
    /// The closure, taking its arguments as one tuple.
    fn into_packed(self) -> Rc<dyn Fn(Args) -> Ret>;
}

impl<R: 'static, F: Fn() -> R + 'static> IntoCallback<(), R> for F {
    fn into_packed(self) -> Rc<dyn Fn(()) -> R> {
        Rc::new(move |()| self())
    }
}

impl<A: 'static, R: 'static, F: Fn(A) -> R + 'static> IntoCallback<(A,), R> for F {
    fn into_packed(self) -> Rc<dyn Fn((A,)) -> R> {
        Rc::new(move |(a,)| self(a))
    }
}

impl<A: 'static, B: 'static, R: 'static, F: Fn(A, B) -> R + 'static> IntoCallback<(A, B), R> for F {
    fn into_packed(self) -> Rc<dyn Fn((A, B)) -> R> {
        Rc::new(move |(a, b)| self(a, b))
    }
}

impl<A: 'static, B: 'static, C: 'static, R: 'static, F: Fn(A, B, C) -> R + 'static>
    IntoCallback<(A, B, C), R> for F
{
    fn into_packed(self) -> Rc<dyn Fn((A, B, C)) -> R> {
        Rc::new(move |(a, b, c)| self(a, b, c))
    }
}

#[doc(hidden)]
pub struct FnHandler;
#[doc(hidden)]
pub struct CallbackHandler;
#[doc(hidden)]
pub struct OptionHandler;

/// What an event handler can be written as: a closure, a [`Callback`], or an
/// `Option<Callback>`.
///
/// A closure is the ordinary case. The other two are the shapes a
/// `#[component]` callback prop arrives in, so a component forwards its own
/// prop, to a container's `on_click` or to another component's builder, as it
/// reads: `.on_change(on_change)`. A `None` leaves the handler unset.
///
/// `Ret` is what the closure returns: `()` for a handler, the page for a
/// render prop.
///
/// The marker parameter is the same trick [`IntoSignal`](super::IntoSignal)
/// uses: it keeps the impls from overlapping.
pub trait IntoHandler<Args, Marker = FnHandler, Ret = ()> {
    /// As a closure the widget keeps for itself.
    fn into_handler(self) -> Option<Rc<dyn Fn(Args) -> Ret>>;
    /// As a handle, which is what a `#[component]` callback prop holds.
    fn into_callback_prop(self) -> Option<Callback<Args, Ret>>;
}

impl<Args: 'static, Ret: 'static, F: IntoCallback<Args, Ret>> IntoHandler<Args, FnHandler, Ret>
    for F
{
    fn into_handler(self) -> Option<Rc<dyn Fn(Args) -> Ret>> {
        Some(self.into_packed())
    }

    fn into_callback_prop(self) -> Option<Callback<Args, Ret>> {
        Some(Callback::new(self))
    }
}

impl<Args: 'static, Ret: 'static> IntoHandler<Args, CallbackHandler, Ret> for Callback<Args, Ret> {
    fn into_handler(self) -> Option<Rc<dyn Fn(Args) -> Ret>> {
        Some(Rc::new(move |args| self.run_packed(args)))
    }

    fn into_callback_prop(self) -> Option<Callback<Args, Ret>> {
        Some(self)
    }
}

impl<Args: 'static, Ret: 'static> IntoHandler<Args, OptionHandler, Ret>
    for Option<Callback<Args, Ret>>
{
    fn into_handler(self) -> Option<Rc<dyn Fn(Args) -> Ret>> {
        self.and_then(IntoHandler::into_handler)
    }

    fn into_callback_prop(self) -> Option<Callback<Args, Ret>> {
        self
    }
}

impl<Ret: 'static> Callback<(), Ret> {
    /// Call the closure.
    pub fn run(&self) -> Ret {
        self.run_packed(())
    }
}

impl<A: 'static, Ret: 'static> Callback<(A,), Ret> {
    /// Call the closure.
    pub fn run(&self, a: A) -> Ret {
        self.run_packed((a,))
    }
}

impl<A: 'static, B: 'static, Ret: 'static> Callback<(A, B), Ret> {
    /// Call the closure.
    pub fn run(&self, a: A, b: B) -> Ret {
        self.run_packed((a, b))
    }
}

impl<A: 'static, B: 'static, C: 'static, Ret: 'static> Callback<(A, B, C), Ret> {
    /// Call the closure.
    pub fn run(&self, a: A, b: B, c: C) -> Ret {
        self.run_packed((a, b, c))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    /// The whole point: a handle that can be dropped into several closures
    /// without a clone in sight.
    #[test]
    fn a_callback_is_copy_and_callable_from_many_closures() {
        let hits = Rc::new(Cell::new(0));
        let hits_cb = hits.clone();
        let cb = Callback::new(move || hits_cb.set(hits_cb.get() + 1));

        let first = move || cb.run();
        let second = move || cb.run();
        first();
        second();
        cb.run();

        assert_eq!(hits.get(), 3);
    }

    #[test]
    fn arguments_stay_flat() {
        let seen = Rc::new(Cell::new(0));
        let seen_one = seen.clone();
        let one = Callback::new(move |v: i32| seen_one.set(v));
        one.run(7);
        assert_eq!(seen.get(), 7);

        let seen_two = seen.clone();
        let two = Callback::new(move |a: i32, b: i32| seen_two.set(a * b));
        two.run(6, 7);
        assert_eq!(seen.get(), 42);
    }

    /// A callback returns what its closure returns, at every arity, and a
    /// handle that names only its arguments still means one returning `()`.
    #[test]
    fn run_returns_the_closures_value() {
        assert_eq!(Callback::new(|| 3).run(), 3);
        assert_eq!(Callback::new(|v: u32| v * 2).run(21), 42);
        assert_eq!(Callback::new(|a: u32, b: u32| a + b).run(40, 2), 42);
        assert_eq!(
            Callback::new(|a: u32, b: u32, c: u32| a * b * c).run(2, 3, 7),
            42
        );

        let unit: Callback<(u32,)> = Callback::new(|_: u32| {});
        let () = unit.run(1);
    }
}
