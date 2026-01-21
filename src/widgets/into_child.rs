use super::children::{ChildrenSource, DynamicChildren, DynItem};
use super::Widget;

/// Trait for types that can be added as a child to a container
///
/// This trait allows `.child()` to accept both:
/// - Static widgets (evaluated once at creation)
/// - Dynamic closures returning Option<Widget> (reactive)
pub trait IntoChild {
    fn add_to_container(self, children_source: &mut ChildrenSource);
}

// Implementation for static widgets
impl<W: Widget + 'static> IntoChild for W {
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        match children_source {
            ChildrenSource::Static(children) => {
                children.push(Box::new(self));
            }
            ChildrenSource::Dynamic(_) => {
                panic!("Cannot add static child to container that already has dynamic children");
            }
        }
    }
}

/// Wrapper for dynamic child closures
///
/// Use this to make `.child()` accept a reactive closure:
/// ```ignore
/// container()
///     .child(text("Static"))
///     .child(dyn_child(move || {
///         if show.get() {
///             Some(text("Dynamic!"))
///         } else {
///             None
///         }
///     }))
/// ```
pub struct DynChild<F> {
    child_fn: F,
}

impl<F, W> IntoChild for DynChild<F>
where
    F: Fn() -> Option<W> + Send + Sync + 'static,
    W: Widget + 'static,
{
    fn add_to_container(self, children_source: &mut ChildrenSource) {
        let child_fn = self.child_fn;

        // Build a dynamic children source with this single optional child
        let items_fn = move || {
            if let Some(widget) = child_fn() {
                vec![DynItem::new(0, widget)]
            } else {
                vec![]
            }
        };

        match children_source {
            ChildrenSource::Static(existing) => {
                if !existing.is_empty() {
                    panic!(
                        "Cannot mix static and dynamic children. \
                        Add dynamic children before static children, \
                        or use .child_dyn() / .children_dyn() directly."
                    );
                }
                // Convert to dynamic mode
                *children_source = ChildrenSource::Dynamic(DynamicChildren::new(items_fn));
            }
            ChildrenSource::Dynamic(_) => {
                panic!(
                    "Cannot add multiple dynamic child sources. \
                    Use .children_dyn() for multiple dynamic children."
                );
            }
        }
    }
}

/// Create a dynamic child from a closure
///
/// # Example
/// ```ignore
/// let show = create_signal(true);
/// container()
///     .child(dyn_child(move || {
///         if show.get() {
///             Some(text("I'm reactive!"))
///         } else {
///             None
///         }
///     }))
/// ```
pub fn dyn_child<F, W>(child_fn: F) -> DynChild<F>
where
    F: Fn() -> Option<W> + Send + Sync + 'static,
    W: Widget + 'static,
{
    DynChild { child_fn }
}
