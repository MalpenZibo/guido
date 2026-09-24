//! The two system selections: the clipboard and the primary selection.
//!
//! Both work the same way, so they are kept together. An offer arriving is
//! only recorded — sctk keeps it, and the application is told that one exists.
//! Its content is read when something pastes: a reader thread pulls it through
//! a pipe, and the result comes back on the calloop ingress channel to whoever
//! asked. Reading means blocking on a pipe until the owning application
//! answers, which is why it never happens on the UI thread.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    data_device_manager::{
        DataDeviceManagerState, ReadPipe,
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::DataOfferHandler,
        data_source::{CopyPasteSource, DataSourceHandler},
    },
    primary_selection::{
        PrimarySelectionManagerState,
        device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler},
        selection::{PrimarySelectionSource, PrimarySelectionSourceHandler},
    },
};
use zeroize::{Zeroize, Zeroizing};

use smithay_client_toolkit::reexports::client::{
    Connection, QueueHandle,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_seat, wl_surface,
    },
};

use super::wayland::WaylandState;
pub use crate::reactive::SelectionKind;
use crate::reactive::clipboard::{answer_paste, set_clipboard_offered};

/// The text types a selection is read as, most preferred first.
const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "TEXT",
    "STRING",
];

/// The first of an offer's mime types a selection can be read as.
fn text_mime(mimes: &[String]) -> Option<&'static str> {
    TEXT_MIMES
        .iter()
        .find(|m| mimes.iter().any(|t| t == *m))
        .copied()
}

/// Everything the two selections own.
///
/// The managers are bound once at startup and are `None` on a compositor that
/// does not advertise the protocol — the feature then no-ops rather than
/// failing. The devices arrive later, with the seat.
pub struct Selections {
    pub(super) data_device_manager: Option<DataDeviceManagerState>,
    pub(super) data_device: Option<DataDevice>,
    pub(super) clipboard_content: Option<String>,
    pub(super) clipboard_source: Option<CopyPasteSource>,

    pub(super) primary_selection_manager: Option<PrimarySelectionManagerState>,
    pub(super) primary_selection_device: Option<PrimarySelectionDevice>,
    pub(super) primary_content: Option<String>,
    pub(super) primary_source: Option<PrimarySelectionSource>,
}

impl Selections {
    pub(super) fn new(
        data_device_manager: Option<DataDeviceManagerState>,
        primary_selection_manager: Option<PrimarySelectionManagerState>,
    ) -> Self {
        Self {
            data_device_manager,
            data_device: None,
            clipboard_content: None,
            clipboard_source: None,
            primary_selection_manager,
            primary_selection_device: None,
            primary_content: None,
            primary_source: None,
        }
    }

    /// What guido itself is offering as `kind`, while it still owns it.
    fn own_content(&self, kind: SelectionKind) -> Option<&String> {
        match kind {
            SelectionKind::Clipboard => self
                .clipboard_source
                .as_ref()
                .and(self.clipboard_content.as_ref()),
            SelectionKind::Primary => self
                .primary_source
                .as_ref()
                .and(self.primary_content.as_ref()),
        }
    }

    /// Bind both devices to a seat that just reported a keyboard.
    ///
    /// Selections hang off the seat, so they cannot be created at startup with
    /// the managers — they wait for the capability.
    pub(super) fn attach_devices(
        &mut self,
        qh: &QueueHandle<WaylandState>,
        seat: &wl_seat::WlSeat,
    ) {
        if self.data_device.is_none()
            && let Some(ref manager) = self.data_device_manager
        {
            log::info!("Creating data device for clipboard");
            self.data_device = Some(manager.get_data_device(qh, seat));
        }

        if self.primary_selection_device.is_none()
            && let Some(ref manager) = self.primary_selection_manager
        {
            log::info!("Creating primary selection device");
            self.primary_selection_device = Some(manager.get_selection_device(qh, seat));
        }
    }
}

impl WaylandState {
    /// Set clipboard content (copy)
    pub fn set_clipboard(&mut self, text: String) {
        let qh = &self.qh;
        if let Some(ref manager) = self.selections.data_device_manager {
            // Create a data source for the clipboard
            let source = manager.create_copy_paste_source(qh, TEXT_MIMES);

            // Store the text to write when compositor requests it
            self.selections.clipboard_content = Some(text);

            // Set selection using the keyboard serial
            if let Some(ref device) = self.selections.data_device {
                source.set_selection(device, self.input.keyboard_serial);
                self.selections.clipboard_source = Some(source);
            }
        }
    }

    /// Set the primary selection content (select-to-copy).
    pub fn set_primary(&mut self, text: String) {
        let qh = &self.qh;
        let Some(ref manager) = self.selections.primary_selection_manager else {
            return;
        };
        let Some(ref device) = self.selections.primary_selection_device else {
            return;
        };

        let source = manager.create_selection_source(qh, TEXT_MIMES);
        self.selections.primary_content = Some(text);
        source.set_selection(device, self.input.latest_input_serial);
        self.selections.primary_source = Some(source);
    }

    /// Read `kind` for the pastes waiting on `token`.
    ///
    /// While guido owns the selection it answers from its own copy: going
    /// through the compositor would only hand the same text back through a
    /// pipe. Otherwise the current offer is read on a reader thread, and the
    /// result comes back through the calloop ingress channel — the message
    /// itself wakes the loop.
    pub(crate) fn read_selection(&mut self, kind: SelectionKind, token: u64) {
        if let Some(own) = self.selections.own_content(kind) {
            answer_paste(token, Some(own.clone()));
            return;
        }
        if self.start_read(kind, token).is_none() {
            answer_paste(token, None);
        }
    }

    /// Open the current offer of `kind` and read it on a reader thread.
    /// `None` when there is nothing to read or the read could not start.
    fn start_read(&self, kind: SelectionKind, token: u64) -> Option<()> {
        let pipe = match kind {
            SelectionKind::Clipboard => {
                let offer = self
                    .selections
                    .data_device
                    .as_ref()?
                    .data()
                    .selection_offer()?;
                receive_text(kind, offer.with_mime_types(text_mime), |mime| {
                    offer.receive(mime)
                })?
            }
            SelectionKind::Primary => {
                let device = self.selections.primary_selection_device.as_ref()?;
                let offer = device.data().selection_offer()?;
                receive_text(kind, offer.with_mime_types(text_mime), |mime| {
                    offer.receive(mime)
                })?
            }
        };
        // Bound to the loop that is running now: the read below has three
        // seconds to finish, and a result delivered into the next session's
        // loop could meet a token that means something else there.
        let sender = crate::ingress::sender_handle()?;
        std::thread::Builder::new()
            .name("guido-clipboard-read".into())
            .spawn(move || {
                let content = read_pipe_with_deadline(pipe, Duration::from_secs(3));
                sender.send(crate::ingress::IngressMessage::SelectionRead { token, content });
            })
            .map_err(|e| log::warn!("Failed to spawn clipboard reader thread: {e}"))
            .ok()?;
        Some(())
    }
}

/// Ask for an offer's content as `mime`, the text type chosen for it.
fn receive_text<E: std::fmt::Debug>(
    kind: SelectionKind,
    mime: Option<&'static str>,
    receive: impl FnOnce(String) -> Result<ReadPipe, E>,
) -> Option<ReadPipe> {
    let mime = mime?;
    receive(mime.to_string())
        .map_err(|e| log::debug!("Failed to receive {kind:?} as {mime}: {e:?}"))
        .ok()
}

/// Read a selection pipe to EOF with a total deadline. Runs on a reader
/// thread — never on the UI thread.
///
/// Every buffer the text passes through is wiped before it is freed: a paste
/// is often a password, copied from a manager.
fn read_pipe_with_deadline(pipe: ReadPipe, deadline: Duration) -> Option<String> {
    let mut file = File::from(OwnedFd::from(pipe));
    let mut buf = Zeroizing::new(Vec::new());
    let mut chunk = Zeroizing::new([0u8; 8192]);
    read_with_deadline(&mut file, &mut buf, &mut chunk[..], deadline)?;

    let text = match String::from_utf8(std::mem::take(&mut *buf)) {
        Ok(text) => text,
        Err(e) => {
            let mut bytes = e.into_bytes();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            bytes.zeroize();
            text
        }
    };
    (!text.is_empty()).then_some(text)
}

/// Read `file` into `buf` until EOF, or give up at the deadline.
fn read_with_deadline(
    file: &mut File,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    deadline: Duration,
) -> Option<()> {
    use std::os::unix::io::AsRawFd;

    let raw_fd = file.as_raw_fd();
    let end = Instant::now() + deadline;

    loop {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            log::warn!("Clipboard read timed out after {:?}", deadline);
            return None;
        }

        let mut poll_fd = libc::pollfd {
            fd: raw_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
        let ret = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            log::warn!("Clipboard read poll failed: {err}");
            return None;
        }
        if ret == 0 {
            log::warn!("Clipboard read timed out after {:?}", deadline);
            return None;
        }

        // POLLIN or POLLHUP: data available or writer closed — read either way
        match file.read(chunk) {
            Ok(0) => return Some(()), // EOF
            Ok(n) => {
                // Grown by hand, so the old buffer is wiped before it goes:
                // `extend_from_slice` would reallocate and free it as it was.
                if buf.capacity() - buf.len() < n {
                    let mut grown = Vec::with_capacity((buf.len() + n).max(2 * buf.capacity()));
                    grown.extend_from_slice(buf);
                    buf.zeroize();
                    *buf = grown;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("Clipboard read failed: {e}");
                return None;
            }
        }
    }
}

impl DataDeviceHandler for WaylandState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        _surface: &wl_surface::WlSurface,
    ) {
        // Drag and drop enter - not used for clipboard
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _data_device: &WlDataDevice) {
        // Drag and drop leave - not used for clipboard
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
        // Drag and drop motion - not used for clipboard
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        // Drag and drop performed - not used for clipboard
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        log::debug!("Clipboard selection changed");
        // Nothing is read until something pastes: only whether there is
        // anything to paste is recorded.
        let offered = self
            .selections
            .data_device
            .as_ref()
            .and_then(|device| device.data().selection_offer())
            .and_then(|offer| offer.with_mime_types(text_mime))
            .is_some();
        set_clipboard_offered(offered);
    }
}

impl PrimarySelectionDeviceHandler for WaylandState {
    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _device: &smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    ) {
        // Nothing is read until a middle click pastes.
        log::debug!("Primary selection changed");
    }
}

impl PrimarySelectionSourceHandler for WaylandState {
    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
        mime: String,
        write_pipe: smithay_client_toolkit::data_device_manager::WritePipe,
    ) {
        log::debug!("Primary selection send request for mime type: {mime}");
        if let Some(ref content) = self.selections.primary_content {
            let content = content.clone();
            let owned_fd = OwnedFd::from(write_pipe);
            if let Err(e) = std::thread::Builder::new()
                .name("guido-primary-send".into())
                .spawn(move || {
                    let mut file = File::from(owned_fd);
                    if let Err(e) = file.write_all(content.as_bytes()) {
                        log::warn!("Failed to write primary selection content: {e}");
                    }
                })
            {
                log::warn!("Failed to spawn primary selection writer thread: {e}");
            }
        }
    }

    fn cancelled(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
    ) {
        log::debug!("Primary selection source cancelled");
        self.selections.primary_source = None;
    }
}

impl DataOfferHandler for WaylandState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _actions: DndAction,
    ) {
        // Drag and drop actions - not used for clipboard
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _action: DndAction,
    ) {
        // Drag and drop selected action - not used for clipboard
    }
}

impl DataSourceHandler for WaylandState {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
        // Mime type accepted notification
    }

    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        mime: String,
        fd: smithay_client_toolkit::data_device_manager::WritePipe,
    ) {
        log::debug!("Clipboard send request for mime type: {}", mime);

        // Write clipboard content on a short-lived thread: a payload larger
        // than the pipe buffer with a slow reader would otherwise block the
        // UI thread indefinitely inside write_all.
        if let Some(ref content) = self.selections.clipboard_content {
            let content = content.clone();
            let owned_fd = OwnedFd::from(fd);
            if let Err(e) = std::thread::Builder::new()
                .name("guido-clipboard-send".into())
                .spawn(move || {
                    let mut file = File::from(owned_fd);
                    if let Err(e) = file.write_all(content.as_bytes()) {
                        log::warn!("Failed to write clipboard content: {}", e);
                    }
                })
            {
                log::warn!("Failed to spawn clipboard writer thread: {e}");
            }
        }
    }

    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        log::debug!("Clipboard source cancelled");
        self.selections.clipboard_source = None;
    }

    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        // Drag and drop completed - not used for clipboard
    }

    fn dnd_finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
        // Drag and drop finished - not used for clipboard
    }

    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _action: DndAction,
    ) {
        // Action notification - not used for clipboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Longer than a chunk and than the first allocation, so the buffer grows
    /// by hand more than once and each grow has to carry the text over.
    #[test]
    fn a_read_keeps_every_byte_across_its_grows() {
        let text: String = (0..20_000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let (reader, mut writer) = std::io::pipe().unwrap();
        let sent = text.clone();
        let writing = std::thread::spawn(move || writer.write_all(sent.as_bytes()));

        let mut file = File::from(OwnedFd::from(reader));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let read = read_with_deadline(&mut file, &mut buf, &mut chunk, Duration::from_secs(5));
        writing.join().unwrap().unwrap();

        assert_eq!(read, Some(()));
        assert_eq!(buf, text.as_bytes());
    }

    #[test]
    fn the_preferred_text_type_is_the_one_read() {
        let offered = ["STRING".to_owned(), "text/plain;charset=utf-8".to_owned()];
        assert_eq!(text_mime(&offered), Some("text/plain;charset=utf-8"));
        assert_eq!(text_mime(&["image/png".to_owned()]), None);
    }
}
