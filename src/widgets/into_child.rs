//! Child attachment forms for containers.
//!
//! Children come in four forms — two static, two reactive:
//!
//! ```ignore
//! container()
//!     .child(text("hi"))                                  // static single
//!     .child(move || build_menu(entries.get()))           // reactive single
//!     .children([a, b, c])                                // static list
//!     .children(keyed(move || items.get(), |i| i.id, row)) // reactive keyed list
//! ```
//!
//! A reactive closure re-runs when a signal it read is written, and its
//! result **replaces** the previous widget — there is no key comparison and
//! no silent discard. Tracking is per segment: only the closure whose
//! signals changed re-runs, not every dynamic child of the container.
//!
//! State created inside a closure (signals, effects, `on_cleanup`) lives in
//! an owner scope disposed on replacement. State that must survive a rebuild
//! belongs in signals created outside the closure. To narrow *when* a
//! closure re-runs, read a [`Memo`](crate::reactive::Memo) instead of the
//! raw signal — memos notify only when their value actually changes.
//!
//! For lists, [`keyed()`] preserves per-row state through stable identity
//! and rebuilds only rows whose item content changed.

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::reactive::invalidation::suspend_widget_tracking;
use crate::reactive::{OwnerId, dispose_owner_now, with_owner};

use super::Widget;
use super::children::{ChildrenSource, DynItem, OwnedWidget, SharedOwner};

/// Marker type for a static child (widget value)
pub struct StaticChild;

/// Marker type for a reactive child (closure)
pub struct DynamicChild;

/// Trait for values that can be added as a single child to a container.
///
/// Accepted forms:
/// - a widget value — static, evaluated once
/// - `move || widget` / `move || Option<widget>` — reactive: the closure
///   re-runs when a signal it reads changes, and its result replaces the
///   previous child (`None` removes it)
#[diagnostic::on_unimplemented(
    message = "`.child()` takes a widget or a reactive closure, and `{Self}` is neither",
    note = "reactive forms: `.child(move || widget)` or `.child(move || Option<widget>)` — the closure re-runs and replaces the child when a signal it reads changes",
    note = "for reactive lists use `.children(...)` with a closure or `keyed(data, key, build)`"
)]
pub trait IntoChild<Marker = StaticChild> {
    fn add_to_container(self, children_source: &mut ChildrenSource);
}

// Implementation for static widgets
impl<W: Widget + 'static> IntoChild<StaticChild> for W {
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        children_source.add_static(Box::new(self));
    }
}

/// What a reactive child closure may return: a widget (always present) or
/// `Option<widget>` (present when Some).
pub trait IntoDynChild {
    fn into_dyn_child(self) -> Option<Box<dyn Widget>>;
}

impl<W: Widget + 'static> IntoDynChild for W {
    fn into_dyn_child(self) -> Option<Box<dyn Widget>> {
        Some(Box::new(self))
    }
}

impl<W: Widget + 'static> IntoDynChild for Option<W> {
    fn into_dyn_child(self) -> Option<Box<dyn Widget>> {
        self.map(|w| Box::new(w) as Box<dyn Widget>)
    }
}

/// Reactive single child: the closure runs inside its segment's tracking
/// scope — the signals it reads are what trigger a re-run — and inside an
/// owner scope, so signals/effects created during the build are disposed
/// when the widget is replaced or removed. Every re-run replaces the child.
impl<F, C> IntoChild<DynamicChild> for F
where
    F: Fn() -> C + 'static,
    C: IntoDynChild,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        struct State {
            generation: u64,
            /// Widget built by the latest run, adopted by the reconciler's
            /// factory call in the same pass (the fresh generation key
            /// guarantees the factory runs).
            pending: Option<(Box<dyn Widget>, OwnerId)>,
        }

        let state = Rc::new(RefCell::new(State {
            generation: 0,
            pending: None,
        }));
        let child_fn = Rc::new(self);

        let items_fn = move || {
            let mut st = state.borrow_mut();

            // Defensive: a widget stashed by a previous pass that was never
            // adopted would leak its owner scope.
            if let Some((_, owner_id)) = st.pending.take() {
                dispose_owner_now(owner_id);
            }

            let child_fn = Rc::clone(&child_fn);
            let (built, owner_id) = with_owner(move || child_fn().into_dyn_child());
            match built {
                Some(widget) => {
                    st.generation += 1;
                    st.pending = Some((widget, owner_id));
                    let adopt_state = Rc::clone(&state);
                    vec![DynItem::new(st.generation, move || {
                        let (widget, owner_id) =
                            adopt_state.borrow_mut().pending.take().expect(
                                "reactive child factory ran without a freshly built widget",
                            );
                        OwnedWidget::new(widget, owner_id)
                    })]
                }
                None => {
                    dispose_owner_now(owner_id);
                    vec![]
                }
            }
        };

        children_source.add_dynamic(items_fn);
    }
}

/// Marker type for static children (iterator of widgets)
pub struct StaticChildren;

/// Marker type for reactive children (closure or [`keyed()`])
pub struct DynamicChildren;

/// Trait for values that can be added as children to a container.
///
/// Accepted forms:
/// - an iterator of widgets — static, evaluated once
/// - `move || iterator_of_widgets` — reactive: re-runs when a signal it
///   reads changes, replacing all rows
/// - [`keyed(data, key, build)`](keyed) — reactive with stable identity:
///   preserves per-row state, rebuilds only changed rows
#[diagnostic::on_unimplemented(
    message = "`.children()` takes an iterator of widgets, a reactive closure returning one, or `keyed(data, key, build)` — and `{Self}` is none of those",
    note = "`.children(move || widgets)` replaces all rows when a signal it reads changes; `keyed(data, key, build)` preserves per-row state via stable identity and rebuilds only rows whose item changed"
)]
pub trait IntoChildren<Marker = StaticChildren> {
    fn add_to_container(self, children_source: &mut ChildrenSource);
}

// Implementation for static children - IntoIterator<Item = W> where W: Widget
// Each widget in the iterator becomes a separate static slot
impl<I, W> IntoChildren<StaticChildren> for I
where
    I: IntoIterator<Item = W>,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        for widget in self {
            children_source.add_static(Box::new(widget));
        }
    }
}

/// Reactive unkeyed list: the closure re-runs when a signal it reads
/// changes and its rows replace the previous ones wholesale. All rows of
/// one run share a single owner scope, disposed when the last row drops.
///
/// Rows have no identity across runs — inner state does not survive a
/// re-run. Use [`keyed()`] when rows carry state worth preserving.
impl<F, I, W> IntoChildren<DynamicChildren> for F
where
    F: Fn() -> I + 'static,
    I: IntoIterator<Item = W>,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        struct State {
            next_generation: u64,
            /// Rows built by the latest run, adopted by the reconciler's
            /// factory calls in the same pass.
            pending: FxHashMap<u64, Box<dyn Widget>>,
        }

        let state = Rc::new(RefCell::new(State {
            next_generation: 0,
            pending: FxHashMap::default(),
        }));
        let list_fn = Rc::new(self);

        let items_fn = move || {
            let mut st = state.borrow_mut();
            // Defensive: unadopted rows from a previous pass (their shared
            // owner guard was dropped with the unadopted factories).
            st.pending.clear();

            let list_fn = Rc::clone(&list_fn);
            let (widgets, owner_id) = with_owner(move || {
                list_fn()
                    .into_iter()
                    .map(|w| Box::new(w) as Box<dyn Widget>)
                    .collect::<Vec<_>>()
            });
            if widgets.is_empty() {
                dispose_owner_now(owner_id);
                return vec![];
            }

            let shared = SharedOwner::new(owner_id);
            let mut out = Vec::with_capacity(widgets.len());
            for widget in widgets {
                let generation = st.next_generation;
                st.next_generation += 1;
                st.pending.insert(generation, widget);

                let adopt_state = Rc::clone(&state);
                let shared = shared.clone();
                out.push(DynItem::new(generation, move || {
                    let widget = adopt_state
                        .borrow_mut()
                        .pending
                        .remove(&generation)
                        .expect("reactive children factory ran without a freshly built row");
                    OwnedWidget::new_shared(widget, shared)
                }));
            }
            out
        };

        children_source.add_dynamic(items_fn);
    }
}

/// A reactive keyed children list: tracked data, key-based identity,
/// untracked per-item builder. Built with [`keyed()`], consumed by
/// `.children()`.
pub struct KeyedChildren<T, I, K, W> {
    data: Box<dyn Fn() -> I>,
    key: Box<dyn Fn(&T) -> K>,
    build: Box<dyn Fn(T) -> W>,
}

/// Reactive keyed children list: tracked data, key-based identity,
/// untracked per-item builder with content diffing.
///
/// `data` runs inside the segment's tracking scope and yields the items.
/// `key` extracts each item's stable identity (it survives reorders). Per
/// item:
///
/// - new key → `build` runs (untracked, in an owner scope)
/// - known key, item `==` previous → the existing widget is kept untouched
///   (inner state and animations persist, including across reorders)
/// - known key, item changed → `build` re-runs and the rebuilt widget
///   replaces the old one (its owner scope is disposed)
/// - key gone → widget dropped with owner cleanup
///
/// The item type chooses the granularity: fields excluded from it can be
/// read via signals inside the row for in-place updates, fields included
/// trigger a row rebuild when they change.
///
/// The key is any `Hash + Eq + Clone`, so an identity that is already a string
/// or a tuple can be used as it is rather than hashed by hand at the call site.
/// Rows are indexed by the key itself, not by a hash of it, so two distinct keys
/// are never reconciled as one:
///
/// ```ignore
/// container().children(keyed(
///     move || workspaces.get(),
///     |ws| ws.id,
///     workspace_pill,
/// ))
///
/// container().children(keyed(
///     move || tabs.get(),
///     |tab| tab.title.clone(),
///     tab_button,
/// ))
/// ```
pub fn keyed<T, I, K, W>(
    data: impl Fn() -> I + 'static,
    key: impl Fn(&T) -> K + 'static,
    build: impl Fn(T) -> W + 'static,
) -> KeyedChildren<T, I, K, W>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    K: Hash + Eq + Clone + 'static,
    W: Widget + 'static,
{
    KeyedChildren {
        data: Box::new(data),
        key: Box::new(key),
        build: Box::new(build),
    }
}

impl<T, I, K, W> IntoChildren<DynamicChildren> for KeyedChildren<T, I, K, W>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T> + 'static,
    K: Hash + Eq + Clone + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        add_keyed_children(children_source, self.data, self.key, self.build);
    }
}

/// Wire a (tracked data, key, untracked builder) triple into a
/// ChildrenSource as a keyed dynamic list. See [`keyed()`] for the
/// semantics.
fn add_keyed_children<T, I, K, W>(
    source: &mut ChildrenSource,
    data_fn: impl Fn() -> I + 'static,
    key_fn: impl Fn(&T) -> K + 'static,
    build_fn: impl Fn(T) -> W + 'static,
) where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    K: Hash + Eq + Clone + 'static,
    W: Widget + 'static,
{
    struct Row<T> {
        item: T,
        /// Serves as the reconciliation key: globally unique per (identity,
        /// content version), stable while the item compares equal.
        generation: u64,
    }
    /// Rows are indexed by the key itself, not by a hash of it. Reducing the
    /// key to a u64 first would make two colliding keys the same row — one
    /// widget serving two items and the other silently dropped — and no
    /// non-cryptographic hash can rule that out for keys like strings and
    /// tuples, which is exactly what this signature invites.
    struct KeyedState<T, K> {
        rows: FxHashMap<K, Row<T>>,
        next_generation: u64,
        /// Widgets built eagerly this pass, awaiting adoption by the
        /// reconciler's factory calls (keyed by generation).
        pending: FxHashMap<u64, (Box<dyn Widget>, OwnerId)>,
    }

    // Fx rather than the std hasher: this runs once per row per pass, on every
    // frame a list changes, and widening the key from u64 to any Hash made a
    // string key ordinary. These are the app's own identities, not adversarial
    // input, which is the condition SipHash is there for.
    let state = Rc::new(RefCell::new(KeyedState::<T, K> {
        rows: FxHashMap::default(),
        next_generation: 0,
        pending: FxHashMap::default(),
    }));

    let items_fn = move || {
        let items = data_fn(); // tracked: runs inside the segment scope
        let mut st = state.borrow_mut();

        // Defensive: widgets stashed by a previous pass that were never
        // adopted would leak their owner scopes.
        for (_, (_, owner_id)) in st.pending.drain() {
            dispose_owner_now(owner_id);
        }

        let mut seen = FxHashSet::default();
        let mut out = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let key = key_fn(&item);
            // One lookup rather than a `contains` and then an `insert`: this runs
            // per row per pass, and the clone a fresh row would have paid anyway
            // is paid once here instead.
            if !seen.insert(key.clone()) {
                // Without the key's value: `Debug` would be a bound the mechanism
                // does not need, and it would keep an opaque newtype out of
                // `keyed` in order to format a log line. The type and the
                // position are what can be said for nothing.
                log::warn!(
                    "keyed children: duplicate {} key at index {index}, skipping item",
                    std::any::type_name::<K>()
                );
                continue;
            }

            let generation = match st.rows.get(&key) {
                Some(row) if row.item == item => row.generation,
                _ => {
                    let generation = st.next_generation;
                    st.next_generation += 1;
                    let (widget, owner_id) = with_owner(|| {
                        suspend_widget_tracking(|| {
                            Box::new(build_fn(item.clone())) as Box<dyn Widget>
                        })
                    });
                    st.pending.insert(generation, (widget, owner_id));
                    st.rows.insert(key.clone(), Row { item, generation });
                    generation
                }
            };

            let adopt_state = Rc::clone(&state);
            out.push(DynItem::new(generation, move || {
                let (widget, owner_id) = adopt_state
                    .borrow_mut()
                    .pending
                    .remove(&generation)
                    .expect("keyed children factory ran without a freshly built widget");
                OwnedWidget::new(widget, owner_id)
            }));
        }
        st.rows.retain(|key, _| seen.contains(key));
        out
    };

    source.add_dynamic(items_fn);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::layout::{Constraints, Size};
    use crate::reactive::{create_signal, on_cleanup};
    use crate::tree::{Tree, WidgetId};

    struct TestWidget;
    impl Widget for TestWidget {
        fn layout(&mut self, _: &mut Tree, _: WidgetId, _: Constraints) -> Size {
            Size::zero()
        }
        fn paint(&self, _: &Tree, _: WidgetId, _: &mut crate::renderer::PaintContext) {}
    }

    /// A parented ChildrenSource ready for reconciliation.
    fn children_host() -> (Tree, ChildrenSource) {
        let mut tree = Tree::new();
        let parent = tree.register(Box::new(TestWidget));
        let mut source = ChildrenSource::default();
        source.set_container_id(parent);
        (tree, source)
    }

    /// Two keys that a 64-bit hash cannot tell apart must still be two rows.
    /// While the reconciler indexed by `FxHash` of the key this collapsed them:
    /// one widget served both items and the other was silently dropped.
    #[test]
    fn colliding_keys_are_still_distinct_rows() {
        /// A key whose hash is deliberately useless, standing in for the
        /// collision a real hash makes merely unlikely.
        // Deliberately not Debug: the key bound must not require it.
        #[derive(Clone, PartialEq, Eq)]
        struct Collides(&'static str);

        impl std::hash::Hash for Collides {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                0u8.hash(state);
            }
        }

        let (mut tree, mut source) = children_host();
        let items = create_signal(vec![Collides("a"), Collides("b")]);

        IntoChildren::<DynamicChildren>::add_to_container(
            keyed(
                move || items.get(),
                |c: &Collides| c.clone(),
                |_| TestWidget,
            ),
            &mut source,
        );

        let ids = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(ids.len(), 2, "two keys, two widgets");

        // And the identity survives a reorder: same widgets, swapped.
        items.set(vec![Collides("b"), Collides("a")]);
        source.reconcile_with_tracking(&mut tree);
        let reordered = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(reordered, vec![ids[1], ids[0]]);
    }

    #[test]
    fn closure_child_replaces_on_tracked_change() {
        let (mut tree, mut source) = children_host();
        let sig = create_signal(1u64);
        let unrelated = create_signal(0u64);
        let builds = Rc::new(Cell::new(0usize));

        let builds_counter = builds.clone();
        let closure = move || {
            sig.get();
            builds_counter.set(builds_counter.get() + 1);
            TestWidget
        };
        IntoChild::<DynamicChild>::add_to_container(closure, &mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 1);
        assert_eq!(builds.get(), 1);

        // Untracked signal change: segment not dirty, widget untouched
        unrelated.set(1);
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).clone(), first);
        assert_eq!(builds.get(), 1);

        // Tracked signal change: closure re-runs, widget replaced
        sig.set(2);
        source.reconcile_with_tracking(&mut tree);
        let second = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0], first[0]);
        assert_eq!(builds.get(), 2);

        // No-op write (same value): signals skip notification entirely
        sig.set(2);
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).clone(), second);
        assert_eq!(builds.get(), 2);
    }

    /// A replaced dynamic child's WHOLE subtree must leave the tree during
    /// the reconcile itself. If descendants lingered for deferred cleanup,
    /// their queued reconciles could still run in the same batch and read
    /// state owned (and just disposed) by the replaced root.
    #[test]
    fn replaced_child_subtree_is_torn_down_synchronously() {
        struct ParentWidget;
        impl Widget for ParentWidget {
            fn layout(&mut self, _: &mut Tree, _: WidgetId, _: Constraints) -> Size {
                Size::zero()
            }
            fn paint(&self, _: &Tree, _: WidgetId, _: &mut crate::renderer::PaintContext) {}
            fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {
                let child = tree.register(Box::new(TestWidget));
                tree.set_parent(child, id);
            }
        }

        let (mut tree, mut source) = children_host();
        let sig = create_signal(0u64);

        let closure = move || {
            sig.get();
            ParentWidget
        };
        IntoChild::<DynamicChild>::add_to_container(closure, &mut source);
        source.reconcile_and_get(&mut tree);

        // host + row root + row's registered child
        let count_after_build = tree.widget_count();
        assert_eq!(count_after_build, 3);

        // Tracked change: the row is replaced by a fresh build
        sig.set(1);
        source.reconcile_with_tracking(&mut tree);

        // Same shape, and the OLD subtree is fully gone already — no
        // deferred leftovers
        assert_eq!(tree.widget_count(), count_after_build);
    }

    #[test]
    fn segments_rerun_independently() {
        let (mut tree, mut source) = children_host();
        let sig_a = create_signal(0u64);
        let sig_b = create_signal(0u64);

        let closure_a = move || {
            sig_a.get();
            TestWidget
        };
        let closure_b = move || {
            sig_b.get();
            TestWidget
        };
        IntoChild::<DynamicChild>::add_to_container(closure_a, &mut source);
        IntoChild::<DynamicChild>::add_to_container(closure_b, &mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 2);

        // Only segment A's signal changes: B's widget must survive
        sig_a.set(1);
        source.reconcile_with_tracking(&mut tree);
        let second = source.reconcile_and_get(&mut tree).clone();
        assert_ne!(second[0], first[0]);
        assert_eq!(second[1], first[1]);
    }

    #[test]
    fn closure_child_controls_presence() {
        let (mut tree, mut source) = children_host();
        let show = create_signal(false);

        let closure = move || show.get().then_some(TestWidget);
        IntoChild::<DynamicChild>::add_to_container(closure, &mut source);

        assert!(source.reconcile_and_get(&mut tree).is_empty());

        show.set(true);
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).len(), 1);

        show.set(false);
        source.reconcile_with_tracking(&mut tree);
        assert!(source.reconcile_and_get(&mut tree).is_empty());
    }

    #[test]
    fn closure_children_replace_all_rows_and_dispose_batch_owner() {
        let (mut tree, mut source) = children_host();
        let count = create_signal(2u64);
        let cleanups = Rc::new(Cell::new(0usize));

        let cleanups_counter = cleanups.clone();
        let closure = move || {
            let cleanups_counter = cleanups_counter.clone();
            on_cleanup(move || cleanups_counter.set(cleanups_counter.get() + 1));
            (0..count.get()).map(|_| TestWidget).collect::<Vec<_>>()
        };
        IntoChildren::<DynamicChildren>::add_to_container(closure, &mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 2);
        assert_eq!(cleanups.get(), 0);

        count.set(3);
        source.reconcile_with_tracking(&mut tree);
        let second = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(second.len(), 3);
        // All rows replaced, previous batch owner disposed exactly once
        assert!(first.iter().all(|id| !second.contains(id)));
        assert_eq!(cleanups.get(), 1);
    }

    #[test]
    fn keyed_children_diff_by_identity_and_content() {
        let (mut tree, mut source) = children_host();
        let items = create_signal(vec![(1u64, "a".to_string()), (2, "b".to_string())]);
        let builds = Rc::new(Cell::new(0usize));

        let builds_counter = builds.clone();
        keyed(
            move || items.get(),
            |(id, _)| *id,
            move |_| {
                builds_counter.set(builds_counter.get() + 1);
                TestWidget
            },
        )
        .add_to_container(&mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 2);
        assert_eq!(builds.get(), 2);

        // Reorder with equal content: widgets reused, order follows data
        items.set(vec![(2, "b".to_string()), (1, "a".to_string())]);
        source.reconcile_with_tracking(&mut tree);
        let reordered = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(reordered, vec![first[1], first[0]]);
        assert_eq!(builds.get(), 2);

        // Content change on one item: only that row rebuilds
        items.set(vec![(2, "B!".to_string()), (1, "a".to_string())]);
        source.reconcile_with_tracking(&mut tree);
        let changed = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(changed.len(), 2);
        assert_ne!(changed[0], reordered[0]); // row 2 rebuilt
        assert_eq!(changed[1], reordered[1]); // row 1 untouched
        assert_eq!(builds.get(), 3);

        // Removal drops only the removed row
        items.set(vec![(1, "a".to_string())]);
        source.reconcile_with_tracking(&mut tree);
        let removed = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(removed, vec![changed[1]]);
        assert_eq!(builds.get(), 3);
    }

    #[test]
    fn keyed_children_skip_duplicate_keys() {
        let (mut tree, mut source) = children_host();

        keyed(
            move || vec![(7u64, "x"), (7, "y")],
            |(id, _)| *id,
            |_| TestWidget,
        )
        .add_to_container(&mut source);

        // Only the first item with a given key is rendered
        assert_eq!(source.reconcile_and_get(&mut tree).len(), 1);
    }
}
