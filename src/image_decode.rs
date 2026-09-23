//! Raster images decoded off the frame.
//!
//! An `ImageSource::Path` or `ImageSource::Bytes` is encoded, and turning it
//! into pixels takes as long as the image is large — 140 ms for a 2912×1632
//! wallpaper. Done inside the frame that first draws it, nothing of that frame
//! reaches the screen until it is finished, which a lock screen shows as the
//! compositor's "locker has not drawn" colour.
//!
//! So the decode is a signal. Each source has one entry in a cache the whole
//! application shares, and the entry holds a signal that is
//! [`Pending`](DecodeState::Pending), [`Ready`](DecodeState::Ready) or
//! [`Failed`](DecodeState::Failed). A widget reads it while painting, which
//! subscribes it; a worker thread decodes and writes the result through a
//! `WriteSignal`, whose write is queued, wakes the loop, and repaints exactly
//! the widgets that read the entry. Two widgets on one source read one entry,
//! so they share one decode.
//!
//! The pixels are not in the signal. They wait in the entry's
//! [`DecodedImage`] until the renderer takes them to upload, and taking them
//! is what drops them: from then on the texture is the image, as iced's cache
//! turns `Memory::Host` into `Memory::Device`. A texture that is gone when it
//! is needed again — evicted, or its renderer dropped — has nothing left to be
//! drawn from, so the renderer reports it and the source goes back to pending
//! and to the worker. No surface can draw from pixels that were dropped,
//! because the only way to the pixels is to take them.
//!
//! The box does not wait for the pixels: the intrinsic size comes from the
//! header, read synchronously when the entry is made, which costs a few bytes
//! rather than the whole image.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::app_state::with_app_state;
use crate::reactive::owner::with_root_owner;
use crate::reactive::{OwnerId, RwSignal, WriteSignal, create_signal, dispose_owner, with_owner};
use crate::widgets::image::ImageSource;

/// Where one source's decoded pixels wait for the renderer.
///
/// A handle: the entry, the job decoding into it and every draw command
/// painted from it hold the same one, and the pixels in it are there only
/// until the renderer takes them to upload. A paint cache
/// that keeps the command keeps an empty handle, not a copy of the image.
#[derive(Clone, Debug, Default)]
pub struct DecodedImage(Arc<Mutex<Slot>>);

#[derive(Debug, Default)]
enum Slot {
    /// No pixels: the decode has not landed, or its texture was evicted.
    #[default]
    Waiting,
    /// Decoded, waiting for the renderer.
    Filled(Pixels),
    /// The renderer took the pixels, so a texture of them exists — until it
    /// says it evicted it.
    Uploaded,
    /// Nobody will draw them: pixels that arrive now are dropped on arrival.
    Abandoned,
}

/// Decoded RGBA8 pixels, row-major, `width * height * 4` bytes.
#[derive(Debug)]
pub(crate) struct Pixels {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

impl DecodedImage {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The pixels, for the renderer to upload — once. What was taken is gone
    /// from here, which is the drop the upload is.
    pub(crate) fn take(&self) -> Option<Pixels> {
        let mut slot = self.slot();
        match std::mem::take(&mut *slot) {
            Slot::Filled(pixels) => {
                *slot = Slot::Uploaded;
                Some(pixels)
            }
            other => {
                *slot = other;
                None
            }
        }
    }

    /// How many bytes of pixels are waiting here: zero once uploaded.
    #[cfg(feature = "testing")]
    pub(crate) fn byte_size(&self) -> usize {
        match &*self.slot() {
            Slot::Filled(pixels) => pixels.rgba.len(),
            _ => 0,
        }
    }

    fn fill(&self, pixels: Pixels) {
        let mut slot = self.slot();
        if !matches!(*slot, Slot::Abandoned) {
            *slot = Slot::Filled(pixels);
        }
    }
}

/// Two handles are the same when they are the same slot.
impl PartialEq for DecodedImage {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Where the decode of one source has got to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DecodeState {
    /// Queued or running on the worker.
    Pending,
    /// Decoded: the pixels wait for the renderer, or are already a texture.
    Ready,
    /// The header or the decode failed, and said why once, in the log.
    Failed,
}

/// What an entry is found by: the source, for the two kinds that decode.
///
/// Bytes hash a sample rather than every byte, because a paint looks its entry
/// up and a paint must not read a 9 MB buffer end to end. Equality then
/// settles what a sample cannot, and the same `Arc` settles it at once.
#[derive(Clone, Debug)]
pub(crate) enum DecodeKey {
    Path(PathBuf),
    Bytes(Arc<[u8]>),
}

impl DecodeKey {
    pub(crate) fn of(source: &ImageSource) -> Option<Self> {
        match source {
            ImageSource::Path(path) => Some(Self::Path(path.clone())),
            ImageSource::Bytes(bytes) => Some(Self::Bytes(bytes.clone())),
            ImageSource::Rgba { .. } | ImageSource::SvgPath(_) | ImageSource::SvgBytes(_) => None,
        }
    }
}

impl PartialEq for DecodeKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Path(a), Self::Path(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => Arc::ptr_eq(a, b) || a[..] == b[..],
            _ => false,
        }
    }
}

impl Eq for DecodeKey {}

impl Hash for DecodeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Path(path) => {
                0u8.hash(state);
                path.hash(state);
            }
            Self::Bytes(bytes) => {
                1u8.hash(state);
                hash_sampled(bytes, state);
            }
        }
    }
}

/// Bytes sampled from each of the three places [`hash_sampled`] reads.
const HASH_SAMPLE_SIZE: usize = 256;

/// Hash a buffer by its length and three samples of it — the start, the
/// middle and the end — or whole when it is small.
///
/// A lookup's cost, not an identity: whoever keys on this settles collisions
/// with equality or accepts them, and the renderer's texture cache has
/// accepted them since it was written.
pub(crate) fn hash_sampled(bytes: &[u8], hasher: &mut impl Hasher) {
    bytes.len().hash(hasher);
    if bytes.len() < 1024 {
        bytes.hash(hasher);
        return;
    }
    let sample = HASH_SAMPLE_SIZE;
    bytes[..sample].hash(hasher);
    let mid = bytes.len() / 2 - sample / 2;
    bytes[mid..mid + sample].hash(hasher);
    bytes[bytes.len() - sample..].hash(hasher);
}

/// One source's entry in [`AppState::decoded_images`](crate::app_state::AppState).
#[derive(Clone)]
pub(crate) struct DecodeEntry {
    state: RwSignal<DecodeState>,
    /// Where the pixels wait for the renderer.
    pixels: DecodedImage,
    /// The scope the signal lives in, under the root, disposed with the entry.
    scope: OwnerId,
    /// Read from the header when the entry was made.
    size: Option<(u32, u32)>,
    /// How many [`DecodeHandle`]s hold the entry.
    users: u32,
}

/// The entry for `key`, made — and its decode started — if there is none,
/// with `users` more holders than it had.
fn entry(key: DecodeKey, source: &ImageSource, users: u32) -> DecodeEntry {
    let found = with_app_state(|app| {
        app.decoded_images.borrow_mut().get_mut(&key).map(|entry| {
            entry.users += users;
            entry.clone()
        })
    });
    if let Some(found) = found {
        return found;
    }

    // Outside the borrow: creating a signal reaches the runtime and the owner
    // arena, and neither may find the map borrowed.
    let size = crate::image_metadata::get_intrinsic_size(source);
    let initial = if size.is_some() {
        DecodeState::Pending
    } else {
        log::warn!("Failed to read the image header of {}", describe(&key));
        DecodeState::Failed
    };
    // Under the root, in a scope of its own: the entry outlives whichever
    // widget happened to ask first.
    let (state, scope) = with_root_owner(|| with_owner(|| create_signal(initial)));
    let entry = DecodeEntry {
        state,
        pixels: DecodedImage::default(),
        scope,
        size,
        users,
    };
    if size.is_some() {
        decode_later(key.clone(), &entry);
    }
    with_app_state(|app| {
        app.decoded_images.borrow_mut().insert(key, entry.clone());
    });
    entry
}

fn describe(key: &DecodeKey) -> String {
    match key {
        DecodeKey::Path(path) => path.display().to_string(),
        DecodeKey::Bytes(bytes) => format!("an in-memory image of {} bytes", bytes.len()),
    }
}

/// Where the decode of `source` has got to, read so that the reader is told
/// when it moves, and where its pixels wait — or `None` for a source that
/// needs no decode: its pixels are already there (`Rgba`) or are rasterised
/// when drawn (SVG).
///
/// Makes the entry and starts its decode if nobody has asked before. An entry
/// made here rather than by an [`acquire`] is held by nobody: it lives while
/// its texture does.
pub(crate) fn state(source: &ImageSource) -> Option<(DecodeState, DecodedImage)> {
    let key = DecodeKey::of(source)?;
    let entry = entry(key, source, 0);
    Some((entry.state.get(), entry.pixels))
}

/// Whether `source` can be drawn now: decoded, or needing no decode.
pub(crate) fn is_ready(source: &ImageSource) -> bool {
    !matches!(
        state(source),
        Some((DecodeState::Pending | DecodeState::Failed, _))
    )
}

/// Hold the entry for `source`, starting its decode if it has none — or `None`
/// for a source that needs no decode.
pub(crate) fn acquire(source: &ImageSource) -> Option<DecodeHandle> {
    let key = DecodeKey::of(source)?;
    let entry = entry(key.clone(), source, 1);
    Some(DecodeHandle {
        key,
        state: entry.state,
        pixels: entry.pixels,
        size: entry.size,
    })
}

/// A claim on one source's entry, given back when dropped.
pub(crate) struct DecodeHandle {
    key: DecodeKey,
    state: RwSignal<DecodeState>,
    pixels: DecodedImage,
    size: Option<(u32, u32)>,
}

impl DecodeHandle {
    /// Where the decode has got to, read so that the reader is told when it
    /// moves, and where the pixels wait. The holder's own: no lookup, no
    /// hashing.
    pub(crate) fn state(&self) -> (DecodeState, DecodedImage) {
        (self.state.get(), self.pixels.clone())
    }

    /// The size the header gave, or `None` where it could not be read.
    pub(crate) fn size(&self) -> Option<(u32, u32)> {
        self.size
    }
}

/// Letting go is settled by the loop, like what the renderer reports: a drop
/// happens wherever a widget is torn down, and what the last one may lead to —
/// an entry removed, its readers told — is a signal write.
impl Drop for DecodeHandle {
    fn drop(&mut self) {
        with_app_state(|app| {
            let last = app
                .decoded_images
                .borrow_mut()
                .get_mut(&self.key)
                .is_some_and(|entry| {
                    entry.users = entry.users.saturating_sub(1);
                    entry.users == 0
                });
            if last {
                app.image_events
                    .push(ImageEvent::Released(self.key.clone()));
            }
        });
    }
}

// ---------------------------------------------------------------------------
// What the renderer and the widgets report, settled once per pass
// ---------------------------------------------------------------------------

/// Something that happened to an entry where no signal may be written: in the
/// renderer, or in a widget's drop.
pub(crate) enum ImageEvent {
    /// A frame drew the source, its texture was not there, and its pixels had
    /// already gone to an upload.
    Missing(DecodeKey),
    /// The renderer evicted the source's texture.
    Evicted(DecodeKey),
    /// The last image holding the entry let go of it.
    Released(DecodeKey),
}

/// The renderer drew `source`, found no texture and no pixels.
pub(crate) fn texture_missing(source: &ImageSource) {
    if let Some(key) = DecodeKey::of(source) {
        with_app_state(|app| app.image_events.push(ImageEvent::Missing(key)));
    }
}

/// The renderer evicted the texture of the source `key` names.
pub(crate) fn texture_evicted(key: DecodeKey) {
    with_app_state(|app| app.image_events.push(ImageEvent::Evicted(key)));
}

/// Settle what the renderer and the widgets reported since the last pass.
///
/// - A **missing** texture of a ready source sends it back to pending and to
///   the worker: its readers repaint to nothing, and again when it lands.
/// - An entry nobody holds goes when nothing is left to be drawn from — its
///   texture **evicted**, or never made when the last holder was
///   **released**. An uploaded one stays while its texture does, so an image
///   mounted again is drawn from it without a decode.
pub(crate) fn settle_image_events() {
    for event in with_app_state(|app| app.image_events.drain()) {
        let key = match &event {
            ImageEvent::Missing(key) | ImageEvent::Evicted(key) | ImageEvent::Released(key) => key,
        };
        let Some(entry) = with_app_state(|app| app.decoded_images.borrow().get(key).cloned())
        else {
            continue;
        };
        match event {
            ImageEvent::Missing(key) => {
                let gone = !matches!(*entry.pixels.slot(), Slot::Filled(_));
                if gone && entry.state.get_untracked() == DecodeState::Ready {
                    entry.state.set(DecodeState::Pending);
                    decode_later(key, &entry);
                }
            }
            ImageEvent::Evicted(key) => {
                {
                    let mut slot = entry.pixels.slot();
                    if matches!(*slot, Slot::Uploaded) {
                        *slot = Slot::Waiting;
                    }
                }
                remove_if_unheld(&key, entry);
            }
            ImageEvent::Released(key) => remove_if_unheld(&key, entry),
        }
    }
}

/// Remove an entry nobody holds and that has no texture to be drawn from.
///
/// Its readers are told, so a paint that looked it up by source looks again
/// and finds a fresh one; pixels still on their way are dropped on arrival.
fn remove_if_unheld(key: &DecodeKey, entry: DecodeEntry) {
    if entry.users > 0 || matches!(*entry.pixels.slot(), Slot::Uploaded) {
        return;
    }
    with_app_state(|app| app.decoded_images.borrow_mut().remove(key));
    *entry.pixels.slot() = Slot::Abandoned;
    entry.state.set_always(DecodeState::Pending);
    dispose_owner(entry.scope);
}

// ---------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------

/// A source to decode, where to put the pixels, and the signal that says so.
struct DecodeJob {
    key: DecodeKey,
    pixels: DecodedImage,
    write: WriteSignal<DecodeState>,
}

/// The application's decode worker: one thread, fed through a channel, which
/// ends when the channel does — when the `App` that spawned it is forgotten.
pub(crate) struct Decoder {
    jobs: mpsc::Sender<DecodeJob>,
    progress: Arc<Progress>,
}

/// What the worker and the loop both see: how many decodes are queued or
/// running, and whether a test has held them.
#[derive(Default)]
struct Progress {
    state: Mutex<ProgressState>,
    changed: Condvar,
}

#[derive(Default)]
struct ProgressState {
    in_flight: usize,
    started: u64,
    held: bool,
}

impl Progress {
    fn lock(&self) -> std::sync::MutexGuard<'_, ProgressState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Decoder {
    fn spawn() -> Option<Self> {
        let (jobs, received) = mpsc::channel::<DecodeJob>();
        let progress = Arc::new(Progress::default());
        let worker = progress.clone();
        std::thread::Builder::new()
            .name("guido-image-decode".into())
            .spawn(move || {
                for job in received {
                    drop(worker.changed.wait_while(worker.lock(), |state| state.held));
                    // Queued before the count goes down, so whoever waits for
                    // the count finds the write already in the queue.
                    let state = match decode(&job.key) {
                        Some(pixels) => {
                            job.pixels.fill(pixels);
                            DecodeState::Ready
                        }
                        None => DecodeState::Failed,
                    };
                    job.write.set(state);
                    worker.lock().in_flight -= 1;
                    worker.changed.notify_all();
                }
            })
            .map_err(|e| log::warn!("failed to spawn the image decode thread: {e}"))
            .ok()?;
        Some(Self { jobs, progress })
    }
}

/// The application's decoder, spawned the first time it is needed.
fn with_decoder<R>(f: impl FnOnce(&Decoder) -> R) -> Option<R> {
    with_app_state(|app| {
        let mut decoder = app.image_decoder.borrow_mut();
        if decoder.is_none() {
            *decoder = Decoder::spawn();
        }
        decoder.as_ref().map(f)
    })
}

/// Hand `key` to the worker, which fills the entry's pixels and writes its
/// state.
fn decode_later(key: DecodeKey, entry: &DecodeEntry) {
    let write = entry.state.writer();
    let pixels = entry.pixels.clone();
    let sent = with_decoder(|decoder| {
        {
            let mut progress = decoder.progress.lock();
            progress.in_flight += 1;
            progress.started += 1;
        }
        decoder.jobs.send(DecodeJob { key, pixels, write }).is_ok()
    });
    // No worker to decode it: the entry is failed rather than pending for ever.
    if sent != Some(true) {
        write.set(DecodeState::Failed);
    }
}

/// Decode on the worker. A failure is loud, once: a missing decoder feature
/// (`webp` disabled) or a bad file would otherwise be a silently empty box.
/// An empty image is a failure too — there is nothing to upload, and a ready
/// entry the renderer cannot upload would be sent back to the worker for ever.
fn decode(key: &DecodeKey) -> Option<Pixels> {
    let decoded = match key {
        DecodeKey::Path(path) => image::open(path),
        DecodeKey::Bytes(bytes) => image::load_from_memory(bytes),
    };
    match decoded {
        Ok(image) => {
            let rgba = image.into_rgba8();
            let (width, height) = rgba.dimensions();
            if width == 0 || height == 0 {
                log::warn!("{} decodes to an empty image", describe(key));
                return None;
            }
            Some(Pixels {
                width,
                height,
                rgba: rgba.into_raw(),
            })
        }
        Err(e) => {
            log::warn!("Failed to decode {}: {e}", describe(key));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// What a test holds the worker with
// ---------------------------------------------------------------------------

/// Decodes held back until this is released or dropped — what makes "not yet
/// decoded" something a test can arrange rather than race.
#[cfg(feature = "testing")]
pub struct DecodeHold(Arc<Progress>);

#[cfg(feature = "testing")]
impl DecodeHold {
    /// Let the held decodes run.
    pub fn release(self) {}
}

#[cfg(feature = "testing")]
impl Drop for DecodeHold {
    fn drop(&mut self) {
        self.0.lock().held = false;
        self.0.changed.notify_all();
    }
}

/// Hold this application's decodes until the returned hold is released.
#[cfg(feature = "testing")]
pub(crate) fn hold() -> DecodeHold {
    let progress = with_decoder(|decoder| decoder.progress.clone())
        .expect("the image decode thread could not be spawned");
    progress.lock().held = true;
    DecodeHold(progress)
}

/// Block until every decode this application started has queued its write.
///
/// Panics if decodes are held, or if they take longer than a test should.
#[cfg(feature = "testing")]
pub(crate) fn wait() {
    let Some(progress) = with_app_state(|app| {
        app.image_decoder
            .borrow()
            .as_ref()
            .map(|d| d.progress.clone())
    }) else {
        return;
    };
    let state = progress.lock();
    assert!(
        state.in_flight == 0 || !state.held,
        "waiting for image decodes that are held: release the hold first"
    );
    let (state, timeout) = progress
        .changed
        .wait_timeout_while(state, std::time::Duration::from_secs(30), |state| {
            state.in_flight > 0
        })
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        !timeout.timed_out(),
        "{} image decodes still running after 30 s",
        state.in_flight
    );
}

/// How many bytes of decoded pixels this application's cache is holding.
#[cfg(feature = "testing")]
pub(crate) fn held_bytes() -> usize {
    with_app_state(|app| {
        app.decoded_images
            .borrow()
            .values()
            .map(|entry| entry.pixels.byte_size())
            .sum()
    })
}

/// How many decodes this application has started.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn started() -> u64 {
    with_app_state(|app| {
        app.image_decoder
            .borrow()
            .as_ref()
            .map_or(0, |d| d.progress.lock().started)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> usize {
        with_app_state(|app| app.decoded_images.borrow().len())
    }

    /// An entry that never became a texture lives while an image holds it and
    /// goes with the last one.
    ///
    /// A file that is not there, so the header fails where the entry is made
    /// and no worker is started: what a worker writes lands in the
    /// process-wide queue, which every other test in this binary drains.
    #[test]
    fn an_entry_goes_with_the_last_image_that_holds_it() {
        let source = ImageSource::Path("not-there/decode-entry.png".into());
        let before = entries();

        let first = acquire(&source).expect("a raster source has an entry");
        let second = acquire(&source).expect("and the same one again");
        assert_eq!(entries(), before + 1, "one entry for one source");
        assert_eq!(
            state(&source).map(|(state, _)| state),
            Some(DecodeState::Failed)
        );
        assert_eq!(first.size(), None, "the header could not be read");

        drop(first);
        settle_image_events();
        assert_eq!(entries(), before + 1, "still held by the second");
        drop(second);
        settle_image_events();
        assert_eq!(entries(), before, "gone with the last");
        assert_eq!(started(), 0, "and nothing was handed to a worker");
    }

    /// A source that needs no decode has no entry and is ready at once.
    #[test]
    fn raw_pixels_and_svg_need_no_decode() {
        let rgba = ImageSource::Rgba {
            width: 1,
            height: 1,
            pixels: vec![0; 4].into(),
        };
        let svg = ImageSource::SvgBytes(b"<svg/>".to_vec().into());
        for source in [rgba, svg] {
            assert!(acquire(&source).is_none());
            assert!(is_ready(&source));
        }
    }
}
