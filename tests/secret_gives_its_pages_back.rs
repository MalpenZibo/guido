//! A dropped `Secret` gives its pages back.
//!
//! **One test, on purpose.** What it asks is whether any mapping of the process
//! still holds the address the secret's pages were at, and the process is the
//! test binary. Beside other tests, a thread that maps a page of its own — a
//! `Secret` of another test, most often — can be handed the address the drop
//! just freed, and the question answers for it (#527). A file is one binary,
//! so a file with one test is the only place the answer is about this secret.
//!
//! That the pages are wiped before they go is watched in `src/secret.rs`, by
//! `wiping_zeroes_every_byte_of_the_mapping`.

use guido::prelude::*;

/// Whether any mapping of this process holds `addr`.
fn mapped(addr: usize) -> bool {
    let maps = std::fs::read_to_string("/proc/self/maps").expect("Linux has maps");
    maps.lines().any(|line| {
        let range = line.split(' ').next().unwrap_or_default();
        let (start, end) = range.split_once('-').unwrap_or_default();
        match (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) {
            (Ok(start), Ok(end)) => (start..end).contains(&addr),
            _ => false,
        }
    })
}

#[test]
fn dropping_gives_the_pages_back() {
    let secret = Secret::from(String::from("hunter2"));
    let addr = secret.expose().as_ptr() as usize;
    assert!(mapped(addr), "the text lives in the secret's own mapping");
    drop(secret);
    assert!(!mapped(addr), "the mapping outlived its secret");
}
