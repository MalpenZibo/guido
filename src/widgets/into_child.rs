use super::children::{ChildrenSource, DynItem};
use super::Widget;

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

// Implementation for dynamic closures
impl<F, W> IntoChild<DynamicChild> for F
where
    F: Fn() -> Option<W> + Send + Sync + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let child_fn = self;

        // Build a dynamic children source with this single optional child
        let items_fn = move || {
            if let Some(widget) = child_fn() {
                vec![DynItem::new(0, widget)]
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

// Implementation for dynamic children - Fn() -> I where I: IntoIterator<Item = (u64, W)>
// Adds a single dynamic slot with keyed reconciliation
impl<F, I, W> IntoChildren<DynamicChildren> for F
where
    F: Fn() -> I + Send + Sync + 'static,
    I: IntoIterator<Item = (u64, W)>,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let items_fn = move || {
            self()
                .into_iter()
                .map(|(key, widget)| DynItem::new(key, widget))
                .collect()
        };
        children_source.add_dynamic(items_fn);
    }
}
