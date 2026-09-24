//! A password typed into a field is never left behind in freed memory.
//!
//! The allocator below is the test's instrument. It scans every block the
//! program hands back, before handing it on to the system, for the first eight
//! bytes of the password — so a copy the field made and dropped without
//! wiping is caught at the moment it is dropped, and a prefix the undo history
//! kept counts as a copy. It cannot allocate while it scans, so it compares
//! bytes in place and records what it found in an atomic.
//!
//! **One test, on purpose.** The allocator is the whole process's, and the test
//! runner's own threads free blocks too; a file is one binary, so a file with
//! one test is the only place "nothing freed held the password" means this
//! field.
//!
//! The password never exists on the heap on the test's side: it is typed one
//! `Key::Char` at a time from a `&'static str`, and compared through a borrow.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::Harness;
use guido::prelude::*;

/// Twenty characters, so a regrowth or two happens while it is typed, and
/// none of it a word the rest of the test binary might hold for its own
/// reasons.
const PASSWORD: &str = "Tr0ub4dor&3-horse-99";

/// What the scan looks for: the first eight bytes. A shorter needle would find
/// accidents; the whole password would miss the prefixes a history keeps.
const NEEDLE: &[u8] = b"Tr0ub4do";

static FOUND: AtomicBool = AtomicBool::new(false);

/// `System`, with every freed block searched first.
///
/// `realloc` is left to the trait's default, which allocates, copies and then
/// frees through `dealloc` — so a buffer that grows is searched on the way out
/// like any other.
struct Scanning;

unsafe impl GlobalAlloc for Scanning {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the block is still ours until `System.dealloc` below, and it
        // is `layout.size()` bytes long.
        let block = unsafe { std::slice::from_raw_parts(ptr, layout.size()) };
        if block.windows(NEEDLE.len()).any(|w| w == NEEDLE) {
            FOUND.store(true, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static HEAP: Scanning = Scanning;

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 40.0;

fn press(harness: &mut Harness, key: Key, modifiers: Modifiers) {
    // Each key a moment after the one before, as a real dispatch would stamp
    // it.
    let at = Instant::now() + Duration::from_millis(1);
    harness.send_at(Event::KeyDown { key, modifiers }, at);
    // A layout between keys, as the real loop runs one: it is where the field
    // reads its value back, and a read that copies is a copy.
    harness.lay_out(WIDTH, HEIGHT);
}

fn key(harness: &mut Harness, key: Key) {
    press(harness, key, Modifiers::default());
}

fn type_str(harness: &mut Harness, text: &str) {
    for c in text.chars() {
        key(harness, Key::Char(c));
    }
}

fn ctrl(harness: &mut Harness, c: char) {
    press(
        harness,
        Key::Char(c),
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
}

#[test]
fn no_freed_block_ever_held_the_password() {
    let password = create_password();
    let submitted = Rc::new(Cell::new(false));
    let seen = submitted.clone();
    // The secret is compared through the borrow, and dropped here — wiped.
    let field = password_input(password)
        .on_submit(move |secret: Secret| seen.set(secret.expose() == PASSWORD));

    let mut harness = Harness::focused(field, WIDTH, HEIGHT);

    type_str(&mut harness, PASSWORD);
    // Back into the middle, and the second half again.
    for _ in 0..10 {
        key(&mut harness, Key::Backspace);
    }
    type_str(&mut harness, &PASSWORD[10..]);
    // Everything, replaced by itself.
    ctrl(&mut harness, 'a');
    type_str(&mut harness, PASSWORD);
    key(&mut harness, Key::Enter);
    assert!(submitted.get(), "Enter did not hand over what was typed");
    assert!(
        password.with_untracked(Secret::is_empty),
        "the field kept the password after handing it over"
    );

    // Once more, and wiped by the application rather than submitted.
    type_str(&mut harness, PASSWORD);
    password.clear();
    harness.lay_out(WIDTH, HEIGHT);

    // Pre-filled from a `String` the application held — a keyring's answer —
    // which `From` wipes on the way in. `String::from` puts the password on
    // the heap on purpose, here and only here.
    password.set(Secret::from(String::from(PASSWORD)));
    harness.lay_out(WIDTH, HEIGHT);
    // And replaced, so the one set above is dropped.
    password.set(Secret::new());

    drop(harness);

    assert!(
        !FOUND.load(Ordering::Relaxed),
        "a block holding the password was freed without being wiped"
    );
}
