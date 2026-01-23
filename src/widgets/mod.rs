pub mod children;
pub mod container;
pub mod into_child;
pub mod text;
pub mod widget;

pub use children::ChildrenSource;
pub use container::{container, Border, Container, GradientDirection, LinearGradient, Overflow};
pub use into_child::{DynamicChildren, IntoChild, IntoChildren, StaticChildren};
pub use text::{text, Text};
pub use widget::{Color, Event, EventResponse, MouseButton, Padding, Rect, ScrollSource, Widget};

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
