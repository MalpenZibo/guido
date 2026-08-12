//! `Callback` — a `Copy` handle to a closure.
//!
//! An `Rc<dyn Fn(..)>` has to be cloned for every closure that wants to call
//! it, which is how component bodies end up opening with a row of
//! `let on_click = on_click.clone();` and cloning again inside each nested
//! closure. A `Callback` stores the closure in the reactive arena and hands
//! back its id, so the handle is `Copy` and goes wherever it is needed.
//!
//! It is also what keeps a struct that groups handles `Copy` — the same
//! reason [`Signal`](super::Signal) and [`Service`](super::Service) are.
//!
//! The argument list is a tuple, and each arity gets its own `new`/`run` so
//! both ends stay flat:
//!
//! ```ignore
//! let greet = Callback::new(|name: String| println!("hi {name}"));
//! greet.run("world".into());
//! ```

use std::marker::PhantomData;
use std::rc::Rc;

use super::signal::{Signal, create_stored};

/// A `Copy` handle to a closure, parameterised by its argument tuple.
///
/// `Callback<()>` takes no arguments, `Callback<(i32,)>` takes one,
/// `Callback<(f32, f32)>` takes two. `#[component]` callback props are these.
pub struct Callback<Args = ()> {
    inner: Signal<Rc<dyn Fn(Args)>>,
    _args: PhantomData<Args>,
}

// Manual impls: the derives would demand bounds on `Args`, but the handle is
// an arena id whatever the arguments are.
impl<Args> Clone for Callback<Args> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Args> Copy for Callback<Args> {}

impl<Args: 'static> Callback<Args> {
    fn from_rc(f: Rc<dyn Fn(Args)>) -> Self {
        Self {
            inner: create_stored(f),
            _args: PhantomData,
        }
    }

    /// Call with the arguments already packed into their tuple.
    ///
    /// Prefer the flat `run` of the matching arity; this is the escape hatch
    /// for generic code that only knows `Args`.
    pub fn run_packed(&self, args: Args) {
        self.inner.with_untracked(|f| f(args));
    }
}

impl<Args: 'static> Callback<Args> {
    /// Wrap a closure. The arity is inferred from the closure itself:
    /// `Callback::new(|| …)`, `Callback::new(|v: i32| …)`,
    /// `Callback::new(|x: f32, y: f32| …)`.
    pub fn new<F: IntoCallback<Args>>(f: F) -> Self {
        f.into_callback()
    }
}

/// Closures that can become a [`Callback`], one impl per arity.
///
/// Same shape as [`IntoSignal`](super::IntoSignal): the trait parameter is
/// what lets one constructor accept closures of different arities.
pub trait IntoCallback<Args> {
    fn into_callback(self) -> Callback<Args>;
}

impl<F: Fn() + 'static> IntoCallback<()> for F {
    fn into_callback(self) -> Callback<()> {
        Callback::from_rc(Rc::new(move |()| self()))
    }
}

impl<A: 'static, F: Fn(A) + 'static> IntoCallback<(A,)> for F {
    fn into_callback(self) -> Callback<(A,)> {
        Callback::from_rc(Rc::new(move |(a,)| self(a)))
    }
}

impl<A: 'static, B: 'static, F: Fn(A, B) + 'static> IntoCallback<(A, B)> for F {
    fn into_callback(self) -> Callback<(A, B)> {
        Callback::from_rc(Rc::new(move |(a, b)| self(a, b)))
    }
}

impl<A: 'static, B: 'static, C: 'static, F: Fn(A, B, C) + 'static> IntoCallback<(A, B, C)> for F {
    fn into_callback(self) -> Callback<(A, B, C)> {
        Callback::from_rc(Rc::new(move |(a, b, c)| self(a, b, c)))
    }
}

impl Callback<()> {
    /// Call the closure.
    pub fn run(&self) {
        self.run_packed(());
    }
}

impl<A: 'static> Callback<(A,)> {
    /// Call the closure.
    pub fn run(&self, a: A) {
        self.run_packed((a,));
    }
}

impl<A: 'static, B: 'static> Callback<(A, B)> {
    /// Call the closure.
    pub fn run(&self, a: A, b: B) {
        self.run_packed((a, b));
    }
}

impl<A: 'static, B: 'static, C: 'static> Callback<(A, B, C)> {
    /// Call the closure.
    pub fn run(&self, a: A, b: B, c: C) {
        self.run_packed((a, b, c));
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
}
