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
//! The box does not wait for the pixels: the intrinsic size comes from the
//! header, read synchronously when the entry is made, which costs a few bytes
//! rather than the whole image.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

use crate::app_state::with_app_state;
use crate::reactive::owner::with_root_owner;
use crate::reactive::{OwnerId, RwSignal, WriteSignal, create_signal, dispose_owner, with_owner};
use crate::widgets::image::ImageSource;

/// Decoded RGBA8 pixels, row-major, `width * height * 4` bytes.
///
/// The pixels are shared rather than copied: the cache entry, the draw command
/// that carries them to the renderer and the paint cache that keeps that
/// command all hold the same buffer.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, `width * height * 4` bytes.
    pub pixels: Arc<[u8]>,
}

/// Two decodes are the same when they are the same buffer. Comparing the bytes
/// would read megabytes to learn what the pointer already says.
impl PartialEq for DecodedImage {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && Arc::ptr_eq(&self.pixels, &other.pixels)
    }
}

/// Where the decode of one source has got to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DecodeState {
    /// Queued or running on the worker.
    Pending,
    /// Decoded; the renderer uploads it on the next frame that draws it.
    Ready(DecodedImage),
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
    fn of(source: &ImageSource) -> Option<Self> {
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
pub(crate) struct DecodeEntry {
    state: RwSignal<DecodeState>,
    /// The scope the signal lives in, under the root: disposed when the last
    /// image showing the source lets go of it.
    scope: OwnerId,
    /// Read from the header when the entry was made.
    size: Option<(u32, u32)>,
    /// How many [`DecodeHandle`]s hold the entry.
    users: u32,
}

/// The entry for `key`, made — and its decode started — if there is none,
/// with `users` more holders than it had.
fn entry(
    key: DecodeKey,
    source: &ImageSource,
    users: u32,
) -> (RwSignal<DecodeState>, Option<(u32, u32)>) {
    let found = with_app_state(|app| {
        app.decoded_images.borrow_mut().get_mut(&key).map(|entry| {
            entry.users += users;
            (entry.state, entry.size)
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
    // widget happened to ask first, and goes when the last one lets go.
    let (state, scope) = with_root_owner(|| with_owner(|| create_signal(initial)));
    let job = size.is_some().then(|| key.clone());
    with_app_state(|app| {
        app.decoded_images.borrow_mut().insert(
            key,
            DecodeEntry {
                state,
                scope,
                size,
                users,
            },
        );
    });
    if let Some(key) = job {
        decode_later(key, state.writer());
    }
    (state, size)
}

fn describe(key: &DecodeKey) -> String {
    match key {
        DecodeKey::Path(path) => path.display().to_string(),
        DecodeKey::Bytes(bytes) => format!("an in-memory image of {} bytes", bytes.len()),
    }
}

/// Where the decode of `source` has got to, read so that the reader is told
/// when it moves — or `None` for a source that needs no decode: its pixels
/// are already there (`Rgba`) or are rasterised when drawn (SVG).
///
/// Makes the entry and starts its decode if nobody has asked before. An entry
/// made here rather than by an [`acquire`] is held by nobody, and lives as
/// long as the application.
pub(crate) fn state(source: &ImageSource) -> Option<DecodeState> {
    let key = DecodeKey::of(source)?;
    Some(entry(key, source, 0).0.get())
}

/// Whether `source` can be drawn now: decoded, or needing no decode.
pub(crate) fn is_ready(source: &ImageSource) -> bool {
    !matches!(
        state(source),
        Some(DecodeState::Pending | DecodeState::Failed)
    )
}

/// Hold the entry for `source`, starting its decode if it has none — or `None`
/// for a source that needs no decode.
///
/// The entry lives while somebody holds it: the decoded pixels are as large
/// as the image, and an application that showed a wallpaper once should not
/// keep it in memory for good.
pub(crate) fn acquire(source: &ImageSource) -> Option<DecodeHandle> {
    let key = DecodeKey::of(source)?;
    let (state, size) = entry(key.clone(), source, 1);
    Some(DecodeHandle { key, state, size })
}

/// A claim on one source's entry, given back when dropped.
pub(crate) struct DecodeHandle {
    key: DecodeKey,
    state: RwSignal<DecodeState>,
    size: Option<(u32, u32)>,
}

impl DecodeHandle {
    /// Where the decode has got to, read so that the reader is told when it
    /// moves. The holder's own signal: no lookup, no hashing.
    pub(crate) fn state(&self) -> DecodeState {
        self.state.get()
    }

    /// The size the header gave, or `None` where it could not be read.
    pub(crate) fn size(&self) -> Option<(u32, u32)> {
        self.size
    }
}

impl Drop for DecodeHandle {
    fn drop(&mut self) {
        let released = with_app_state(|app| {
            let mut map = app.decoded_images.borrow_mut();
            let entry = map.get_mut(&self.key)?;
            entry.users = entry.users.saturating_sub(1);
            if entry.users > 0 {
                return None;
            }
            map.remove(&self.key).map(|entry| entry.scope)
        });
        // A decode still running for it writes into a disposed signal, and
        // that write is dropped where the queue is flushed.
        if let Some(scope) = released {
            dispose_owner(scope);
        }
    }
}

// ---------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------

/// A source to decode, and where to write what it decoded to.
struct DecodeJob {
    key: DecodeKey,
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
                    job.write.set(decode(&job.key));
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

/// Hand `key` to the worker, which writes the outcome through `write`.
fn decode_later(key: DecodeKey, write: WriteSignal<DecodeState>) {
    let sent = with_decoder(|decoder| {
        {
            let mut progress = decoder.progress.lock();
            progress.in_flight += 1;
            progress.started += 1;
        }
        decoder.jobs.send(DecodeJob { key, write }).is_ok()
    });
    // No worker to decode it: the entry is failed rather than pending for ever.
    if sent != Some(true) {
        write.set(DecodeState::Failed);
    }
}

/// Decode on the worker. A failure is loud, once: a missing decoder feature
/// (`webp` disabled) or a bad file would otherwise be a silently empty box.
fn decode(key: &DecodeKey) -> DecodeState {
    let decoded = match key {
        DecodeKey::Path(path) => image::open(path),
        DecodeKey::Bytes(bytes) => image::load_from_memory(bytes),
    };
    match decoded {
        Ok(image) => {
            let rgba = image.into_rgba8();
            let (width, height) = rgba.dimensions();
            DecodeState::Ready(DecodedImage {
                width,
                height,
                pixels: rgba.into_raw().into(),
            })
        }
        Err(e) => {
            log::warn!("Failed to decode {}: {e}", describe(key));
            DecodeState::Failed
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

    /// An entry lives while an image holds it and goes with the last one, so a
    /// wallpaper shown once is not kept decoded for good.
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
        assert_eq!(state(&source), Some(DecodeState::Failed));
        assert_eq!(first.size(), None, "the header could not be read");

        drop(first);
        assert_eq!(entries(), before + 1, "still held by the second");
        drop(second);
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
