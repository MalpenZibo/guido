//! The corner geometry is written three times, and the three have to agree.
//!
//! WGSL has no include. A clip is one shape and a corner is one curve, but the
//! pipelines that cut them are three — the shape shader, the textured quad
//! (images and transformed text), and the backdrop composite — and each carries
//! its own copy of the signed distance function.
//!
//! Copies drift. #403 is what that looked like: the backdrop read the curvature
//! as `max(k, 0.05) * 2` where the other two read `pow(2, k)`. The two
//! expressions agree at exactly k = 1 and k = 2 — `Corners::rounded` and
//! `Corners::squircle` — which is every curvature any golden drew *with a
//! backdrop blur*, so a bevel blurred with a concave notch inside an octagonal
//! border and nothing objected for as long as the file existed.
//! (`corner_curvature_family` does draw a bevel, a scoop and k = 1.5; it draws
//! them with the shape shader, which was right.)
//!
//! A golden catches the disagreement once somebody draws the case where it
//! shows. This catches it at the character, without anyone having to think of
//! the scenario.
//!
//! **There is a fourth copy, and it is not WGSL.** `Rect::shape_distance` in
//! `src/widgets/widget.rs` is the same function in Rust, and it is what
//! hit-testing runs — so it decides where a click lands the way the shaders
//! decide where a pixel lands. It cannot be compared character for character
//! against WGSL, so the last test here pins it to the definition instead: the
//! point that sits exactly on the superellipse is bracketed a twentieth of a
//! pixel either side, and the boundary must fall between. Measuring at the
//! point itself is what it used to do, and cannot be: the computed distance
//! there lands within four millionths of a pixel of zero, so which side it
//! falls on is the last bit of two `powf` calls. Change the
//! curve and that fails too.
//!
//! **Why three copies rather than one.** All three shaders are loaded with
//! `include_str!`, so `concat!` with a shared prelude file would make one copy
//! genuinely be one copy, with no build script and no dependency. It is not
//! free: naga reports errors by line and column, and every diagnostic in every
//! shader would then be off by the prelude's length; and a `.wgsl` file stops
//! being valid on its own, which is what an editor or `wgsl-analyzer` reads.
//! Paying a string comparison to keep the files standalone is the trade, and it
//! is reversible in an afternoon if the diagnostics matter less than they seem.

use std::path::Path;

/// The files that must carry the block, and the markers that delimit it.
const SHADERS: [&str; 3] = [
    "src/renderer/shader.wgsl",
    "src/renderer/textured_quad_shader.wgsl",
    "src/renderer/backdrop_shader.wgsl",
];
const START: &str = "// === SHARED SDF";
const END: &str = "// === END SHARED SDF ===";

fn block(path: &str) -> String {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    let start = source
        .find(START)
        .unwrap_or_else(|| panic!("{path} has no `{START}` marker — every shader that cuts a corner carries the shared block"));
    let end = source.find(END).unwrap_or_else(|| {
        panic!("{path} opens the shared block and never closes it with `{END}`")
    });
    assert!(
        end > start,
        "{path}: the shared block's markers are inverted"
    );
    source[start..end + END.len()].to_owned()
}

/// Character for character, comments included — because a comment that drifts
/// is how the next reader learns the wrong thing, and because an exact
/// comparison is the only one nobody has to maintain.
#[test]
fn every_shader_that_cuts_a_corner_cuts_the_same_one() {
    let reference = block(SHADERS[0]);
    for path in &SHADERS[1..] {
        let other = block(path);
        if other != reference {
            let mismatch = reference
                .lines()
                .zip(other.lines())
                .position(|(a, b)| a != b)
                .map(|i| format!("first differing line is {} of the block", i + 1))
                .unwrap_or_else(|| "one is a prefix of the other".to_owned());
            panic!(
                "{path} and {} have drifted apart: {mismatch}.\n\
                 Copy the block between `{START}` and `{END}` from one to the other — \
                 every shader cuts the same corner, and #403 is what happens when they do not. \
                 `Rect::shape_distance` in src/widgets/widget.rs is a fourth copy in Rust; \
                 if the curve itself is changing, it changes too.",
                SHADERS[0]
            );
        }
    }
}

/// The block is only shared if it is actually reached. A file could carry it
/// and go on calling something else.
#[test]
fn every_shader_calls_the_shared_function_it_carries() {
    for path in &SHADERS {
        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap();
        let calls = source.matches("rounded_rect_sdf(").count();
        assert!(
            calls >= 2,
            "{path} defines `rounded_rect_sdf` and never calls it — \
             the shared block is carried but some other corner is being cut"
        );
    }
}

/// The fourth copy answers to the same definition.
///
/// `Rect::shape_distance` is `rounded_rect_sdf` written in Rust, and it is what
/// decides whether a click landed inside a widget. It cannot be compared with
/// the WGSL character for character, so it is compared with the geometry: for a
/// superellipse `|x|^n + |y|^n = r^n` with `n = 2^k`, the point on the corner's
/// diagonal sits at `r · 2^(-1/n)` from the arc's centre on each axis, and its
/// distance to the edge is zero.
///
/// That point is bracketed rather than asserted on directly. Its computed
/// distance lands within four millionths of a pixel of zero at every `k`
/// here, and which side of zero is the last bit of two `powf` calls — a
/// coin flip, not a statement about the curve. A twentieth of a pixel either
/// way is not: at every `k` the measured distance there is at least 0.05, so
/// the bracket says where the boundary is to twenty times the precision the
/// half-pixel one did. The `<=` that puts the boundary itself inside is
/// checked on the straight edge instead, where the distance is exactly zero
/// by construction and no rounding is involved.
///
/// Every curvature the public API names, one between them because `Corners`
/// interpolates, and on up to `k = 8`. The ceiling is not decoration:
/// `Corners::superellipse` takes any `f32`, and raising a 40-pixel offset to
/// `n = 2^8` is `40^256`, far outside `f32`. A norm that computes that
/// directly returns `inf` whatever the other offset holds, `inf - r` is
/// `inf`, and every point the corner box reaches reads as outside — which is
/// this copy's own answer and what stops the shape taking clicks over all but
/// its inner square.
#[test]
fn hit_testing_measures_the_same_corner_the_shaders_draw() {
    use guido::widgets::{Corners, Rect};

    let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
    let r = 40.0_f32;

    for k in [0.0_f32, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0] {
        let n = 2f32.powf(k);
        // On the corner diagonal, |x| = |y| = q and 2·q^n = r^n.
        let q = r * 2f32.powf(-1.0 / n);
        // The top-left arc's centre, then out along the diagonal.
        let (x, y) = (r - q, r - q);

        let corners = Corners::superellipse(r, k);
        let just_inside = rect.contains_shape(x + 0.05, y + 0.05, corners);
        let just_outside = rect.contains_shape(x - 0.05, y - 0.05, corners);

        assert!(
            just_inside,
            "k={k}: a point a twentieth of a pixel inside the corner reads as outside it — \
             at k above about 4 this is the norm overflowing to inf, which puts every \
             fragment in the corner outside the shape"
        );
        assert!(
            !just_outside,
            "k={k}: a point a twentieth of a pixel outside the corner reads as inside it — \
             hit-testing is measuring a different curve from the one the shaders draw"
        );
        // The boundary itself is inside, since the test is `distance <= 0`.
        // Taken on the straight edge, where the distance is exactly zero.
        assert!(
            rect.contains_shape(100.0, 0.0, corners),
            "k={k}: the edge itself should count as inside"
        );
    }
}
