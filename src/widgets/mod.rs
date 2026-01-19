pub mod column;
pub mod container;
pub mod row;
pub mod text;
pub mod widget;

pub use column::{column, Column};
pub use container::{container, Border, Container, GradientDirection, LinearGradient};
pub use row::{row, CrossAxisAlignment, MainAxisAlignment, Row};
pub use text::{text, Text};
pub use widget::{Color, Event, EventResponse, MouseButton, Padding, Rect, ScrollSource, Widget};

// IntoMaybeDyn implementations for widget types
use crate::reactive::{IntoMaybeDyn, MaybeDyn};

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
