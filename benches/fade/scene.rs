//! Cards fading, which is what a popup, a toast or a panel does on its way in
//! and out.
//!
//! Each card is a background with a label on it, so its draws overlap and a
//! fade composites it as one layer — the case `benches/scroll_list` and its
//! siblings never draw, because none of them fades anything. The script both
//! benchmarks play is a list of scroll deltas, so the cards fade by them: the
//! surface hears the wheel and turns the distance travelled into an opacity,
//! and every scripted delta is a frame of the fade.
//!
//! Three ways to fade the same cards, because the question is what the layer
//! costs against what it replaces: [`Fade::Group`] is the default, a layer
//! per card; [`Fade::Modulate`] is the per-draw fade it replaces, which
//! draws an overlap wrong and draws nothing offscreen; [`Fade::Blur`] holds
//! the opacity and moves a blur instead, which runs the blur over the texture
//! every frame and draws nothing into it.

use guido::prelude::*;

/// The surface, in logical pixels — tall enough for the pointer the script
/// aims at.
pub const VIEWPORT: (u32, u32) = (600, 800);

const CARD_HEIGHT: f32 = 56.0;
const GAP: f32 = 8.0;
const PAD: f32 = 12.0;

/// How the cards fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fade {
    /// `opacity`, as declared: each card one layer.
    Group,
    /// `opacity` with [`OpacityMode::Modulate`]: each draw on its own.
    Modulate,
    /// A blur that moves at a fixed opacity of one.
    Blur,
}

impl Fade {
    /// The word this scenario is named by in the table.
    pub fn name(self) -> &'static str {
        match self {
            Fade::Group => "group-opacity",
            Fade::Modulate => "modulated-opacity",
            Fade::Blur => "moving-blur",
        }
    }

    /// Every scenario, in the order the benchmark prints them.
    pub const ALL: [Fade; 3] = [Fade::Group, Fade::Modulate, Fade::Blur];
}

/// Where the travelled distance puts the fade: a triangle wave between a
/// tenth and nine tenths, a full sweep every 1200 pixels — so the slow phase
/// of the script crawls along it and the flick runs it back and forth.
fn phase(travelled: f32) -> f32 {
    let at = (travelled / 1200.0).rem_euclid(1.0);
    0.1 + 0.8 * (1.0 - (2.0 * at - 1.0).abs())
}

/// `cards` cards in a column, fading by the wheel.
pub fn scene(cards: usize, fade: Fade) -> Container {
    let travelled = create_signal(0.0f32);
    container()
        .width(fill())
        .height(fill())
        .padding(PAD)
        .layout(Flex::column().spacing(GAP))
        .on_scroll(move |_, dy, _| travelled.update(|at| *at += dy))
        .children((0..cards).map(move |index| card(index, fade, travelled)))
}

fn card(index: usize, fade: Fade, travelled: RwSignal<f32>) -> Container {
    let card = container()
        .width(fill())
        .height(CARD_HEIGHT)
        .padding(PAD)
        .corners(10.0)
        .background(Color::rgb(0.20, 0.22, 0.30))
        .child(
            text(format!("card {index}"))
                .font_size(18.0)
                .color(Color::WHITE),
        );
    match fade {
        Fade::Group => card.opacity(move || phase(travelled.get())),
        Fade::Modulate => card
            .opacity(move || phase(travelled.get()))
            .opacity_mode(OpacityMode::Modulate),
        Fade::Blur => card.blur(move || 8.0 * phase(travelled.get())),
    }
}
