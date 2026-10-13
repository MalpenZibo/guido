#![cfg(feature = "testing")]
//! A scrollbar handle's press ripple stays inside its rounded scroller (#708).
//!
//! The handle is a child of the scroller and is cut by its clip, arc included.
//! Its ripple is an overlay, cut to the handle's own pill — and it used to be
//! cut by nothing else, so pressing and holding the handle near a rounded
//! corner drew the ripple past the scroller's arc: 22 pixels at scale 1 on
//! the press, a few hundred once the handle was dragged into the corner.
//!
//! Read off the frame `Headless` draws, so the whole of it — paint, flatten,
//! the clip tree and the shader — is what answers.

mod common;

use guido::prelude::*;
use std::time::{Duration, Instant};

const BACKGROUND: Color = Color::rgb(0.1, 0.1, 0.12);

/// A 260-square scroller with 70-pixel corners at (20, 20), and rows enough
/// to scroll.
fn scroller() -> Container {
    let rows = (0..12).map(|i| {
        container().height(50.0).background(if i % 2 == 0 {
            Color::rgb(0.85, 0.35, 0.3)
        } else {
            Color::rgb(0.3, 0.55, 0.85)
        })
    });
    container().padding(20.0).child(
        container()
            .width(260.0)
            .height(260.0)
            .corners(70.0)
            .background(Color::rgb(0.2, 0.2, 0.25))
            .scroll(Scroll::vertical().width(14.0))
            .child(container().layout(Flex::column()).children(rows)),
    )
}

/// Physical pixels drawn outside the scroller's two right-hand corner arcs,
/// with two logical pixels of slack for the edge's antialiasing.
fn outside_the_arc(frame: &[u8], width: u32, height: u32, scale: f32) -> usize {
    let (right, top, bottom, radius) = (280.0, 20.0, 280.0, 70.0);
    let mut count = 0;
    for py in 0..height {
        for px in 0..width {
            let (x, y) = ((px as f32 + 0.5) / scale, (py as f32 + 0.5) / scale);
            let in_corner_box = x >= right - radius
                && x < right
                && ((y >= top && y < top + radius) || (y >= bottom - radius && y < bottom));
            if !in_corner_box {
                continue;
            }
            let cy = if y < top + radius {
                top + radius
            } else {
                bottom - radius
            };
            if (x - (right - radius)).hypot(y - cy) < radius + 2.0 {
                continue;
            }
            let i = ((py * width + px) * 4) as usize;
            let background = [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b].map(|c| (c * 255.0) as i32);
            let off: i32 = (0..3)
                .map(|c| (frame[i + c] as i32 - background[c]).abs())
                .sum();
            if off > 12 {
                count += 1;
            }
        }
    }
    count
}

/// Press the handle near the top-right corner, hold it, and drag it down into
/// the bottom-right one; the worst frame on the way.
fn worst_frame_while_held(scale: f32) -> Option<usize> {
    let mut app = common::headless()?;
    let (w, h) = (300, 300);
    let surface = app.surface(
        SurfaceConfig::new()
            .width(w)
            .height(h)
            .background_color(BACKGROUND),
        scroller,
    );
    app.configure(surface, w, h, scale);

    let mut at = Instant::now();
    app.step_at(at);
    let mut send = |app: &mut guido::testing::Headless, event: Event| {
        at += Duration::from_millis(16);
        app.event_at(surface, event, at);
        for _ in 0..3 {
            at += Duration::from_millis(16);
            app.step_at(at);
        }
    };
    let (pw, ph) = app.physical_size(surface);
    let mut worst = 0;
    let mut look = |app: &guido::testing::Headless| {
        worst = worst.max(outside_the_arc(&app.read_frame(surface), pw, ph, scale));
    };

    send(&mut app, Event::mouse_enter(272.0, 84.0));
    send(&mut app, Event::mouse_move(272.0, 84.0));
    send(&mut app, Event::mouse_down(272.0, 84.0, MouseButton::Left));
    look(&app);
    for step in 0..30 {
        send(&mut app, Event::mouse_move(272.0, 84.0 + step as f32 * 6.0));
        look(&app);
    }
    Some(worst)
}

#[test]
fn a_held_handle_s_ripple_is_cut_by_the_scroller_s_arc() {
    for scale in [1.0, 2.0] {
        let Some(worst) = worst_frame_while_held(scale) else {
            return;
        };
        assert_eq!(
            worst, 0,
            "at scale {scale}, the handle's press ripple drew {worst} physical \
             pixels past the scroller's rounded corner"
        );
    }
}
