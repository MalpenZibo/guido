use std::cell::RefCell;
use std::rc::Rc;

use crate::reactive::invalidation::suspend_widget_tracking;
use crate::reactive::{OwnerId, dispose_owner, with_owner};

use super::Widget;
use super::children::{ChildrenSource, DynItem, OwnedWidget};

/// Marker type for a static child (widget value)
pub struct StaticChild;

/// Marker type for a reactive child (built with [`dynamic()`])
pub struct DynamicChild;

/// Trait for values that can be added as a single child to a container.
///
/// Accepted forms:
/// - a widget value — static, evaluated once
/// - [`dynamic(data, build)`](dynamic) — reactive content
///
/// Bare closures are rejected at compile time: with them, content updates
/// could be silently discarded (there is no way to know which data the
/// widget was built from). `dynamic()` makes that data explicit.
#[diagnostic::on_unimplemented(
    message = "`.child()` takes a widget or `dynamic(data, build)`, and `{Self}` is neither",
    note = "for reactive content use `.child(dynamic(data, build))`: `data` is a tracked closure returning `Clone + PartialEq` data, `build` turns that data into the widget (return `Option<_>` to also control presence)",
    note = "for reactive lists use `.children(keyed(data, key, build))`"
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

/// What a [`dynamic()`] builder may return: a widget (always present) or
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

/// A reactive single child: tracked data + untracked builder.
/// Built with [`dynamic()`], consumed by `.child()`.
pub struct DynChild<T, C> {
    data: Box<dyn Fn() -> T>,
    build: Box<dyn Fn(T) -> C>,
}

/// Reactive single child: tracked data closure + untracked builder.
///
/// `data` runs inside the reconciliation tracking scope — the signals it
/// reads are the ONLY thing that triggers a re-evaluation. Its result is
/// compared with the previous value (`PartialEq`): unchanged data keeps the
/// existing widget untouched (inner state, animations and subscriptions
/// persist); changed data runs `build` and swaps the rebuilt widget in,
/// disposing the old one's owner scope.
///
/// `build` runs with widget tracking suspended: signal reads inside it see
/// current values but create no dependencies. Data the widget must react to
/// has to flow through `data` — that is what makes staleness unexpressible.
///
/// The builder may return a widget, or `Option<widget>` to also control
/// presence.
///
/// ```ignore
/// // Content that re-renders when the data changes
/// container().child(dynamic(move || menu.get(), build_menu))
///
/// // Presence: shown only while there is an active window
/// container().child(dynamic(
///     move || state.active_window.get(),
///     |window| window.map(title_widget),
/// ))
/// ```
pub fn dynamic<T, C>(
    data: impl Fn() -> T + 'static,
    build: impl Fn(T) -> C + 'static,
) -> DynChild<T, C>
where
    T: Clone + PartialEq + 'static,
    C: IntoDynChild,
{
    DynChild {
        data: Box::new(data),
        build: Box::new(build),
    }
}

impl<T, C> IntoChild<DynamicChild> for DynChild<T, C>
where
    T: Clone + PartialEq + 'static,
    C: IntoDynChild + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        add_dyn_child(children_source, self.data, self.build);
    }
}

/// Wire a (tracked data, untracked builder) pair into a ChildrenSource as a
/// single dynamic child. See [`dynamic()`] for the semantics.
fn add_dyn_child<T, C>(
    source: &mut ChildrenSource,
    data_fn: impl Fn() -> T + 'static,
    build_fn: impl Fn(T) -> C + 'static,
) where
    T: Clone + PartialEq + 'static,
    C: IntoDynChild,
{
    struct DynChildState<T> {
        prev: Option<T>,
        generation: u64,
        /// Widget built eagerly on data change, adopted by the reconciler's
        /// factory call in the same pass (the new generation key guarantees
        /// the factory runs whenever something is stashed here).
        pending: Option<(Box<dyn Widget>, OwnerId)>,
        present: bool,
    }

    let state = Rc::new(RefCell::new(DynChildState::<T> {
        prev: None,
        generation: 0,
        pending: None,
        present: false,
    }));

    let items_fn = move || {
        let data = data_fn(); // tracked: runs inside with_signal_tracking
        let mut st = state.borrow_mut();

        if st.prev.as_ref() != Some(&data) {
            // Defensive: a widget stashed by a previous pass that was never
            // adopted would leak its owner scope.
            if let Some((_, owner_id)) = st.pending.take() {
                dispose_owner(owner_id);
            }

            let (built, owner_id) =
                with_owner(|| suspend_widget_tracking(|| build_fn(data.clone()).into_dyn_child()));
            st.prev = Some(data);
            st.generation += 1;
            match built {
                Some(widget) => {
                    st.pending = Some((widget, owner_id));
                    st.present = true;
                }
                None => {
                    dispose_owner(owner_id);
                    st.present = false;
                }
            }
        }

        if st.present {
            let adopt_state = Rc::clone(&state);
            vec![DynItem::new(st.generation, move || {
                let (widget, owner_id) = adopt_state
                    .borrow_mut()
                    .pending
                    .take()
                    .expect("dynamic child factory ran without a freshly built widget");
                OwnedWidget::new(widget, owner_id)
            })]
        } else {
            vec![]
        }
    };

    source.add_dynamic(items_fn);
}

/// Marker type for static children (iterator of widgets)
pub struct StaticChildren;

/// Marker type for reactive children (built with [`keyed()`])
pub struct DynamicChildren;

/// Trait for values that can be added as children to a container.
///
/// Accepted forms:
/// - an iterator of widgets — static, evaluated once
/// - [`keyed(data, key, build)`](keyed) — reactive list
///
/// Bare closures are rejected at compile time: with them, per-item content
/// updates could be silently discarded. `keyed()` makes both the identity
/// and the data of each row explicit.
#[diagnostic::on_unimplemented(
    message = "`.children()` takes an iterator of widgets or `keyed(data, key, build)`, and `{Self}` is neither",
    note = "for reactive lists use `.children(keyed(data, key, build))`: `data` is a tracked closure returning an iterator of `Clone + PartialEq` items, `key` extracts each item's stable identity, `build` turns an item into its widget"
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

/// A reactive children list: tracked data, key-based identity, untracked
/// per-item builder. Built with [`keyed()`], consumed by `.children()`.
pub struct KeyedChildren<T, I, W> {
    data: Box<dyn Fn() -> I>,
    key: Box<dyn Fn(&T) -> u64>,
    build: Box<dyn Fn(T) -> W>,
}

/// Reactive children list: tracked data, key-based identity, untracked
/// per-item builder with content diffing.
///
/// `data` runs inside the reconciliation tracking scope and yields the
/// items. `key` extracts each item's stable identity (it survives reorders).
/// Per item:
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
/// ```ignore
/// container().children(keyed(
///     move || workspaces.get(),
///     |ws| ws.id,
///     workspace_pill,
/// ))
/// ```
pub fn keyed<T, I, W>(
    data: impl Fn() -> I + 'static,
    key: impl Fn(&T) -> u64 + 'static,
    build: impl Fn(T) -> W + 'static,
) -> KeyedChildren<T, I, W>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    W: Widget + 'static,
{
    KeyedChildren {
        data: Box::new(data),
        key: Box::new(key),
        build: Box::new(build),
    }
}

impl<T, I, W> IntoChildren<DynamicChildren> for KeyedChildren<T, I, W>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T> + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        add_dyn_children(children_source, self.data, self.key, self.build);
    }
}

/// Wire a (tracked data, key, untracked builder) triple into a
/// ChildrenSource as a keyed dynamic list. See [`keyed()`] for the
/// semantics.
fn add_dyn_children<T, I, W>(
    source: &mut ChildrenSource,
    data_fn: impl Fn() -> I + 'static,
    key_fn: impl Fn(&T) -> u64 + 'static,
    build_fn: impl Fn(T) -> W + 'static,
) where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    W: Widget + 'static,
{
    struct Row<T> {
        item: T,
        /// Serves as the reconciliation key: globally unique per (identity,
        /// content version), stable while the item compares equal.
        generation: u64,
    }
    struct DynChildrenState<T> {
        rows: std::collections::HashMap<u64, Row<T>>,
        next_generation: u64,
        /// Widgets built eagerly this pass, awaiting adoption by the
        /// reconciler's factory calls (keyed by generation).
        pending: std::collections::HashMap<u64, (Box<dyn Widget>, OwnerId)>,
    }

    let state = Rc::new(RefCell::new(DynChildrenState::<T> {
        rows: std::collections::HashMap::new(),
        next_generation: 0,
        pending: std::collections::HashMap::new(),
    }));

    let items_fn = move || {
        let items = data_fn(); // tracked: runs inside with_signal_tracking
        let mut st = state.borrow_mut();

        // Defensive: widgets stashed by a previous pass that were never
        // adopted would leak their owner scopes.
        for (_, (_, owner_id)) in st.pending.drain() {
            dispose_owner(owner_id);
        }

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for item in items {
            let key = key_fn(&item);
            if !seen.insert(key) {
                log::warn!("keyed children: duplicate key {key}, skipping item");
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
                    st.rows.insert(key, Row { item, generation });
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

    #[test]
    fn dynamic_child_rebuilds_only_when_data_changes() {
        let (mut tree, mut source) = children_host();
        let data = Rc::new(Cell::new(1u64));
        let builds = Rc::new(Cell::new(0usize));

        let data_reader = data.clone();
        let builds_counter = builds.clone();
        dynamic(
            move || data_reader.get(),
            move |_| {
                builds_counter.set(builds_counter.get() + 1);
                TestWidget
            },
        )
        .add_to_container(&mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 1);
        assert_eq!(builds.get(), 1);

        // Same data: widget kept, builder not called
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).clone(), first);
        assert_eq!(builds.get(), 1);

        // Changed data: builder runs, widget replaced
        data.set(2);
        source.reconcile_with_tracking(&mut tree);
        let second = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0], first[0]);
        assert_eq!(builds.get(), 2);
    }

    #[test]
    fn dynamic_child_builder_controls_presence() {
        let (mut tree, mut source) = children_host();
        let data = Rc::new(Cell::new(1u64));

        let data_reader = data.clone();
        dynamic(
            move || data_reader.get(),
            // Present only for even data
            |n| (n % 2 == 0).then_some(TestWidget),
        )
        .add_to_container(&mut source);

        assert!(source.reconcile_and_get(&mut tree).is_empty());

        data.set(2);
        source.reconcile_with_tracking(&mut tree);
        let present = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(present.len(), 1);

        data.set(3);
        source.reconcile_with_tracking(&mut tree);
        assert!(source.reconcile_and_get(&mut tree).is_empty());

        data.set(4);
        source.reconcile_with_tracking(&mut tree);
        let revived = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(revived.len(), 1);
        assert_ne!(revived[0], present[0]);
    }

    #[test]
    fn keyed_children_diff_by_identity_and_content() {
        let (mut tree, mut source) = children_host();
        let items = Rc::new(RefCell::new(vec![(1u64, "a"), (2, "b")]));
        let builds = Rc::new(Cell::new(0usize));

        let items_reader = items.clone();
        let builds_counter = builds.clone();
        keyed(
            move || items_reader.borrow().clone(),
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
        *items.borrow_mut() = vec![(2, "b"), (1, "a")];
        source.reconcile_with_tracking(&mut tree);
        let reordered = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(reordered, vec![first[1], first[0]]);
        assert_eq!(builds.get(), 2);

        // Content change on one item: only that row rebuilds
        *items.borrow_mut() = vec![(2, "B!"), (1, "a")];
        source.reconcile_with_tracking(&mut tree);
        let changed = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(changed.len(), 2);
        assert_ne!(changed[0], reordered[0]); // row 2 rebuilt
        assert_eq!(changed[1], reordered[1]); // row 1 untouched
        assert_eq!(builds.get(), 3);

        // Removal drops only the removed row
        *items.borrow_mut() = vec![(1, "a")];
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
