pub mod children;
pub mod container;
pub mod font;
pub mod image;
pub mod into_child;
pub mod scroll;
pub mod state_layer;
pub mod text;
pub mod text_input;
pub mod widget;

pub use children::ChildrenSource;
pub use container::{Border, Container, GradientDirection, LinearGradient, Overflow, container};
pub use font::{FontFamily, FontWeight};
pub use image::{ContentFit, Image, ImageSource, image};
pub use into_child::{DynamicChildren, IntoChild, IntoChildren, StaticChildren};
pub use scroll::{ScrollAxis, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility};
pub use state_layer::{BackgroundOverride, RippleConfig, StateStyle};
pub use text::{Text, text};
pub use text_input::{Selection, TextInput, text_input};
pub use widget::{
    Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding, Rect,
    ScrollSource, Widget,
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

impl IntoMaybeDyn<FontFamily> for FontFamily {
    fn into_maybe_dyn(self) -> MaybeDyn<FontFamily> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<FontWeight> for FontWeight {
    fn into_maybe_dyn(self) -> MaybeDyn<FontWeight> {
        MaybeDyn::Static(self)
    }
}
