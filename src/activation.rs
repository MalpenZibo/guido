//! Activation tokens, asked for by application code and answered by the
//! compositor.
//!
//! A token is what lets another application take the focus on Wayland: the
//! compositor hands one out only against the serial of a real input event, so
//! it has to be asked for by the client that received the event, with that
//! event's serial — the toolkit, not the code handling the click. The answer
//! comes a round trip later, so a request is a callback, run on a later loop
//! iteration the way a paste's is. The queues are fields of the application's
//! state, in `src/app_state.rs`.

use crate::app_state::with_app_state;
use crate::surface::SurfaceId;

/// What a request for a token is answered through.
pub(crate) type TokenCallback = Box<dyn FnOnce(Option<String>)>;

/// Ask for a token for `surface`, answered through `done`.
pub(crate) fn request(surface: SurfaceId, done: TokenCallback) {
    with_app_state(|app| {
        let request = app.activation_waits.issue(done);
        app.activation_requests.push((surface, request));
    });
}

/// The tokens asked for since the last call: the surface each is for and the
/// request its answer comes back under, to answer with [`answer`].
pub(crate) fn take_requests() -> Vec<(SurfaceId, u64)> {
    with_app_state(|app| app.activation_requests.drain())
}

/// Queue what the compositor answered `request` with, to be handed over by
/// [`deliver`]. Called by the platform when the token arrives.
pub(crate) fn answer(request: u64, token: Option<String>) {
    with_app_state(|app| {
        if let Some(done) = app.activation_waits.resolve(request) {
            app.activation_answers.push((done, token));
        }
    });
}

/// Hand the tokens that have arrived to whoever asked for them. The loop calls
/// it once per iteration.
pub(crate) fn deliver() {
    let answers = with_app_state(|app| app.activation_answers.drain());
    if answers.is_empty() {
        return;
    }
    crate::reactive::diagnostics::snapshot_zone(|| {
        for (done, token) in answers {
            done(token);
        }
    });
}
