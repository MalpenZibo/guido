//! Clipboard support for text copy/paste operations.
//!
//! A copy is handed to the loop, which gives it to the compositor. A paste is
//! a request: nothing of another application's selection is read until
//! something asks for it, and what is read goes to whoever asked and nowhere
//! else. Holding a copy of every offer, the way a prefetch does, keeps each
//! password the user copies in this process whether or not anything here ever
//! pastes it. GTK and smithay-clipboard read on demand for the same reason.
//!
//! The answer arrives on a later loop iteration, because reading means waiting
//! on the application that owns the selection, and that never happens on the
//! UI thread. The queues are fields of the application's state, in
//! `src/app_state.rs`.

use std::fmt;
use std::rc::Rc;

use crate::Secret;
use crate::app_state::with_app_state;
use crate::tree::{Tree, WidgetId};
use crate::widgets::Event;

/// Which system selection a paste reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// The regular clipboard (Ctrl+C / Ctrl+V).
    Clipboard,
    /// The primary selection (select-to-copy / middle-click paste).
    Primary,
}

/// Text a paste brought to a widget.
///
/// What arrives in [`Event::Pasted`]. Pasting a password from a manager is the
/// ordinary case, so the text is held in a [`Secret`] — wiped when the last
/// widget it was sent to lets go of it, and never printed by `Debug`. Every
/// paste answered by one read shares it.
#[derive(Clone)]
pub struct PastedText(Rc<Secret>);

impl PastedText {
    /// The text.
    pub fn as_str(&self) -> &str {
        self.0.expose()
    }
}

impl fmt::Debug for PastedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PastedText(..)")
    }
}

/// Who a paste is for.
pub(crate) enum PasteTarget {
    /// Application code, answered through its callback.
    Callback(Box<dyn FnOnce(String)>),
    /// A widget, answered with [`Event::Pasted`].
    Widget(WidgetId),
}

/// Copy text to the clipboard.
pub fn clipboard_copy(text: &str) {
    // Setting the slot is what wakes the loop that hands the copy over.
    with_app_state(|app| app.outgoing_clipboard.set(text.to_string()));
}

/// Take the copy waiting to go out to the compositor, if any.
pub fn take_clipboard_change() -> Option<String> {
    with_app_state(|app| app.outgoing_clipboard.take())
}

/// Paste from the clipboard.
///
/// `on_text` runs on the UI thread once the application that owns the
/// clipboard has answered, on a later loop iteration. It is not called when
/// there is nothing to paste or the read fails. The `String` is yours: if it
/// may hold a secret, wipe it when you are done.
pub fn clipboard_paste(on_text: impl FnOnce(String) + 'static) {
    request(
        SelectionKind::Clipboard,
        PasteTarget::Callback(Box::new(on_text)),
    );
}

/// Check if clipboard has content
pub fn clipboard_has_content() -> bool {
    with_app_state(|app| app.clipboard_offered.get())
}

/// Copy text to the primary selection (select-to-copy).
pub fn primary_copy(text: &str) {
    with_app_state(|app| app.outgoing_primary.set(text.to_string()));
}

/// Take the select-to-copy waiting to go out to the compositor, if any.
#[doc(hidden)]
pub fn take_primary_change() -> Option<String> {
    with_app_state(|app| app.outgoing_primary.take())
}

/// Paste from the primary selection (middle-click paste). Answered the way
/// [`clipboard_paste`] is.
pub fn primary_paste(on_text: impl FnOnce(String) + 'static) {
    request(
        SelectionKind::Primary,
        PasteTarget::Callback(Box::new(on_text)),
    );
}

/// Paste into a widget: the text comes back to it as [`Event::Pasted`].
pub(crate) fn paste_into(kind: SelectionKind, widget: WidgetId) {
    request(kind, PasteTarget::Widget(widget));
}

fn request(kind: SelectionKind, target: PasteTarget) {
    with_app_state(|app| app.paste_requests.push((kind, target)));
}

/// Record whether the compositor offers a clipboard to paste (called from
/// Wayland event handling).
pub(crate) fn set_clipboard_offered(offered: bool) {
    with_app_state(|app| app.clipboard_offered.set(offered));
}

/// The pastes asked for since the last call, one read per kind: a token for
/// each kind somebody asked for, to answer with [`answer_paste`].
pub(crate) fn take_paste_requests() -> Vec<(SelectionKind, u64)> {
    let requests = with_app_state(|app| app.paste_requests.drain());
    if requests.is_empty() {
        return Vec::new();
    }
    let (clipboard, primary): (Vec<_>, Vec<_>) = requests
        .into_iter()
        .partition(|(kind, _)| *kind == SelectionKind::Clipboard);
    with_app_state(|app| {
        let mut reads = app.paste_reads.borrow_mut();
        [
            (SelectionKind::Clipboard, clipboard),
            (SelectionKind::Primary, primary),
        ]
        .into_iter()
        .filter(|(_, asked)| !asked.is_empty())
        .map(|(kind, asked)| {
            let token = app.next_paste_read.get();
            app.next_paste_read.set(token + 1);
            reads.push((token, asked.into_iter().map(|(_, target)| target).collect()));
            (kind, token)
        })
        .collect()
    })
}

/// Queue what the read for `token` brought for everyone who asked, to be
/// handed over by [`deliver_pastes`]. Called by the platform when the read
/// comes back; the `String` it was read into is wiped here.
pub(crate) fn answer_paste(token: u64, text: Option<String>) {
    // Into a `Secret` first, so a read nobody waits for any more is wiped too.
    let text = text.map(Secret::from);
    with_app_state(|app| {
        let mut reads = app.paste_reads.borrow_mut();
        let Some(at) = reads.iter().position(|(t, _)| *t == token) else {
            return;
        };
        let targets = reads.swap_remove(at).1;
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        let text = PastedText(Rc::new(text));
        for target in targets {
            app.pastes.push((target, text.clone()));
        }
    });
}

/// Answer every paste asked for so far with `text`, as the platform does when
/// a read comes back. Public so a test with no platform can play it.
#[doc(hidden)]
pub fn answer_pastes(kind: SelectionKind, text: Option<&str>) {
    for (asked, token) in take_paste_requests() {
        let text = if asked == kind { text } else { None };
        answer_paste(token, text.map(str::to_owned));
    }
}

/// Hand the pastes that have arrived to whoever asked for them: a callback is
/// called, a widget is sent [`Event::Pasted`]. The loop calls it once per
/// iteration; public so a test with no loop can.
#[doc(hidden)]
pub fn deliver_pastes(tree: &mut Tree) {
    let pastes = with_app_state(|app| app.pastes.drain());
    if pastes.is_empty() {
        return;
    }
    tree.set_event_instant(Some(std::time::Instant::now()));
    crate::reactive::diagnostics::snapshot_zone(|| {
        for (target, text) in pastes {
            match target {
                PasteTarget::Callback(on_text) => on_text(text.as_str().to_owned()),
                PasteTarget::Widget(id) => {
                    let event = Event::Pasted(text);
                    tree.with_widget_mut(id, |widget, id, tree| widget.event(tree, id, &event));
                }
            }
        }
    });
    tree.set_event_instant(None);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    type Got = Rc<RefCell<Vec<String>>>;

    /// A paste callback that records what it was handed into `got`.
    fn record(got: &Got) -> impl FnOnce(String) + 'static {
        let got = got.clone();
        move |text| got.borrow_mut().push(text)
    }

    #[test]
    fn each_kind_asked_for_is_one_read() {
        answer_pastes(SelectionKind::Clipboard, None);
        clipboard_paste(|_| {});
        primary_paste(|_| {});
        clipboard_paste(|_| {});

        let reads = take_paste_requests();
        assert_eq!(reads.len(), 2, "{reads:?}");
        assert_ne!(reads[0].1, reads[1].1, "each read has its own token");
        assert!(
            take_paste_requests().is_empty(),
            "and nothing is asked twice"
        );
    }

    #[test]
    fn a_read_goes_to_everyone_who_asked_and_nobody_else() {
        let got = Got::default();
        clipboard_paste(record(&got));
        clipboard_paste(record(&got));
        let reads = take_paste_requests();
        clipboard_paste(record(&got));

        answer_paste(reads[0].1, Some("copied".into()));
        deliver_pastes(&mut Tree::new());
        assert_eq!(*got.borrow(), ["copied", "copied"]);

        answer_pastes(SelectionKind::Clipboard, None);
        deliver_pastes(&mut Tree::new());
    }

    #[test]
    fn nothing_to_paste_calls_nobody() {
        let got = Got::default();
        clipboard_paste(record(&got));
        primary_paste(record(&got));
        for (_, token) in take_paste_requests() {
            answer_paste(token, None);
        }
        clipboard_paste(record(&got));
        answer_pastes(SelectionKind::Clipboard, Some(""));
        deliver_pastes(&mut Tree::new());

        assert!(got.borrow().is_empty(), "{:?}", got.borrow());
    }

    /// The token a read from a finished application comes back with means
    /// nothing to this one.
    #[test]
    fn an_answer_nobody_waits_for_is_dropped() {
        let got = Got::default();
        clipboard_paste(record(&got));
        let reads = take_paste_requests();

        answer_paste(reads[0].1 + 1, Some("stale".into()));
        deliver_pastes(&mut Tree::new());
        assert!(got.borrow().is_empty());

        answer_paste(reads[0].1, Some("fresh".into()));
        deliver_pastes(&mut Tree::new());
        assert_eq!(*got.borrow(), ["fresh"]);
    }

    #[test]
    fn a_paste_for_a_widget_that_is_gone_is_dropped() {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(crate::widgets::container()));
        tree.unregister(id);
        paste_into(SelectionKind::Clipboard, id);

        answer_pastes(SelectionKind::Clipboard, Some("text"));
        deliver_pastes(&mut tree);
        assert!(with_app_state(|app| app.pastes.is_empty()));
    }

    #[test]
    fn pasted_text_does_not_print_itself() {
        let text = PastedText(Rc::new(Secret::from(String::from("hunter2"))));
        assert_eq!(format!("{text:?}"), "PastedText(..)");
        assert_eq!(text.as_str(), "hunter2");
    }
}
