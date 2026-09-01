//! The rule that decides whether a run may rewrite a reference.
//!
//! `CLAUDE.md` calls it non-negotiable, and it was prose: both harnesses wrote
//! on `UPDATE_*` whatever was already on disk, and neither read `REBLESS_*` at
//! all. The rule is a pure function of three booleans now, so the table it is
//! supposed to satisfy can be written down.

mod common;

use common::{Blessing, blessing};

/// Making a reference that does not exist yet is ordinary work — a new
/// scenario needs a first picture — and `UPDATE_*` is what makes it.
#[test]
fn update_creates_a_reference_that_is_not_there() {
    assert_eq!(blessing(true, false, false), Blessing::Write);
}

/// The line the harnesses were missing. Pointed at a reference that already
/// exists, `UPDATE_*` declines and lets the comparison run, because rewriting
/// one turns a failing test green without changing anything back.
#[test]
fn update_declines_a_reference_that_already_exists() {
    assert_eq!(blessing(true, false, true), Blessing::Compare);
}

/// Rewriting is what `REBLESS_*` is for, and the only thing it is for.
#[test]
fn rebless_rewrites_whatever_is_there() {
    assert_eq!(blessing(false, true, true), Blessing::Write);
    assert_eq!(blessing(false, true, false), Blessing::Write);
}

/// Asking for both is asking for the stronger of the two. Nobody should write
/// it, and it should not mean "decline".
#[test]
fn rebless_wins_over_update() {
    assert_eq!(blessing(true, true, true), Blessing::Write);
}

/// An ordinary run compares, and never writes, whatever is on disk.
#[test]
fn an_ordinary_run_only_compares() {
    assert_eq!(blessing(false, false, true), Blessing::Compare);
    assert_eq!(blessing(false, false, false), Blessing::Compare);
}

