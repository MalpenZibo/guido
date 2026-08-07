use crate::reactive::with_owner;

use super::Widget;
use super::children::{ChildrenSource, DynItem, OwnedWidget};

/// Marker type for static child (widget value)
pub struct StaticChild;

/// Marker type for dynamic child (closure)
pub struct DynamicChild;

/// Marker type for keyed dynamic child (closure returning `Option<(key, widget)>`)
pub struct KeyedChild;

/// Trait for types that can be added as a child to a container
///
/// This trait uses a marker type parameter to disambiguate between:
/// - Static widgets (evaluated once at creation) - uses `StaticChild` marker
/// - Dynamic closures returning `Option<Widget>` (presence-reactive) - uses `DynamicChild` marker
/// - Dynamic closures returning `Option<(u64, Widget)>` (content-reactive) - uses `KeyedChild` marker
///
/// The marker parameter defaults to `StaticChild` so `.child(widget)` works without annotation.
pub trait IntoChild<Marker = StaticChild> {
    fn add_to_container(self, children_source: &mut ChildrenSource);
}

// Implementation for static widgets
impl<W: Widget + 'static> IntoChild<StaticChild> for W {
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        children_source.add_static(Box::new(self));
    }
}

/// Implementation for dynamic closures returning `Option<Widget>`.
///
/// **This form is presence-only.** The child always reconciles under the same
/// key, so `Some -> Some` transitions keep the FIRST widget ever built and
/// discard the rebuilt one — signals and state inside it persist, but a
/// widget with different structure will never appear. Use it to show/hide a
/// widget whose own properties are reactive.
///
/// When the widget's *structure* depends on data (lists, branches, rebuilt
/// trees), return `Option<(key, widget)>` instead — see [`KeyedChild`] — with
/// a key that changes alongside the content (typically a hash of the data).
impl<F, W> IntoChild<DynamicChild> for F
where
    F: Fn() -> Option<W> + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let child_fn = std::rc::Rc::new(self);

        let items_fn = move || {
            let child_fn = child_fn.clone();
            if let Some(widget) = child_fn() {
                // For single optional child, wrap in owner at creation time
                vec![DynItem::new(0, move || {
                    let (widget, owner_id) = with_owner(|| widget);
                    OwnedWidget::new(Box::new(widget), owner_id)
                })]
            } else {
                vec![]
            }
        };

        children_source.add_dynamic(items_fn);
    }
}

/// Implementation for dynamic closures returning `Option<(u64, Widget)>`.
///
/// The content-reactive companion to the presence-only `Option<Widget>` form:
/// the key states which version of the content the widget renders. Same key =
/// the cached widget is kept (rebuilt value discarded, inner state persists);
/// new key = the old widget is dropped (owner cleanup runs) and the rebuilt
/// one is swapped in.
///
/// # Example
///
/// ```ignore
/// let entries = create_signal(vec!["a".to_string()]);
/// container().child(move || {
///     let list = entries.get();
///     let key = hash_of(&list); // any u64 that changes with the content
///     Some((key, build_list_widget(list)))
/// })
/// ```
impl<F, W> IntoChild<KeyedChild> for F
where
    F: Fn() -> Option<(u64, W)> + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let child_fn = std::rc::Rc::new(self);

        let items_fn = move || {
            let child_fn = child_fn.clone();
            if let Some((key, widget)) = child_fn() {
                vec![DynItem::new(key, move || {
                    let (widget, owner_id) = with_owner(|| widget);
                    OwnedWidget::new(Box::new(widget), owner_id)
                })]
            } else {
                vec![]
            }
        };

        children_source.add_dynamic(items_fn);
    }
}

/// Marker type for static children (iterator of widgets)
pub struct StaticChildren;

/// Marker type for dynamic children (closure returning keyed items)
pub struct DynamicChildren;

/// Trait for types that can be added as children to a container
///
/// This trait uses a marker type parameter to disambiguate between:
/// - Static children (iterator of widgets) - uses `StaticChildren` marker
/// - Dynamic children (closure returning keyed items) - uses `DynamicChildren` marker
///
/// The marker parameter defaults to `StaticChildren` so `.children([...])` works without annotation.
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

/// Implementation for dynamic children with closures.
///
/// Accepts `Fn() -> Iterator<Item = (key, FnOnce() -> Widget)>`.
///
/// Each child's closure runs inside an owner scope, so signals and effects
/// created during widget construction are automatically owned and cleaned up
/// when the child is removed.
///
/// **IMPORTANT:** The widget closure is only called for NEW keys. Existing keys
/// reuse their cached widgets, so signals/effects persist across frames.
///
/// # Example
///
/// ```ignore
/// let items = create_signal(vec![1, 2, 3]);
/// container().children(move || {
///     items.get().iter().map(|&id| {
///         (id as u64, move || {
///             text(format!("Item {}", id))
///         })
///     })
/// })
/// ```
impl<F, I, G, W> IntoChildren<DynamicChildren> for F
where
    F: Fn() -> I + 'static,
    I: IntoIterator<Item = (u64, G)>,
    G: FnOnce() -> W + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let items_fn = move || {
            self()
                .into_iter()
                .map(|(key, widget_fn)| {
                    // Return DynItem with a LAZY widget factory.
                    // The closure is only called by reconciliation for NEW keys.
                    // with_owner wraps the widget creation for automatic cleanup.
                    DynItem::new(key, move || {
                        let (widget, owner_id) = with_owner(widget_fn);
                        OwnedWidget::new(Box::new(widget), owner_id)
                    })
                })
                .collect()
        };
        children_source.add_dynamic(items_fn);
    }
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
    fn keyed_child_swaps_widget_when_key_changes() {
        let (mut tree, mut source) = children_host();
        let key = Rc::new(Cell::new(1u64));

        let key_reader = key.clone();
        let closure = move || Some((key_reader.get(), TestWidget));
        IntoChild::<KeyedChild>::add_to_container(closure, &mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 1);

        // Same key: the cached widget must be kept
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).clone(), first);

        // New key: the widget must be replaced
        key.set(2);
        source.reconcile_with_tracking(&mut tree);
        let second = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0], first[0]);
    }

    #[test]
    fn option_child_is_presence_only() {
        let (mut tree, mut source) = children_host();
        let present = Rc::new(Cell::new(true));

        let present_reader = present.clone();
        let closure = move || present_reader.get().then_some(TestWidget);
        IntoChild::<DynamicChild>::add_to_container(closure, &mut source);

        let first = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(first.len(), 1);

        // Some -> Some keeps the original widget: this form cannot express
        // content changes, only presence
        source.reconcile_with_tracking(&mut tree);
        assert_eq!(source.reconcile_and_get(&mut tree).clone(), first);

        // None removes it, Some builds a fresh one
        present.set(false);
        source.reconcile_with_tracking(&mut tree);
        assert!(source.reconcile_and_get(&mut tree).is_empty());

        present.set(true);
        source.reconcile_with_tracking(&mut tree);
        let revived = source.reconcile_and_get(&mut tree).clone();
        assert_eq!(revived.len(), 1);
        assert_ne!(revived[0], first[0]);
    }
}
