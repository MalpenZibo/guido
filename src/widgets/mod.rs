pub mod children;
pub mod container;
pub mod control;
mod corners;
pub mod font;
pub mod image;
pub mod input_style;
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
pub use input_style::{InputStyle, InputStyled};
pub use into_child::{
    DynamicChildren, IntoChild, IntoChildren, IntoDynChild, KeyedChildren, StaticChildren, keyed,
};
pub use scroll::{ScrollAxis, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility};
pub use state_layer::{
    BackgroundOverride, BorderOverride, RippleConfig, StateStyle, StateWhen, Stateful,
};
pub use text::{Text, text};
pub use text_input::{Selection, TextInput, text_input};
pub use text_style::{TextShadow, TextStroke, TextStyle, TextStyled};
pub use widget::{
    AnyWidget, Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding,
    Rect, ScrollSource, Widget,
};

// IntoVal<Padding> impls for closures returning numeric types
use crate::reactive::IntoVal;

// And for a closure that returns a bare size where a corner shape is wanted.
impl IntoVal<Corners> for f32 {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for f64 {
    fn into_val(self) -> Corners {
        Corners::from(self as f32)
    }
}

impl IntoVal<Corners> for i32 {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for u16 {
    fn into_val(self) -> Corners {
        self.into()
    }
}

impl IntoVal<Corners> for (f32, f32, f32, f32) {
    fn into_val(self) -> Corners {
        self.into()
    }
}

impl IntoVal<Corners> for u32 {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for [i32; 2] {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for [i32; 4] {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for [f32; 2] {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

impl IntoVal<Corners> for [f32; 4] {
    fn into_val(self) -> Corners {
        Corners::from(self)
    }
}

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

impl IntoVal<Padding> for [f32; 2] {
    fn into_val(self) -> Padding {
        self.into()
    }
}

impl IntoVal<Padding> for [f32; 4] {
    fn into_val(self) -> Padding {
        self.into()
    }
}

impl IntoVal<Padding> for [i32; 2] {
    fn into_val(self) -> Padding {
        self.into()
    }
}

impl IntoVal<Padding> for [i32; 4] {
    fn into_val(self) -> Padding {
        self.into()
    }
}

impl IntoVal<Padding> for f64 {
    fn into_val(self) -> Padding {
        Padding::from(self as f32)
    }
}

// A signal accepts what a closure returning the same type accepts. The lists
// mirror the `IntoVal` impls above one for one; see `reactive::into_signal`
// for why they cannot be one blanket impl.
crate::reactive::converting_signals!(
    f32 => Corners,
    f64 => Corners,
    i32 => Corners,
    u32 => Corners,
    [f32; 2] => Corners,
    [f32; 4] => Corners,
    [i32; 2] => Corners,
    [i32; 4] => Corners,
    u16 => Corners,
    (f32, f32, f32, f32) => Corners,

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
