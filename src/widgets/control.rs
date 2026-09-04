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
use crate::reactive::signal::{OptionSignalExt, RwSignal, Signal};
use crate::tree::WidgetId;

use super::container::InteractionFlags;
use super::state_layer::StateWhen;

/// A handle on the interaction unit a widget belongs to.
///
/// Reading any of these subscribes the caller, which is the point: a leaf that
/// asks whether its control is hovered is thereby repainted when it changes.
#[derive(Clone, Copy)]
pub struct Control {
    id: WidgetId,
    flags: RwSignal<InteractionFlags>,
    /// Whether this unit takes input, with its ancestors already folded in —
    /// see [`Container::enabled`](crate::widgets::Container::enabled).
    ///
    /// `None` where nothing at or above this unit was declared with one, which
    /// is nearly every control there is and costs no signal at all.
    enabled: Option<Signal<bool>>,
}

impl Control {
    pub(crate) fn new(
        id: WidgetId,
        flags: RwSignal<InteractionFlags>,
        enabled: Option<Signal<bool>>,
    ) -> Self {
        Self { id, flags, enabled }
    }

    /// The container that declared the boundary.
    pub fn widget(&self) -> WidgetId {
        self.id
    }

    /// Whether the pointer is inside the control.
    ///
    /// A disabled unit is never hovered and never pressed: it refuses the
    /// pointer, so it is not under one. The declaration is read first, so a
    /// control that is off does not subscribe to flags it would ignore.
    pub fn is_hovered(&self) -> bool {
        self.is_enabled() && self.flags.get().contains(InteractionFlags::HOVERED)
    }

    /// Whether the pointer is down on the control.
    pub fn is_pressed(&self) -> bool {
        self.is_enabled() && self.flags.get().contains(InteractionFlags::PRESSED)
    }

    /// Whether the keyboard focus is inside the control's subtree.
    ///
    /// The path, rather than a walk of the unit's descendants: the same
    /// question has to be answerable from a `create_derived` closure, which has
    /// no tree, and that is where a container resolves the text colour it
    /// publishes below it.
    ///
    /// Gated on the declaration, like hover and press: a disabled unit shows no
    /// focus ring, which is what Qt, GTK and a `<fieldset disabled>` all show.
    /// A field that must go on saying the keyboard is *its* is not disabled —
    /// it is [read-only](crate::widgets::TextInput::readonly), which is a
    /// different thing and keeps everything here.
    pub fn has_focus(&self) -> bool {
        self.is_enabled() && focus_path().contains(self.id)
    }

    /// Whether this unit, and every unit above it, takes input.
    pub fn is_enabled(&self) -> bool {
        self.enabled.get_or(true)
    }

    /// The complement, which is how a state layer reads it.
    pub fn is_disabled(&self) -> bool {
        !self.is_enabled()
    }

    /// The folded answer itself, so a nested unit can fold its own into it.
    pub(crate) fn enabled_signal(&self) -> Option<Signal<bool>> {
        self.enabled
    }

    /// Whether a state layer with this trigger applies right now.
    ///
    /// The single statement of what each [`StateWhen`] means. Everything that
    /// resolves a state layer against a unit goes through here — the container
    /// its own layers, a `Text` or a `TextInput` the layers it declares — so
    /// "a disabled unit is never hovered" is written once rather than once per
    /// widget that could disagree about it.
    ///
    /// Reading the answer is what subscribes the caller, which is why it is
    /// asked only for a layer that declares something about the property being
    /// resolved.
    pub(crate) fn is_active(&self, when: &StateWhen) -> bool {
        match when {
            StateWhen::Hovered => self.is_hovered(),
            StateWhen::Pressed => self.is_pressed(),
            StateWhen::Focused => self.has_focus(),
            StateWhen::Disabled => self.is_disabled(),
            StateWhen::When(condition) => condition.get(),
        }
    }
}
