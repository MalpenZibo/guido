//! Text that must not outlive its use: a password, in memory the rest of the
//! program never shares.
//!
//! An ordinary `String` is the wrong home for a password three times over. A
//! `String` that grows moves, and the old buffer goes back to the allocator
//! still holding its bytes. Its pages can be swapped to disk. And a crash under
//! `systemd-coredump` writes the whole heap to a file the user can read back
//! with `coredumpctl`.
//!
//! A [`Secret`] keeps its text in pages mapped for it alone:
//!
//! - **`mlock`ed**, so they are never swapped out. If the kernel refuses (a
//!   `RLIMIT_MEMLOCK` of nothing, say), the secret works unlocked, as
//!   swaylock's buffer does.
//! - **`MADV_DONTDUMP`**, so a core dump leaves them out. This is the flag
//!   GTK 4's secure memory sets, and the one that answers `coredumpctl`.
//! - **Zeroed before they are given back**, whether the secret grew out of
//!   them, was cleared, or was dropped.
//!
//! What it deliberately lacks is as much a part of it: no `Clone`, no
//! `Display`, no `Deref`, and a `Debug` that prints nothing. The one way to the
//! text is [`expose`](Secret::expose), which lends it.
//!
//! It does not encrypt. .NET's `SecureString` did, and Microsoft now advises
//! against it: the text has to be plain to be used, so encryption moves the
//! exposure rather than removing it.

use std::fmt;
use std::ops::Range;
use std::ptr::NonNull;

use zeroize::Zeroize;

/// A password, held where it cannot be swapped, dumped, or left behind.
///
/// Build one from a `String` with [`From`] — the `String` is zeroed on the
/// way in — or receive one from a
/// [`password_input`](crate::widgets::password_input). Read it with
/// [`expose`](Self::expose), for as long as the borrow lasts.
///
/// ```
/// use guido::Secret;
///
/// let secret = Secret::from(String::from("hunter2"));
/// assert_eq!(secret.expose(), "hunter2");
/// assert_eq!(format!("{secret:?}"), "Secret(..)");
/// ```
pub struct Secret {
    /// The mapping, or dangling while nothing has been written.
    ptr: NonNull<u8>,
    /// Bytes mapped: a whole number of pages, or zero.
    capacity: usize,
    /// Bytes of text, always valid UTF-8 and always the start of the mapping.
    len: usize,
}

// SAFETY: the mapping is owned by this value alone and reached only through
// `&self`/`&mut self`, as a `Box<[u8]>` would be.
unsafe impl Send for Secret {}
unsafe impl Sync for Secret {}

impl Secret {
    /// An empty secret. Maps nothing until text is written into it.
    pub const fn new() -> Self {
        Self {
            ptr: NonNull::dangling(),
            capacity: 0,
            len: 0,
        }
    }

    /// Lend the text.
    ///
    /// The borrow is the whole of the access: whatever the caller copies out
    /// of it is the caller's to wipe.
    pub fn expose(&self) -> &str {
        // SAFETY: `len` bytes from `ptr` are initialised (or `len` is zero on a
        // dangling pointer), and every write keeps them valid UTF-8.
        unsafe { std::str::from_utf8_unchecked(self.bytes()) }
    }

    /// Whether there is any text.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// A second secret holding the same text, in pages of its own.
    ///
    /// Copied from mapping to mapping with nothing on the heap in between, so
    /// the copy is as protected as the original and is wiped when it drops.
    /// For handing the text to another thread while the field keeps it — a
    /// lock screen that shows its dots while the password is being checked.
    /// A method rather than `Clone`, so that every copy is written down.
    pub fn duplicate(&self) -> Secret {
        let mut copy = Secret::new();
        copy.replace_range(0..0, self.expose());
        copy
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: see `expose`.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Replace `range` (in bytes, on character boundaries) with `with`, in
    /// place.
    ///
    /// Shifting the tail and writing the new text happens inside the mapping;
    /// bytes a shrink leaves past the end are zeroed. Only when the text no
    /// longer fits does it move, to a mapping at least twice the size, and the
    /// old one is wiped on the way out.
    pub(crate) fn replace_range(&mut self, range: Range<usize>, with: &str) {
        let text = self.expose();
        assert!(
            range.start <= range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end),
            "replace_range out of bounds or off a character boundary"
        );
        let old_len = self.len;
        let new_len = old_len - range.len() + with.len();

        if new_len > self.capacity {
            let mut grown = Self::with_capacity(new_len.max(self.capacity * 2));
            let dst = grown.ptr.as_ptr();
            let src = self.ptr.as_ptr();
            // SAFETY: `grown` maps at least `new_len` bytes; the three copies
            // cover `0..new_len` exactly, from initialised source bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(src, dst, range.start);
                std::ptr::copy_nonoverlapping(with.as_ptr(), dst.add(range.start), with.len());
                std::ptr::copy_nonoverlapping(
                    src.add(range.end),
                    dst.add(range.start + with.len()),
                    old_len - range.end,
                );
            }
            grown.len = new_len;
            // The old mapping is wiped and released by its `Drop`.
            *self = grown;
            return;
        }

        let base = self.ptr.as_ptr();
        // SAFETY: everything stays inside `0..capacity`; `copy` handles the
        // overlap of a tail moving within its own buffer.
        unsafe {
            std::ptr::copy(
                base.add(range.end),
                base.add(range.start + with.len()),
                old_len - range.end,
            );
            std::ptr::copy_nonoverlapping(with.as_ptr(), base.add(range.start), with.len());
        }
        if new_len < old_len {
            self.slice_mut(new_len..old_len).zeroize();
        }
        self.len = new_len;
    }

    /// Wipe the text, and keep the pages for what is typed next.
    pub(crate) fn clear(&mut self) {
        self.slice_mut(0..self.len).zeroize();
        self.len = 0;
    }

    fn slice_mut(&mut self, range: Range<usize>) -> &mut [u8] {
        debug_assert!(range.end <= self.capacity || range.is_empty());
        // SAFETY: inside the mapping, which this value owns exclusively.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().add(range.start), range.len()) }
    }

    /// A fresh mapping of at least `bytes`, rounded up to whole pages, locked
    /// and left out of core dumps.
    fn with_capacity(bytes: usize) -> Self {
        // Asked each time rather than kept: `sysconf` answers from the aux
        // vector without a syscall, and a cached copy would be ambient state.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let capacity = bytes.div_ceil(page).max(1) * page;
        // SAFETY: an anonymous private mapping, owned by the value returned.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert!(
            ptr != libc::MAP_FAILED,
            "could not map {capacity} bytes for a secret: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: the mapping just made. Both are advice the kernel may
        // refuse, and neither refusal makes the memory unusable: an unlocked
        // secret is still wiped, which is what swaylock settles for on EPERM.
        unsafe {
            if libc::mlock(ptr, capacity) != 0 {
                log::warn!(
                    "could not lock a secret in memory, so it may be swapped: {}",
                    std::io::Error::last_os_error()
                );
            }
            libc::madvise(ptr, capacity, libc::MADV_DONTDUMP);
        }
        Self {
            ptr: NonNull::new(ptr.cast()).expect("mmap returned null"),
            capacity,
            len: 0,
        }
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        if self.capacity == 0 {
            return;
        }
        // The whole mapping, not just the text: a shrink already zeroed its
        // tail, but wiping what was never written costs a page at most.
        self.slice_mut(0..self.capacity).zeroize();
        // SAFETY: the mapping this value made, released once.
        unsafe {
            libc::munlock(self.ptr.as_ptr().cast(), self.capacity);
            libc::munmap(self.ptr.as_ptr().cast(), self.capacity);
        }
    }
}

/// Copies the text in, then zeroes the `String` it came from.
///
/// The `String`'s own buffer is wiped before it is freed, so taking a password
/// the application already holds — from a keyring, a config file — leaves no
/// second copy behind.
impl From<String> for Secret {
    fn from(mut text: String) -> Self {
        let mut secret = Self::new();
        secret.replace_range(0..0, &text);
        text.zeroize();
        secret
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `/proc/self/smaps` entry of the mapping that holds `addr`: its
    /// `VmFlags` line, and its `Locked:` size in kB.
    fn smaps_entry(addr: usize) -> (String, usize) {
        let smaps = std::fs::read_to_string("/proc/self/smaps").expect("Linux has smaps");
        let mut inside = false;
        let mut locked = 0;
        for line in smaps.lines() {
            if let Some((range, _)) = line.split_once(' ')
                && let Some((start, end)) = range.split_once('-')
                && let (Ok(start), Ok(end)) = (
                    usize::from_str_radix(start, 16),
                    usize::from_str_radix(end, 16),
                )
            {
                inside = (start..end).contains(&addr);
                continue;
            }
            if !inside {
                continue;
            }
            if let Some(kb) = line.strip_prefix("Locked:") {
                locked = kb
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
            }
            if let Some(flags) = line.strip_prefix("VmFlags:") {
                return (flags.to_owned(), locked);
            }
        }
        panic!("no mapping holds {addr:#x}");
    }

    #[test]
    fn its_pages_are_left_out_of_core_dumps_and_locked_when_the_kernel_allows() {
        let secret = Secret::from(String::from("hunter2"));
        let (flags, locked) = smaps_entry(secret.ptr.as_ptr() as usize);
        assert!(
            flags.split_whitespace().any(|f| f == "dd"),
            "the pages are not marked MADV_DONTDUMP: {flags}"
        );
        // `mlock` may be refused (a zero RLIMIT_MEMLOCK in a container), and
        // the secret is allowed to carry on unlocked; when it was not refused,
        // the kernel must say the pages are locked.
        if flags.split_whitespace().any(|f| f == "lo") {
            assert!(locked > 0, "locked flag without locked pages");
        }
    }

    #[test]
    fn it_edits_in_place_and_keeps_its_text_across_a_regrowth() {
        let mut secret = Secret::new();
        assert!(secret.is_empty());
        assert_eq!(secret.capacity, 0, "an empty secret maps nothing");

        secret.replace_range(0..0, "held");
        secret.replace_range(2..2, "-lo-");
        assert_eq!(secret.expose(), "he-lo-ld");
        secret.replace_range(2..6, "");
        assert_eq!(secret.expose(), "held");
        let first = secret.capacity;

        let long = "x".repeat(first);
        secret.replace_range(4..4, &long);
        assert!(
            secret.capacity > first,
            "it did not move to a larger mapping"
        );
        assert!(secret.expose().starts_with("held"));
        assert_eq!(secret.expose().len(), 4 + first);

        secret.clear();
        assert!(secret.is_empty());
        assert!(
            secret.slice_mut(0..8).iter().all(|&b| b == 0),
            "clear left bytes behind"
        );
    }

    #[test]
    fn a_shrink_zeroes_what_it_leaves_past_the_end() {
        let mut secret = Secret::from(String::from("abcdefgh"));
        secret.replace_range(2..8, "");
        assert_eq!(secret.expose(), "ab");
        assert!(secret.slice_mut(2..8).iter().all(|&b| b == 0));
    }

    #[test]
    fn a_duplicate_has_pages_of_its_own_and_outlives_the_original() {
        let original = Secret::from(String::from("hunter2"));
        let copy = original.duplicate();
        assert_eq!(copy.expose(), "hunter2");
        assert_ne!(
            copy.ptr, original.ptr,
            "the copy shares the original's pages"
        );
        let (flags, _) = smaps_entry(copy.ptr.as_ptr() as usize);
        assert!(
            flags.split_whitespace().any(|f| f == "dd"),
            "the copy is not left out of core dumps"
        );
        drop(original);
        assert_eq!(copy.expose(), "hunter2");
    }

    #[test]
    fn debug_does_not_print_it() {
        let secret = Secret::from(String::from("hunter2"));
        assert_eq!(format!("{secret:?}"), "Secret(..)");
    }
}
