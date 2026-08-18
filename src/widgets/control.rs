//! The interaction unit a widget resolves its states from.
//!
//! When a text declares a hover style, hover *of what*? Not its own glyphs — a
//! button's label has to light up while the pointer is on the button's
//! padding, nowhere near them. Not any ancestor either — a label inside one
//! button must stay dark while the pointer is over a sibling button in the
//! same highlighted row.
//!
//! The answer is a boundary that is written down. A container marked
//! [`control`](crate::widgets::Container::control) is one interaction unit;
//! everything inside it resolves hover, press and focus from that unit, until
//! a nested one takes over. Every widget then asks the same question — *is my
//! control in this state?* — whatever the state is.
//!
//! Direction stops being a property of the mechanism and becomes only how a
//! control notices:
//!
//! - **hover, press** — the pointer is inside my bounds
//! - **focus** — the focus is inside my subtree
//!
//! Which is what makes the case that had no answer work: a label reacting to
//! the focus of a *sibling* input, the floating label of every form. The two
//! are in the same control, so the focus is the control's, and the label can
//! ask about it.
//!
//! # Nesting scopes resolution, not state
//!
//! Every control notices its own state independently: a list row is hovered
//! because the pointer is inside the row, full stop, even when the pointer is
//! over a button nested in it. What the nested control changes is only *who a
//! descendant asks* — the label inside the button asks the button, not the
//! row. The other reading, where a nested control blocks the outer one, would
//! switch the row's highlight off while the pointer is plainly still on it.

use crate::reactive::focus::focus_path;
use crate::reactive::signal::RwSignal;
use crate::tree::WidgetId;

use super::container::InteractionFlags;

/// A handle on the interaction unit a widget belongs to.
///
/// Reading any of these subscribes the caller, which is the point: a leaf that
/// asks whether its control is hovered is thereby repainted when it changes.
#[derive(Clone, Copy)]
pub struct Control {
    id: WidgetId,
    flags: RwSignal<InteractionFlags>,
}

impl Control {
    pub(crate) fn new(id: WidgetId, flags: RwSignal<InteractionFlags>) -> Self {
        Self { id, flags }
    }

    /// The container that declared the boundary.
    pub fn widget(&self) -> WidgetId {
        self.id
    }

    /// Whether the pointer is inside the control.
    pub fn is_hovered(&self) -> bool {
        self.flags.get().contains(InteractionFlags::HOVERED)
    }

    /// Whether the pointer is down on the control.
    pub fn is_pressed(&self) -> bool {
        self.flags.get().contains(InteractionFlags::PRESSED)
    }

    /// Whether the keyboard focus is inside the control's subtree.
    pub fn has_focus(&self) -> bool {
        focus_path().contains(self.id)
    }
}
