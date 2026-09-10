pub mod children;
pub mod container;
pub mod control;
mod corners;
pub mod font;
pub mod image;
pub mod into_child;
mod paint_children;

#[cfg(test)]
mod diagnostic_audit;
pub mod scroll;
pub mod state_layer;
pub mod text;
pub mod text_input;
pub mod text_style;
pub mod widget;

pub use crate::renderer::CornerRadii;
pub use children::ChildrenSource;
pub use container::{
    Border, Container, GradientDirection, IntoClickHandler, LinearGradient, Overflow, container,
};
pub use control::Control;
pub use corners::Corners;
pub use font::{FontFamily, FontWeight};
pub use image::{ContentFit, Image, ImageSource, image};
pub use into_child::{
    DynamicChildren, ForwardedChildren, IntoChild, IntoChildren, IntoDynChild, KeyedChildren,
    StaticChildren, keyed,
};
pub use scroll::{Scroll, ScrollbarVisibility};
pub use state_layer::{
    BackgroundOverride, BorderOverride, RippleConfig, StateStyle, StateWhen, Stateful,
};
pub use text::{Text, text};
pub use text_input::{Selection, TextInput, text_input};
pub use text_style::{TextShadow, TextStroke, TextStyle};
pub use widget::{
    AnyWidget, Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding,
    Point, Rect, ScrollSource, Widget,
};

// Every conversion a corner shape or a padding accepts, declared once. The
// value form is the `From` beside each type; these are the other four. See
// `reactive::into_signal` for why the signal forms name their source type.
crate::reactive::converts!(
    f32 => Corners,
    f64 => Corners,
    i32 => Corners,
    u32 => Corners,
    u16 => Corners,
    [f32; 2] => Corners,
    [f32; 4] => Corners,
    [i32; 2] => Corners,
    [i32; 4] => Corners,
    (f32, f32, f32, f32) => Corners,
    crate::renderer::CornerRadii => Corners,

    f32 => Padding,
    f64 => Padding,
    i32 => Padding,
    u32 => Padding,
    u16 => Padding,
    [f32; 2] => Padding,
    [f32; 4] => Padding,
    [i32; 2] => Padding,
    [i32; 4] => Padding,
);
