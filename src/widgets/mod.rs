pub mod children;
pub mod container;
pub mod image;
pub mod into_child;
pub mod scroll;
pub mod state_layer;
pub mod text;
pub mod text_input;
pub mod widget;

/// Macro to implement common dirty flag methods for simple widgets.
///
/// Container keeps its custom implementation because it recurses to children.
macro_rules! impl_dirty_flags {
    () => {
        fn mark_dirty(&mut self, flags: crate::reactive::ChangeFlags) {
            self.dirty_flags |= flags;
        }
        fn needs_layout(&self) -> bool {
            self.dirty_flags
                .contains(crate::reactive::ChangeFlags::NEEDS_LAYOUT)
        }
        fn needs_paint(&self) -> bool {
            self.dirty_flags
                .contains(crate::reactive::ChangeFlags::NEEDS_PAINT)
        }
        fn clear_dirty(&mut self) {
            self.dirty_flags = crate::reactive::ChangeFlags::empty();
        }
    };
}
pub(crate) use impl_dirty_flags;

pub use children::ChildrenSource;
pub use container::{container, Border, Container, GradientDirection, LinearGradient, Overflow};
pub use image::{image, ContentFit, Image, ImageSource};
pub use into_child::{DynamicChildren, IntoChild, IntoChildren, StaticChildren};
pub use scroll::{ScrollAxis, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility};
pub use state_layer::{BackgroundOverride, RippleConfig, StateStyle};
pub use text::{text, Text};
pub use text_input::{text_input, Selection, TextInput};
pub use widget::{
    Color, Event, EventResponse, Key, Modifiers, MouseButton, Padding, Rect, ScrollSource, Widget,
};

// IntoMaybeDyn implementations for widget types
use crate::reactive::{IntoMaybeDyn, MaybeDyn};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;

impl IntoMaybeDyn<Color> for Color {
    fn into_maybe_dyn(self) -> MaybeDyn<Color> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<Padding> for Padding {
    fn into_maybe_dyn(self) -> MaybeDyn<Padding> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<Transform> for Transform {
    fn into_maybe_dyn(self) -> MaybeDyn<Transform> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<TransformOrigin> for TransformOrigin {
    fn into_maybe_dyn(self) -> MaybeDyn<TransformOrigin> {
        MaybeDyn::Static(self)
    }
}
