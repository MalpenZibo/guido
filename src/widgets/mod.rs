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
    AnyWidget, Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding,
    Rect, ScrollSource, Widget,
};

// IntoVal<Padding> impls for closures returning numeric types
use crate::reactive::IntoVal;

impl IntoVal<Padding> for i32 {
    fn into_val(self) -> Padding {
        Padding::from(self)
    }
}

impl IntoVal<Padding> for u32 {
    fn into_val(self) -> Padding {
        Padding::from(self)
    }
}

impl IntoVal<Padding> for u16 {
    fn into_val(self) -> Padding {
        Padding::from(self)
    }
}

impl IntoVal<Padding> for f32 {
    fn into_val(self) -> Padding {
        Padding::from(self)
    }
}
