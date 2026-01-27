use crate::reactive::with_owner;

use super::Widget;
use super::children::{ChildrenSource, DynItem, OwnedWidget};

/// Marker type for static child (widget value)
pub struct StaticChild;

/// Marker type for dynamic child (closure)
pub struct DynamicChild;

/// Trait for types that can be added as a child to a container
///
/// This trait uses a marker type parameter to disambiguate between:
/// - Static widgets (evaluated once at creation) - uses `StaticChild` marker
/// - Dynamic closures returning Option<Widget> (reactive) - uses `DynamicChild` marker
///
/// The marker parameter defaults to `StaticChild` for backwards compatibility.
pub trait IntoChild<Marker = StaticChild> {
    fn add_to_container(self, children_source: &mut ChildrenSource);
}

// Implementation for static widgets
impl<W: Widget + 'static> IntoChild<StaticChild> for W {
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        children_source.add_static(Box::new(self));
    }
}

// Implementation for dynamic closures returning Option<Widget>
// Note: For single optional children, ownership is handled at the item level.
// Use keyed .children() for proper ownership with dynamic lists.
impl<F, W> IntoChild<DynamicChild> for F
where
    F: Fn() -> Option<W> + Send + Sync + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let child_fn = std::sync::Arc::new(self);

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
/// The marker parameter defaults to `StaticChildren` for backwards compatibility.
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

// Implementation for dynamic children with closures
// Fn() -> Iterator<Item = (key, FnOnce() -> Widget)>
//
// Each child's closure runs inside an owner scope, so signals and effects
// created during widget construction are automatically owned and cleaned up
// when the child is removed.
//
// IMPORTANT: The widget closure is only called for NEW keys. Existing keys
// reuse their cached widgets, so signals/effects persist across frames.
//
// Example:
// ```
// .children(move || {
//     items.get().iter().map(|item| {
//         (item.id, move || {
//             let signal = create_signal(0);  // Owned by this child!
//             create_child(item, signal)
//         })
//     })
// })
// ```
impl<F, I, G, W> IntoChildren<DynamicChildren> for F
where
    F: Fn() -> I + Send + Sync + 'static,
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
