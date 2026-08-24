//! The two system selections: the clipboard and the primary selection.
//!
//! Both work the same way — an offer arrives, a reader thread pulls the
//! content through a pipe, and the result comes back on the calloop ingress
//! channel — so they are kept together and share one prefetch path.
//!
//! Reading a selection means blocking on a pipe until the owning application
//! answers, which is why it never happens on the UI thread. Generation
//! counters drop any result that a newer offer has already made stale.

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
    delegate_data_device, delegate_primary_selection,
    primary_selection::{
        PrimarySelectionManagerState,
        device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler},
        selection::{PrimarySelectionSource, PrimarySelectionSourceHandler},
    },
};

use smithay_client_toolkit::reexports::client::{
    Connection, Proxy, QueueHandle,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_seat, wl_surface,
    },
};

use super::wayland::WaylandState;

/// Which system selection a prefetched content update belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// The regular clipboard (Ctrl+C / Ctrl+V).
    Clipboard,
    /// The primary selection (select-to-copy / middle-click paste).
    Primary,
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

    /// Bumped on every new offer. A reader thread stamps its result with the
    /// generation it started from, and a result that no longer matches is
    /// dropped: the selection moved on while the pipe was still open.
    pub(super) selection_generation: u64,
    pub(super) primary_generation: u64,
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
            selection_generation: 0,
            primary_generation: 0,
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
    pub fn set_clipboard(&mut self, text: String, qh: &QueueHandle<Self>) {
        if let Some(ref manager) = self.selections.data_device_manager {
            // Create a data source for the clipboard
            let source = manager.create_copy_paste_source(
                qh,
                vec!["text/plain;charset=utf-8", "UTF8_STRING", "TEXT", "STRING"],
            );

            // Store the text to write when compositor requests it
            self.selections.clipboard_content = Some(text);

            // Set selection using the keyboard serial
            if let Some(ref device) = self.selections.data_device {
                source.set_selection(device, self.input.keyboard_serial);
                self.selections.clipboard_source = Some(source);
            }
        }
    }

    /// Get clipboard content (paste)
    /// Returns the content if available, or None if clipboard is empty
    pub fn get_clipboard(&self) -> Option<String> {
        self.selections.clipboard_content.clone()
    }

    /// Set the primary selection content (select-to-copy).
    pub fn set_primary(&mut self, text: String, qh: &QueueHandle<Self>) {
        let Some(ref manager) = self.selections.primary_selection_manager else {
            return;
        };
        let Some(ref device) = self.selections.primary_selection_device else {
            return;
        };

        let source = manager.create_selection_source(
            qh,
            vec!["text/plain;charset=utf-8", "UTF8_STRING", "TEXT", "STRING"],
        );
        self.selections.primary_content = Some(text);
        source.set_selection(device, self.input.latest_input_serial);
        self.selections.primary_source = Some(source);
    }

    /// Apply a prefetched clipboard/primary content update (from the loop's
    /// ingress channel callback). Reads made stale by a newer offer are
    /// dropped via the generation check.
    pub(crate) fn apply_clipboard_update(
        &mut self,
        kind: SelectionKind,
        generation: u64,
        content: Option<String>,
    ) {
        let current = match kind {
            SelectionKind::Clipboard => self.selections.selection_generation,
            SelectionKind::Primary => self.selections.primary_generation,
        };
        if generation != current {
            log::debug!("Dropping stale {kind:?} content (gen {generation} != {current})");
            return;
        }
        match kind {
            SelectionKind::Clipboard => match content {
                Some(text) => crate::reactive::set_system_clipboard(text),
                None => crate::reactive::clear_system_clipboard(),
            },
            SelectionKind::Primary => crate::reactive::set_system_primary(content),
        }
    }

    /// Start an async prefetch of an offer's content on a reader thread.
    /// `receive` turns a chosen mime type into a read pipe. The result comes
    /// back through the calloop ingress channel — the message itself wakes
    /// the loop, no hand-rolled wakeup involved.
    fn prefetch_selection<R>(&mut self, kind: SelectionKind, mimes: Vec<String>, receive: R)
    where
        R: FnOnce(&str) -> Option<ReadPipe>,
    {
        let generation = match kind {
            SelectionKind::Clipboard => {
                self.selections.selection_generation += 1;
                self.selections.selection_generation
            }
            SelectionKind::Primary => {
                self.selections.primary_generation += 1;
                self.selections.primary_generation
            }
        };

        // Preferred mime order; take the first one offered.
        const PREFERRED: [&str; 5] = [
            "text/plain;charset=utf-8",
            "UTF8_STRING",
            "text/plain",
            "TEXT",
            "STRING",
        ];
        let mime = PREFERRED
            .iter()
            .find(|m| mimes.iter().any(|t| t == *m))
            .copied();

        let Some(pipe) = mime.and_then(receive) else {
            // Nothing readable as text — treat as cleared. We're on the main
            // thread (called from a selection handler), so apply directly.
            self.apply_clipboard_update(kind, generation, None);
            return;
        };

        if !crate::ingress::loop_running() {
            // No running event loop to deliver the result to.
            log::warn!("Selection prefetch skipped: no event loop running");
            return;
        }

        if let Err(e) = std::thread::Builder::new()
            .name("guido-clipboard-read".into())
            .spawn(move || {
                let content = read_pipe_with_deadline(pipe, Duration::from_secs(3));
                crate::ingress::notify(crate::ingress::IngressMessage::ClipboardUpdate {
                    kind,
                    generation,
                    content,
                });
            })
        {
            log::warn!("Failed to spawn clipboard reader thread: {e}");
        }
    }
}

/// Read a selection pipe to EOF with a total deadline. Runs on a reader
/// thread — never on the UI thread.
fn read_pipe_with_deadline(pipe: ReadPipe, deadline: Duration) -> Option<String> {
    use std::os::unix::io::AsRawFd;

    let fd = OwnedFd::from(pipe);
    let mut file = File::from(fd);
    let raw_fd = file.as_raw_fd();
    let mut buf = Vec::new();
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
        let mut chunk = [0u8; 8192];
        match file.read(&mut chunk) {
            Ok(0) => break, // EOF
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("Clipboard read failed: {e}");
                return None;
            }
        }
    }

    let text = String::from_utf8_lossy(&buf).into_owned();
    (!text.is_empty()).then_some(text)
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
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        log::debug!("Clipboard selection changed");
        // Prefetch the new selection's content on a reader thread so paste
        // is instant and never blocks the UI thread.
        let offer = self
            .selections
            .data_device
            .as_ref()
            .and_then(|device| device.data().selection_offer());
        match offer {
            None => {
                // Selection cleared — main thread, apply directly.
                self.selections.selection_generation += 1;
                let generation = self.selections.selection_generation;
                self.apply_clipboard_update(SelectionKind::Clipboard, generation, None);
            }
            Some(offer) => {
                let mimes = offer.with_mime_types(|t| t.to_vec());
                self.prefetch_selection(SelectionKind::Clipboard, mimes, |mime| {
                    offer
                        .receive(mime.to_string())
                        .map_err(|e| log::debug!("Failed to receive clipboard as {mime}: {e:?}"))
                        .ok()
                });
                // Send the receive request out now so the source app starts
                // writing before the next loop-iteration flush.
                let _ = conn.flush();
            }
        }
    }
}

impl PrimarySelectionDeviceHandler for WaylandState {
    fn selection(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        device: &smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    ) {
        log::debug!("Primary selection changed");
        let offer = device
            .data::<smithay_client_toolkit::primary_selection::device::PrimarySelectionDeviceData>()
            .and_then(|data| data.selection_offer());
        match offer {
            None => {
                // Primary selection cleared — main thread, apply directly.
                self.selections.primary_generation += 1;
                let generation = self.selections.primary_generation;
                self.apply_clipboard_update(SelectionKind::Primary, generation, None);
            }
            Some(offer) => {
                let mimes = offer.with_mime_types(|t| t.to_vec());
                self.prefetch_selection(SelectionKind::Primary, mimes, |mime| {
                    offer
                        .receive(mime.to_string())
                        .map_err(|e| log::debug!("Failed to receive primary as {mime}: {e:?}"))
                        .ok()
                });
                let _ = conn.flush();
            }
        }
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

delegate_data_device!(WaylandState);
delegate_primary_selection!(WaylandState);
