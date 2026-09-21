//! Pixel goldens: the render path end to end, on a real GPU, with no
//! compositor anywhere.
//!
//! `render_snapshots.rs` stops at the render tree — it says what would be
//! drawn and where. Everything after that point (the SDF shaders, corner
//! curvature, border and shadow rasterization, gradients, clipping, the
//! backdrop blur passes, HiDPI scaling) has no oracle but a human looking at
//! a screenshot. These tests give it one: each scenario is laid out, painted,
//! flattened and rendered into a texture this test owns, read back, and
//! compared pixel by pixel against a committed PNG.
//!
//! **The font is part of the golden too.** Text metrics depend on the fonts
//! installed on the machine, which is why the render-tree snapshots leave text
//! out entirely. These do not: a font is vendored under `tests/assets/` and
//! registered before any renderer exists, and every scenario that draws text
//! names it. Nothing here can reach a system font, so nothing here depends on
//! which machine it ran on.
//!
//! That matters because transformed text takes a path of its own — rasterised
//! to a texture and drawn as a quad rather than handed to glyphon — and until
//! there was a font, no golden had put a single glyph through it.
//!
//! **The rasterizer is part of the golden.** Two GPUs do not antialias an
//! edge identically, so a golden blessed on a desktop GPU cannot be verified
//! in CI. The reference is lavapipe, Mesa's software Vulkan implementation —
//! the same one wgpu itself tests against. Install it (`vulkan-swrast` on
//! Arch, `mesa-vulkan-drivers` on Debian/Ubuntu) and point the loader at it:
//!
//! ```sh
//! export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
//! cargo test --test golden_images
//! ```
//!
//! Its *version* is not, and cannot be: `ubuntu-latest` installs whatever Mesa
//! its archive holds that day, and no runner label pins a package version. Two
//! versions of lavapipe agree exactly on the shapes and disagree slightly on
//! glyph antialiasing — measured between LLVM 20.1.2 on the runner and LLVM
//! 22.1.8 on a developer's machine, the whole disagreement across a text
//! scenario was four pixels of seventy-nine thousand, three units of 255 each.
//! `TOLERANCE` is set to absorb that and nothing larger.
//!
//! If the drift ever grows past it, the answer is to pin the rasterizer for
//! real — a container running the goldens with one fixed Mesa, in CI and on
//! developer machines alike — and not to widen the tolerance again. Widening it
//! twice is how a threshold stops meaning anything.
//!
//! On any other adapter — no Vulkan at all, or a desktop GPU — these skip
//! rather than fail. A golden holds only against the rasterizer it was blessed
//! on, so running it elsewhere reports a few dozen moved pixels on every
//! scenario, every time, and none of them is a regression. Skipping is what
//! lets `cargo test` on a developer's machine still mean something. CI sets
//! `GUIDO_GOLDEN_REQUIRED=1` in the one job that must not skip, where a skip is
//! a failure — a skipped test that reports green is worth less than no test.
//!
//! **Creating a reference is not rewriting one.** A scenario with no picture
//! needs one, and making it is ordinary work:
//!
//! ```sh
//! UPDATE_GOLDEN=1 cargo test --test golden_images
//! ```
//!
//! Pointed at a reference that already exists, it declines and lets the
//! comparison run instead — because rewriting a picture that disagrees with the
//! code makes a failing test pass without changing anything back. That is
//! `REBLESS_GOLDEN=1`, and the rule both harnesses read is `common::blessing`;
//! a hook refuses it, and CI refuses a pull request that
//! rewrites a reference without the `golden-update` label. So it is always
//! somebody's decision, taken with the diff in front of them. On failure the
//! three images (expected, actual, and a map of what moved) are written to
//! `target/golden-failures/` and uploaded as a CI artifact.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod common;

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{GpuContext, RenderNode, Renderer, flatten_root_into};
use guido::tree::Tree;
use guido::widgets::Widget;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Non-sRGB 8-bit RGBA: the same family of format the Wayland surface picks,
/// so what is read back here is what the compositor would have been handed.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Vendored so the goldens do not depend on what a machine happens to have
/// installed: DejaVu Sans Mono, under the licence beside it. Registered per
/// test thread, because the registry is thread-local and each test builds its
/// own renderer.
const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");
const FONT_FAMILY: &str = "DejaVu Sans Mono";

/// Per-channel difference below which two pixels are the same pixel.
///
/// Two was enough while every scenario drew shapes: on one rasterizer the
/// expected difference is zero, and two absorbed the last of the floating-point
/// noise. Glyphs are not shapes — their antialiasing is where two versions of
/// lavapipe legitimately disagree, by three units on four pixels of a text
/// scenario, while the shapes stayed identical to the last bit.
///
/// So four, which is a recalibration against evidence the original number was
/// not chosen with, and not a way past a failure: a regression moves geometry,
/// and geometry moves a pixel by tens or hundreds. Four is still far below
/// anything a person can see.
const TOLERANCE: u8 = 4;

/// One device for the whole test binary: creating it costs more than every
/// scenario put together.
fn ctx() -> Option<&'static GpuContext> {
    static CTX: OnceLock<Option<GpuContext>> = OnceLock::new();
    CTX.get_or_init(|| {
        let _ = env_logger::builder().is_test(true).try_init();
        GpuContext::try_new()
    })
    .as_ref()
}

/// What the pixels came out of, for the bless guard and for failure reports.
fn adapter_name(ctx: &GpuContext) -> String {
    pollster::block_on(ctx.instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map(|adapter| adapter.get_info().name)
    .unwrap_or_else(|_| "<unknown>".to_string())
}

fn is_software_rasterizer(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("llvmpipe") || name.contains("lavapipe") || name.contains("swiftshader")
}

/// An image as it comes off the GPU: tightly packed RGBA8, no padding.
struct Pixels {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Lay out, paint, flatten and render `widget` into a texture, and read it
/// back. `logical` is the viewport in logical pixels, the unit layout speaks;
/// `scale` is the HiDPI factor, so the texture is `logical * scale`.
fn render_pixels(
    ctx: &GpuContext,
    renderer: &mut Renderer,
    widget: impl Widget + 'static,
    logical: (f32, f32),
    scale: f32,
    clear: Color,
) -> Pixels {
    let (logical_width, logical_height) = logical;
    // The rule `surface::buffer_size` applies to a Wayland buffer, spelled
    // again rather than called: that one is `pub(crate)` and takes a logical
    // size in whole pixels, and a scenario names its viewport in the `f32`
    // layout speaks. Both round halfway away from zero, which is what a
    // scenario at a fractional scale depends on.
    let width = (logical_width * scale).round() as u32;
    let height = (logical_height * scale).round() as u32;

    // Layout and paint: the same two passes `render_snapshots` runs, and
    // neither needs a compositor or a GPU.
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.layout_widget(
        root,
        Constraints::new(0.0, 0.0, logical_width, logical_height),
    );

    let mut node = RenderNode::new(root.as_u64());
    tree.paint_widget(root, &mut node);

    let mut commands = Vec::new();
    let mut layers = Vec::new();
    let _ = flatten_root_into(&node, &mut commands, &mut layers);

    let device = ctx.device.clone();
    let queue = ctx.queue.clone();

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("golden target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    renderer.set_screen_size(width as f32, height as f32);
    renderer.set_scale_factor(scale);
    renderer.render_to_view(&view, width, height, &commands, &layers, clear);

    // Readback. Rows in a mapped buffer are padded to 256 bytes; the copy is
    // made against the padded stride and unpadded on the way out.
    let unpadded = width * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("golden readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("golden readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU never finished the readback");
    rx.recv()
        .expect("the map callback never fired")
        .expect("the readback buffer would not map");

    let mapped = slice.get_mapped_range();
    let mut data = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        data.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();

    Pixels {
        width,
        height,
        data,
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"))
}

fn failures_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("golden-failures")
}

fn write_png(path: &Path, pixels: &Pixels) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    ::image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.data.clone())
        .expect("pixel buffer does not match its own dimensions")
        .save(path)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
}

/// Where the two images disagree, in magenta over a dimmed copy of what was
/// rendered — so the map reads as "here, on this frame", not as noise.
fn diff_image(expected: &Pixels, actual: &Pixels) -> Pixels {
    let (width, height) = (actual.width as i32, actual.height as i32);

    let changed: Vec<bool> = expected
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.data.as_chunks::<4>().0.iter())
        .map(|(e, a)| (0..4).any(|c| e[c].abs_diff(a[c]) > TOLERANCE))
        .collect();

    // Dilated by a pixel: thirty pixels moved on an antialiased edge are
    // invisible at natural size, and a picture nobody can read is not a signal.
    let near = |x: i32, y: i32| {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0
                    && ny >= 0
                    && nx < width
                    && ny < height
                    && changed[(ny * width + nx) as usize]
            })
        })
    };

    let mut data = Vec::with_capacity(actual.data.len());
    for y in 0..height {
        for x in 0..width {
            if near(x, y) {
                data.extend_from_slice(&[255, 0, 255, 255]);
            } else {
                let a = &actual.data[((y * width + x) * 4) as usize..][..4];
                let dim = ((u16::from(a[0]) + u16::from(a[1]) + u16::from(a[2])) / 9 + 24) as u8;
                data.extend_from_slice(&[dim, dim, dim, 255]);
            }
        }
    }

    Pixels {
        width: actual.width,
        height: actual.height,
        data,
    }
}

/// The one assertion every scenario ends in.
fn assert_golden(name: &str, adapter: &str, actual: Pixels) {
    let path = golden_path(name);

    let blessing = common::blessing_from_env("GOLDEN", path.exists());
    if common::write_if_blessed(&path, blessing, || {
        assert!(
            is_software_rasterizer(adapter)
                || std::env::var_os("GUIDO_GOLDEN_ANY_ADAPTER").is_some(),
            "refusing to bless `{name}` on `{adapter}`.\n\
             Goldens are compared against lavapipe in CI, and no two rasterizers \
             antialias an edge the same way, so a golden blessed here would fail \
             there for reasons that have nothing to do with the change.\n\
             Install lavapipe and re-run under the same variable, on lavapipe:\n  \
             VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json"
        );
        write_png(&path, &actual);
        eprintln!("blessed {name} ({adapter})");
    }) {
        return;
    }

    let expected = ::image::open(&path)
        .unwrap_or_else(|_| {
            panic!(
                "missing golden `{name}`. Create it with:\n  \
                 UPDATE_GOLDEN=1 cargo test --test golden_images"
            )
        })
        .to_rgba8();

    let expected = Pixels {
        width: expected.width(),
        height: expected.height(),
        data: expected.into_raw(),
    };

    assert_eq!(
        (expected.width, expected.height),
        (actual.width, actual.height),
        "golden `{name}` is {}x{} but the scenario rendered {}x{}",
        expected.width,
        expected.height,
        actual.width,
        actual.height
    );

    // The report is what somebody reads at 3am, or what an agent reads
    // instead of guessing: how much moved, by how much, and where.
    let mut changed = 0usize;
    let mut worst = 0u8;
    let mut first = Vec::new();
    for (index, (e, a)) in expected
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(actual.data.as_chunks::<4>().0.iter())
        .enumerate()
    {
        let delta = (0..4).map(|c| e[c].abs_diff(a[c])).max().unwrap_or(0);
        if delta > TOLERANCE {
            changed += 1;
            worst = worst.max(delta);
            if first.len() < 6 {
                let (x, y) = (index as u32 % actual.width, index as u32 / actual.width);
                first.push(format!(
                    "({x},{y}) expected rgba{:?} got rgba{:?}",
                    (e[0], e[1], e[2], e[3]),
                    (a[0], a[1], a[2], a[3])
                ));
            }
        }
    }

    if changed > 0 {
        let dir = failures_dir();
        write_png(&dir.join(format!("{name}.expected.png")), &expected);
        write_png(&dir.join(format!("{name}.actual.png")), &actual);
        write_png(
            &dir.join(format!("{name}.diff.png")),
            &diff_image(&expected, &actual),
        );

        let total = (actual.width * actual.height) as f64;
        panic!(
            "`{name}` renders differently: {changed} of {total:.0} pixels \
             ({:.3}%), worst channel delta {worst}, on adapter `{adapter}`.\n\
             {}\n\
             Images written to {}\n\
             If the change is intended, re-bless on lavapipe with \
             REBLESS_GOLDEN=1 and put the diff in the pull request — that diff \
             is the review.",
            100.0 * changed as f64 / total,
            first.join("\n"),
            dir.display()
        );
    }
}

/// Render one scenario and hold it against its golden.
fn golden(
    name: &str,
    logical: (f32, f32),
    scale: f32,
    clear: Color,
    widget: impl Widget + 'static,
) {
    let required = std::env::var_os("GUIDO_GOLDEN_REQUIRED").is_some();

    let Some(ctx) = ctx() else {
        assert!(
            !required,
            "`{name}` needs a Vulkan adapter and found none, and \
             GUIDO_GOLDEN_REQUIRED is set. In CI this means lavapipe is not \
             installed, or VK_ICD_FILENAMES does not point at it."
        );
        eprintln!("skipping golden `{name}`: no Vulkan adapter on this machine");
        return;
    };

    // A golden holds only against the rasterizer it was blessed on, so running
    // it on anything else produces a failure that is not a regression — a few
    // dozen pixels on the corner tangents, every time, for every scenario. On
    // another adapter it skips instead, which is what makes `cargo test` on a
    // machine with a GPU mean something. The job that must not skip says so.
    let adapter = adapter_name(ctx);
    if !is_software_rasterizer(&adapter) && std::env::var_os("GUIDO_GOLDEN_ANY_ADAPTER").is_none() {
        assert!(
            !required,
            "`{name}` ran on `{adapter}`, and GUIDO_GOLDEN_REQUIRED is set. The \
             goldens are blessed against lavapipe; point VK_ICD_FILENAMES at it."
        );
        eprintln!(
            "skipping golden `{name}`: `{adapter}` is not the rasterizer these \
             were blessed on. Point VK_ICD_FILENAMES at lavapipe to run them."
        );
        return;
    }

    // The font registry is thread-local and is read once, when a font system
    // is first built. So it is filled on this thread, before the renderer that
    // will read it exists. Registering the same bytes twice costs nothing.
    guido::load_font(FONT.to_vec());

    // A renderer per test, and dropped before the test returns. It caches
    // with `Rc` — the library it belongs to draws from one thread — so it
    // cannot be shared between test threads, and keeping one per thread in a
    // `thread_local` drops it during TLS teardown, where the state its own
    // drop reaches has already been torn down. The device it draws with is
    // still the shared one, which is the part that costs.
    let mut renderer = Renderer::new(ctx.device.clone(), ctx.queue.clone(), FORMAT);
    let pixels = render_pixels(ctx, &mut renderer, widget, logical, scale, clear);
    drop(renderer);

    assert_golden(name, &adapter, pixels);
}

const BACKDROP: Color = Color::rgb(0.08, 0.08, 0.10);

fn box_of(w: f32, h: f32) -> Container {
    container().width(w).height(h)
}

fn swatch(w: f32, h: f32, c: Color) -> Container {
    box_of(w, h).background(c)
}

/// A backdrop with nowhere to hide: two colours far enough apart that a blur
/// of them is neither, so what the effect reached is legible at a glance.
///
/// Shared by every backdrop scenario, so the frost a container cuts and the
/// frost a text cuts are judged against the same picture.
fn stripes(width: f32, count: usize, bar: f32) -> Container {
    let bars: Vec<AnyWidget> = (0..count)
        .map(|i| {
            let colour = if i % 2 == 0 {
                Color::rgb(0.85, 0.35, 0.30)
            } else {
                Color::rgb(0.15, 0.45, 0.85)
            };
            swatch(width, bar, colour).into_any()
        })
        .collect();
    container()
        .width(width)
        .height(fill())
        .layout(Flex::column())
        .children(bars)
}

/// Always the vendored family, never whatever the machine happens to have.
fn label(content: &str, size: f32) -> Text {
    text(content)
        .font_family(FontFamily::Name(FONT_FAMILY.into()))
        .font_size(size)
        .color(Color::WHITE)
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// The five corner curvatures, at a radius large enough that the shape of the
/// curve — not just its extent — is on the pixels. This is the SDF the whole
/// shape pipeline is built on.
#[test]
fn corner_curvature_family() {
    let base = |c: Color| swatch(90.0, 90.0, c).corners(28.0);
    let blue = Color::rgb(0.30, 0.55, 0.95);

    let view = container()
        .background(BACKDROP)
        .padding(16.0)
        .layout(Flex::row().spacing(16.0))
        .child(base(blue))
        .child(base(blue).corners(Corners::squircle(28.0)))
        .child(base(blue).corners(Corners::bevel(28.0)))
        .child(base(blue).corners(Corners::scoop(28.0)))
        .child(base(blue).corners(Corners::superellipse(28.0, 1.5)));

    golden(
        "corner_curvature_family",
        (570.0, 122.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// The curvatures above the ledge where the norm used to overflow.
///
/// `superellipse_length` raises each offset to `n = 2^k` before taking the
/// root. A 28-pixel offset at `k = 5` is `28^32`, about `10^46`, and `f32`
/// stops at `3.4 × 10^38` — so the sum was `inf` whatever the other offset
/// held, and the distance was `inf` for every fragment the corner box
/// reaches, the straight sides included.
///
/// What that drew is not a missing corner. `inf` reaches `fwidth`, and on
/// lavapipe the antialiasing collapses: the three high-k swatches came out as
/// hard-edged squares a pixel wider all round, which is why this golden's
/// difference against the unfixed shader is a one-pixel outline about each of
/// them rather than four bites. Undefined is undefined and another adapter
/// may do something else — what this golden pins is that on the one the
/// project blesses against, the picture is the curve and not an artefact.
///
/// `Corners::superellipse` is public and takes any `f32`, so this range is
/// reachable by anybody who asks for it, and by any transition whose endpoint
/// is in it. The curve here is barely distinguishable from a square corner —
/// that is what a superellipse *is* as `n` grows — and drawing very nearly a
/// square, antialiased, is the correct answer.
#[test]
fn corners_above_the_overflow_ledge() {
    let base = |k: f32| {
        swatch(90.0, 90.0, Color::rgb(0.30, 0.55, 0.95)).corners(Corners::superellipse(28.0, k))
    };

    let view = container()
        .background(BACKDROP)
        .padding(16.0)
        .layout(Flex::row().spacing(16.0))
        .child(base(3.0))
        .child(base(4.0))
        .child(base(5.0))
        .child(base(6.0))
        .child(base(8.0));

    golden(
        "corners_above_the_overflow_ledge",
        (570.0, 122.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// Borders at the widths where the SDF is easiest to get wrong — one pixel,
/// and wide enough to meet itself around a tight corner — over gradients, so
/// the fill and the stroke are on the same pixels.
#[test]
fn borders_and_gradients() {
    let card = |w: f32, radius: f32| {
        box_of(120.0, 80.0)
            .corners(radius)
            .border(w, Color::rgb(0.95, 0.85, 0.35))
            .gradient(LinearGradient::vertical(
                Color::rgb(0.20, 0.35, 0.75),
                Color::rgb(0.75, 0.20, 0.45),
            ))
    };

    let view = container()
        .background(BACKDROP)
        .padding(16.0)
        .layout(Flex::column().spacing(16.0))
        .child(
            container()
                .layout(Flex::row().spacing(16.0))
                .child(card(1.0, 0.0))
                .child(card(3.0, 12.0))
                .child(card(10.0, 30.0)),
        )
        .child(
            container()
                .layout(Flex::row().spacing(16.0))
                .child(
                    box_of(120.0, 80.0)
                        .corners(12.0)
                        .gradient(LinearGradient::horizontal(
                            Color::rgb(0.10, 0.60, 0.50),
                            Color::rgb(0.90, 0.90, 0.20),
                        )),
                )
                .child(
                    box_of(120.0, 80.0)
                        .corners(Corners::squircle(24.0))
                        .gradient(LinearGradient::diagonal(
                            Color::rgb(0.90, 0.30, 0.10),
                            Color::rgb(0.10, 0.10, 0.60),
                        ))
                        .border(2.0, Color::WHITE),
                ),
        );

    golden("borders_and_gradients", (440.0, 224.0), 1.0, BACKDROP, view);
}

/// A ladder of shadows. Shadows are the one thing in the pipeline that draws
/// outside the widget's own box, so a golden catches both the falloff and the
/// quad expansion that has to make room for it.
#[test]
fn shadow_ladder() {
    let view = container()
        .background(Color::rgb(0.92, 0.92, 0.94))
        .padding(24.0)
        .layout(Flex::row().spacing(24.0))
        .children(
            common::LADDER[..5]
                .iter()
                .map(|&step| swatch(70.0, 70.0, Color::WHITE).corners(10.0).shadow(step)),
        );

    golden(
        "shadow_ladder",
        (494.0, 118.0),
        1.0,
        Color::rgb(0.92, 0.92, 0.94),
        view,
    );
}

/// The three degrees of freedom the elevation table could not reach: a shadow
/// thrown sideways, a coloured one, and one with spread.
///
/// `elevation` wrote `(0.0, offset_y)`, a spread of `0.0` and black at the
/// alpha its level was paired with, so no golden that existed before `shadow`
/// could show any of these — which is why this is a new reference rather than a
/// moved one.
#[test]
fn shadow_variations() {
    let sideways = Shadow::new((16.0, 0.0), 8.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.35));
    let coloured = Shadow::new((0.0, 6.0), 12.0, 0.0, Color::rgba(0.85, 0.1, 0.2, 0.55));
    let spread = Shadow::new((0.0, 0.0), 6.0, 8.0, Color::rgba(0.1, 0.2, 0.8, 0.45));

    let view = container()
        .background(Color::rgb(0.92, 0.92, 0.94))
        .padding(30.0)
        .layout(Flex::row().spacing(30.0))
        .children([sideways, coloured, spread].map(|shadow| {
            swatch(70.0, 70.0, Color::WHITE)
                .corners(10.0)
                .shadow(shadow)
        }));

    golden(
        "shadow_variations",
        (350.0, 130.0),
        1.0,
        Color::rgb(0.92, 0.92, 0.94),
        view,
    );
}

/// Rotation, scale and translation, including a transform inherited through a
/// parent — the case where the world transform is a chain and not a matrix
/// somebody wrote down.
#[test]
fn transforms_and_pivots() {
    let card = |c: Color| swatch(70.0, 40.0, c).corners(8.0);

    let view = container()
        .background(BACKDROP)
        .padding(20.0)
        .layout(Flex::column().spacing(18.0))
        .child(
            container()
                .layout(Flex::row().spacing(24.0))
                .child(card(Color::rgb(0.90, 0.30, 0.30)).rotate(20.0))
                .child(
                    card(Color::rgb(0.30, 0.80, 0.40))
                        .rotate(20.0)
                        .pivot(Pivot::TOP_LEFT),
                )
                .child(card(Color::rgb(0.35, 0.55, 0.95)).scale(0.7)),
        )
        .child(
            box_of(240.0, 90.0)
                .background(Color::rgb(0.18, 0.18, 0.24))
                .corners(10.0)
                .rotate(8.0)
                .layout(Flex::row().spacing(10.0))
                .padding(10.0)
                .child(card(Color::rgb(0.95, 0.75, 0.20)).rotate(15.0).scale(0.8))
                .child(card(Color::rgb(0.60, 0.35, 0.90)).translate((6.0, -8.0))),
        );

    golden("transforms_and_pivots", (330.0, 230.0), 1.0, BACKDROP, view);
}

/// A clip is only tested by content that would cover the corner if nothing
/// stopped it. The first two boxes hold a child far larger than themselves, so
/// what is left on screen *is* the clip: if the corner curvature does not
/// reach the clip, the silhouette comes out square and the golden says so. The
/// third is the ordinary case — content that overflows at an angle.
#[test]
fn rounded_clipping() {
    let clipped = |corners: Corners| {
        box_of(140.0, 100.0)
            .background(Color::rgb(0.15, 0.20, 0.30))
            .corners(corners)
            .overflow(Overflow::Hidden)
            .child(swatch(300.0, 300.0, Color::rgb(0.95, 0.45, 0.25)))
    };

    let view = container()
        .background(BACKDROP)
        .padding(16.0)
        .layout(Flex::row().spacing(16.0))
        .child(clipped(Corners::rounded(24.0)))
        .child(clipped(Corners::squircle(24.0)))
        .child(
            box_of(140.0, 100.0)
                .background(Color::rgb(0.15, 0.20, 0.30))
                .corners(24.0)
                .overflow(Overflow::Hidden)
                .child(swatch(200.0, 160.0, Color::rgb(0.30, 0.85, 0.65)).rotate(15.0)),
        );

    golden("rounded_clipping", (484.0, 132.0), 1.0, BACKDROP, view);
}

/// A clip that has been turned clips to the turned shape, not to the box
/// around it.
///
/// Each box holds a child far larger than itself, so what survives on screen
/// *is* the clip — and the whole error is visible as silhouette. An 80×80
/// square turned 45° has a 113×113 bounding box: a clip that degrades to that
/// box stops clipping a 100×100 child at all, and what shows is the child,
/// square-cornered and at its full size, instead of a rounded 80×80 diamond.
///
/// Three angles because the failure is not all-or-nothing. At 15° the box is
/// ~98 wide and trims the child slightly, with corners that are square where
/// the clip's are round; at 45° it does not trim at all; at 90° the box is
/// exact again, so that column must not move and says the fix did not reach
/// past rotation into the cases that already worked.
///
/// The fourth turns an *ancestor* instead, and the clip is declared by a child
/// that does not turn at all. A clip is placed by the world transform it was
/// flattened under, so this is the same computation — but "the same by
/// construction" is what a fix says about itself right up until the one
/// arrangement it forgot, and this one costs a column.
#[test]
fn rotated_clipping() {
    let clipped = |degrees: f32| {
        box_of(80.0, 80.0)
            .background(Color::rgb(0.15, 0.20, 0.30))
            .corners(16.0)
            .overflow(Overflow::Hidden)
            .rotate(degrees)
            .child(swatch(100.0, 100.0, Color::rgb(0.95, 0.45, 0.25)))
    };

    let view = container()
        .background(BACKDROP)
        .padding(30.0)
        .layout(Flex::row().spacing(30.0))
        .child(clipped(15.0))
        .child(clipped(45.0))
        .child(clipped(90.0))
        .child(
            box_of(80.0, 80.0)
                .rotate(35.0)
                .child(clipped(0.0).background(Color::rgb(0.30, 0.20, 0.15))),
        );

    golden("rotated_clipping", (510.0, 140.0), 1.0, BACKDROP, view);
}

/// A checkerboard, so the clip's edge is unmistakable: every square that
/// survives is one the clip let through.
///
/// Generated rather than loaded — `ImageSource::Rgba` takes pixels directly, so
/// this depends on no file and decodes nothing.
fn checkerboard(size: u32, square: u32) -> ImageSource {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dark = ((x / square) + (y / square)).is_multiple_of(2);
            pixels.extend_from_slice(if dark {
                &[0x1f, 0x2b, 0x3a, 0xff]
            } else {
                &[0xf2, 0x73, 0x40, 0xff]
            });
        }
    }
    ImageSource::Rgba {
        width: size,
        height: size,
        pixels: pixels.into(),
    }
}

/// An image is clipped by the same shape a rectangle is, and follows it through
/// a rotation.
///
/// Images take a pipeline of their own — a textured quad with the clip carried
/// on its four vertices, not the shape shader's per-instance SDF — and until
/// this scenario existed no golden drew an image at all, clipped or otherwise.
/// So the two halves of clipping could disagree, and the only thing that would
/// have noticed is somebody looking at a screen.
///
/// Left: upright, where the clip and its bounding box are the same thing.
/// Middle: the same 90×90 image in the same 70×70 rounded clip, turned 30° —
/// the checks turn with it and the silhouette stays a rounded square.
///
/// Right: a *squircle* clip, which this pipeline could not cut until now. Its
/// SDF took a rect and four radii and no curvature, so a squircle clip cut an
/// image with circular corners while the container beside it cut with
/// squircular ones — one clip, two shapes, depending on which pipeline was
/// asked. It is a few pixels at four corners, which is the size of thing a
/// golden exists to hold.
#[test]
fn clipped_images() {
    let clipped = |corners: Corners, degrees: f32| {
        box_of(70.0, 70.0)
            .corners(corners)
            .overflow(Overflow::Hidden)
            .rotate(degrees)
            .child(
                box_of(90.0, 90.0)
                    .child(image(checkerboard(90, 15)).content_fit(ContentFit::Cover)),
            )
    };

    let view = container()
        .background(BACKDROP)
        .padding(30.0)
        .layout(Flex::row().spacing(30.0))
        .child(clipped(Corners::rounded(14.0), 0.0))
        .child(clipped(Corners::rounded(14.0), 30.0))
        .child(clipped(Corners::squircle(24.0), 0.0));

    golden("clipped_images", (330.0, 130.0), 1.0, BACKDROP, view);
}

/// Two clips a quarter turn apart still cut exactly.
///
/// `intersect_clips` rewrites the inner clip in the outer's coordinates when
/// the map between them sends a rect to a rect. A quarter turn does — the axes
/// swap rather than lean — but it also sends the top-left corner to the
/// top-right, and `CornerRadii` is four numbers in a fixed order. So the pair
/// used to fall back to the axis-aligned box around each, which for a turned
/// outer clip is 157% larger than the shape and lets the content out on every
/// side (#397).
///
/// Left: the outer clip alone, turned, for the silhouette the others should
/// keep. Middle: a child clip a quarter turn further round. Right: a child
/// mirrored instead, which `Container::scale` reaches with a negative factor
/// and which moves the corners the same way.
///
/// Both children are rounded at one corner only, and at the one corner a
/// quarter turn and a mirror disagree about — so the three cells have to differ
/// from each other as well as from the fallback, which squares all four.
#[test]
fn clips_a_quarter_turn_apart() {
    let content = || {
        container()
            .width(fill())
            .height(fill())
            .background(Color::rgb(0.95, 0.75, 0.25))
    };

    let outer = |child: AnyWidget| {
        box_of(96.0, 96.0)
            .corners(Corners::rounded(22.0))
            .overflow(Overflow::Hidden)
            .rotate(25.0)
            .background(Color::rgb(0.20, 0.22, 0.30))
            .layout(ZStack::new())
            .child(child)
    };

    // The top-*right* corner, and one corner only. A whole rounded edge maps to
    // itself under a mirror and would prove nothing about where corners go; the
    // top-left is no better, since a quarter turn and a mirror-x both send it
    // to the top-right. They part company at this one: the turn sends it to the
    // bottom-right and the mirror to the top-left.
    let inner = |degrees: f32, scale: Scale| {
        box_of(96.0, 96.0)
            .corners(Corners::rounded(CornerRadii {
                top_left: 0.0,
                top_right: 40.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            }))
            .overflow(Overflow::Hidden)
            .rotate(degrees)
            .scale(scale)
            .layout(ZStack::new())
            .child(content())
            .into_any()
    };

    let view = container()
        .background(BACKDROP)
        .padding(26.0)
        .layout(Flex::row().spacing(26.0))
        .child(outer(content().into_any()))
        .child(outer(inner(90.0, Scale::NONE)))
        .child(outer(inner(0.0, Scale::new(-1.0, 1.0))));

    golden(
        "clips_a_quarter_turn_apart",
        (370.0, 148.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// A text is clipped by the same shape a rectangle is, and follows it through
/// a rotation.
///
/// The mirror of `clipped_images`, and the half of clipping that was still a
/// box. Every other pipeline tests a fragment in the clip's own space; text
/// took `PlacedShape::world_aabb` and was cut by the upright box around the
/// shape, so a turned scroller let its text out at the corners — over an 80%
/// larger area at 20 degrees (#405).
///
/// Left: upright, where the clip and its box are the same thing — the control,
/// and the one cell glyphon draws, since only a transformed text takes the
/// quad. Middle: the same text in the same clip, turned 30 degrees, so the cut
/// has to follow the turned edge rather than the box. Right: turned the other
/// way *and* a squircle, because the quad clip carries a curvature and a corner
/// is where a box and a shape differ most — an upright squircle would have gone
/// to glyphon and tested none of it.
///
/// Each line is long enough to run out of its clip on both sides, so the cut is
/// what the picture is of.
#[test]
fn clipped_text() {
    let clipped = |corners: Corners, degrees: f32| {
        box_of(96.0, 96.0)
            .corners(corners)
            .background(Color::rgb(0.18, 0.20, 0.28))
            .overflow(Overflow::Hidden)
            .rotate(degrees)
            .layout(Flex::column().main_alignment(MainAlignment::Center))
            // Each line is about twice the width of the clip, so every one of
            // them runs out on both sides. A text that fits its clip proves
            // nothing about where the clip's edge is.
            .child(nowrap_line("MMMMMMMM"))
            .child(nowrap_line("WWWWWWWW"))
            .child(nowrap_line("MMMMMMMM"))
    };

    fn nowrap_line(content: &'static str) -> Container {
        container()
            .width(220.0)
            .child(label(content, 26.0).nowrap())
    }

    let view = container()
        .background(BACKDROP)
        .padding(30.0)
        .layout(Flex::row().spacing(30.0))
        .child(clipped(Corners::rounded(16.0), 0.0))
        .child(clipped(Corners::rounded(16.0), 30.0))
        .child(clipped(Corners::squircle(28.0), -20.0));

    golden("clipped_text", (408.0, 156.0), 1.0, BACKDROP, view);
}

/// An upright text in an upright rounded card, which glyphon cannot cut.
///
/// The everyday half of #405, and the one that has nothing to do with
/// rotation. `TextBounds` is four integers, so the only clip glyphon can
/// honour is an upright rectangle — and a rounded card *is* upright, so
/// nothing about the text or the container looked like the transformed case
/// that was fixed first. The box around the card squares off its four
/// corners, and any glyph reaching into one showed outside the card it is in.
///
/// A radius of 34 on a 130-wide card, and lines long enough to run out of it
/// on both sides at the height where the corner is deepest. Left: the corners
/// at the top, where two lines reach them. Right: the same card turned 12
/// degrees, so the box is wrong along every edge rather than only at the
/// corners — the two failures are different sizes and this shows both.
#[test]
fn text_is_cut_by_the_corners_of_its_card() {
    let card = |degrees: f32| {
        box_of(130.0, 96.0)
            .corners(Corners::rounded(34.0))
            .background(Color::rgb(0.18, 0.20, 0.28))
            .overflow(Overflow::Hidden)
            .rotate(degrees)
            .layout(Flex::column().main_alignment(MainAlignment::Center))
            .child(wide_line("MMMMMM"))
            .child(wide_line("WWWWWW"))
            .child(wide_line("MMMMMM"))
    };

    fn wide_line(content: &'static str) -> Container {
        container()
            .width(260.0)
            .child(label(content, 28.0).nowrap())
    }

    let view = container()
        .background(BACKDROP)
        .padding(28.0)
        .layout(Flex::row().spacing(28.0))
        .child(card(0.0))
        .child(card(12.0));

    golden(
        "text_is_cut_by_the_corners_of_its_card",
        (372.0, 152.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// Two cards over stripes, one upright and one turned, each filtering the
/// surface's own content through its own shape.
///
/// The stripes are drawn by the scene because `SURFACE` blurs what is already
/// on the target, and over an empty background there is nothing to see. Each
/// card's border marks the shape; the blurred area is the mask. Where the two
/// come apart, they have come apart.
fn frosted_cards() -> Container {
    let stripes = stripes(360.0, 14, 14.0);

    let case = |degrees: f32| {
        container()
            .width(180.0)
            .height(180.0)
            .layout(
                Flex::row()
                    .main_alignment(MainAlignment::Center)
                    .cross_alignment(CrossAlignment::Center),
            )
            .child(
                box_of(110.0, 70.0)
                    .corners(16.0)
                    .background(Color::rgba(0.10, 0.10, 0.16, 0.35))
                    .border(2.0, Color::rgba(1.0, 1.0, 1.0, 0.55))
                    .rotate(degrees)
                    .backdrop_blur(BackdropBlur::new(12.0).sources(BackdropSources::SURFACE)),
            )
            .into_any()
    };

    container()
        .width(fill())
        .height(fill())
        .layout(ZStack::new())
        .children([
            stripes.into_any(),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .children([case(0.0), case(25.0)])
                .into_any(),
        ])
}

/// A backdrop blur is filtered to the shape the container drew, and follows it
/// through a rotation.
///
/// **No golden drew a backdrop blur at all before this.** The whole pass — the
/// offscreen target, the downsample, the two gaussian passes, the masked
/// composite — was watched by nobody, which is how `BackdropRegion` came to
/// carry a rect, four radii and no transform without anything objecting.
#[test]
fn backdrop_blur_follows_its_shape() {
    golden(
        "backdrop_blur_follows_its_shape",
        (360.0, 180.0),
        1.0,
        BACKDROP,
        frosted_cards(),
    );
}

/// The same two cards at scale 2.
///
/// The mask's shape and the map that reaches it have to agree about their
/// units, and at scale 1 every wrong pairing agrees by accident. Here they do
/// not: a shape given in physical pixels against a map landing in logical ones
/// is out by a factor of two, the SDF reads inside everywhere it is asked, and
/// the frost fills the whole viewport — which is the bounding box, which is the
/// thing all of this exists to stop, back again on every HiDPI screen.
///
/// One scene for both, so "the same two cards" stays true when either is
/// edited.
#[test]
fn backdrop_blur_follows_its_shape_at_scale_2x() {
    golden(
        "backdrop_blur_follows_its_shape_at_scale_2x",
        (360.0, 180.0),
        2.0,
        BACKDROP,
        frosted_cards(),
    );
}

/// A backdrop blur is cut by its scroller's shape, not by the box around it.
///
/// The backdrop pass was the last consumer to test a clip as four numbers.
/// Every other pipeline carries a clip as a `PlacedShape` and tests the
/// fragment in the clip's own space; this one narrowed its viewport to
/// `world_aabb` and stopped there, so a frosted card in the corner of a rounded
/// scroller blurred the corner the scroller had not drawn, and a turned
/// scroller leaked by the same 73% that motivated #198 (#401).
///
/// Each cell is a card larger than its scroller, so the blur runs at the clip
/// on every side and the cut is the picture. Left: a rounded scroller, where
/// the corners are what a box gets wrong. Right: the same, turned 20°, where
/// the whole outline is.
#[test]
fn backdrop_blur_is_cut_by_its_scroller() {
    golden(
        "backdrop_blur_is_cut_by_its_scroller",
        (360.0, 180.0),
        1.0,
        BACKDROP,
        scrollers(),
    );
}

/// One scene for both scales, so "the same two scrollers" stays true when
/// either is edited.
fn scrollers() -> Container {
    // Shifted up and to the left so it juts out of the scroller on those sides
    // as well as the others. A card the layout has already fitted inside its
    // clip leaves the clip nothing to narrow, and the narrowing is what a
    // viewport is.
    let card = || {
        box_of(150.0, 150.0)
            .background(Color::rgba(0.10, 0.10, 0.16, 0.30))
            .translate((-34.0, -30.0))
            .backdrop_blur(BackdropBlur::new(14.0).sources(BackdropSources::SURFACE))
    };

    let scroller = |degrees: f32| {
        container()
            .width(180.0)
            .height(180.0)
            .layout(
                Flex::row()
                    .main_alignment(MainAlignment::Center)
                    .cross_alignment(CrossAlignment::Center),
            )
            .child(
                box_of(104.0, 104.0)
                    .corners(Corners::rounded(34.0))
                    .overflow(Overflow::Hidden)
                    .rotate(degrees)
                    .border(2.0, Color::rgba(1.0, 1.0, 1.0, 0.75))
                    .layout(ZStack::new())
                    .child(card()),
            )
            .into_any()
    };

    container()
        .width(fill())
        .height(fill())
        .layout(ZStack::new())
        .children([
            stripes(360.0, 13, 14.0).into_any(),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .children([scroller(0.0), scroller(20.0)])
                .into_any(),
        ])
}

/// The same two scrollers at scale 2.
///
/// The clip arrives logical, like the shape beside it, and the pass carries the
/// surface scale itself — so the viewport is narrowed by `clip * scale` and the
/// fragment is carried back by a map with the scale already folded in. Two
/// conventions, and at scale 1 every wrong pairing of them agrees.
///
/// Neither scroller sits at the origin, which is the other half: a clip whose
/// corner is at zero scales the same whether the factor is applied or divided.
#[test]
fn backdrop_blur_is_cut_by_its_scroller_at_scale_2x() {
    golden(
        "backdrop_blur_is_cut_by_its_scroller_at_scale_2x",
        (360.0, 180.0),
        2.0,
        BACKDROP,
        scrollers(),
    );
}

/// A frosted text is cut by its scroller's shape too, and so is its contour.
///
/// The clip reaches three fragment shaders, not one: `fs_composite` cuts a
/// container's blur, `fs_composite_mask` cuts a text's, and `fs_outline` cuts
/// the contour dilated around it. `backdrop_blur_is_cut_by_its_scroller` covers
/// the first. This covers the other two, and it has to exist rather than being
/// a third cell there, because the only frosted text in a clip anywhere else is
/// in an **upright square** one — the single placement where a clip's shape and
/// the box around it are the same picture, so nothing there could tell them
/// apart.
///
/// Left: a turned scroller, where the box leaks four corner triangles. Right: a
/// rounded one, where it leaks four corners. Both texts carry a stroke, so the
/// contour runs at the clip as well — a clip that stopped a frost but not the
/// stroke on it would be two halves of one effect disagreeing.
#[test]
fn frosted_text_is_cut_by_its_scroller() {
    let frosted = || {
        label("MMMM", 22.0)
            .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
            .backdrop_blur(12.0)
            .text_stroke(TextStroke::new(2.0, Color::BLACK))
            .nowrap()
    };

    let scroller = |corners: Corners, degrees: f32| {
        container()
            .width(180.0)
            .height(180.0)
            .layout(
                Flex::row()
                    .main_alignment(MainAlignment::Center)
                    .cross_alignment(CrossAlignment::Center),
            )
            .child(
                box_of(84.0, 84.0)
                    .corners(corners)
                    .overflow(Overflow::Hidden)
                    .rotate(degrees)
                    .layout(
                        Flex::row()
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        container()
                            .width(150.0)
                            .layout(Flex::column().spacing(2.0))
                            .child(frosted())
                            .child(frosted())
                            .child(frosted()),
                    ),
            )
            .into_any()
    };

    let view = container()
        .width(fill())
        .height(fill())
        .layout(ZStack::new())
        .children([
            stripes(360.0, 13, 14.0).into_any(),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .children([
                    scroller(Corners::rounded(8.0), 25.0),
                    scroller(Corners::rounded(40.0), 0.0),
                ])
                .into_any(),
        ]);

    golden(
        "frosted_text_is_cut_by_its_scroller",
        (360.0, 180.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// A backdrop blur cut by the three corner curvatures that are not a circle.
///
/// `backdrop_blur_follows_its_shape` uses `corners(16.0)` — `Corners::rounded`,
/// k = 1. The backdrop composite read the curvature as `max(k, 0.05) * 2` where
/// the shape shader and the quad shader read `pow(2, k)`, and those two
/// expressions agree at exactly k = 1 and k = 2 — `rounded` and `squircle`, the
/// two a scenario reaches for. So #403 sat under every picture there was.
///
/// These are the three that tell them apart: a **bevel** (k = 0), where the
/// wrong reading gives n = 0.1 and blurs a deep concave notch inside an
/// octagonal border; a **scoop** (k = −1), which the backdrop had no concave
/// branch for at all; and a k of 0.5 between the two agreeing points, because
/// `Corners` interpolates and an animation sweeps k continuously rather than
/// landing on the constants.
///
/// Each card's border marks the shape, drawn by the shape shader; the blurred
/// area is the backdrop's own idea of it. The two read one `curvature` and have
/// to make one corner out of it.
#[test]
fn backdrop_blur_follows_every_curvature() {
    let case = |corners: Corners| {
        container()
            .width(150.0)
            .height(150.0)
            .layout(
                Flex::row()
                    .main_alignment(MainAlignment::Center)
                    .cross_alignment(CrossAlignment::Center),
            )
            .child(
                box_of(110.0, 110.0)
                    .corners(corners)
                    .background(Color::rgba(0.10, 0.10, 0.16, 0.35))
                    .border(2.0, Color::rgba(1.0, 1.0, 1.0, 0.75))
                    .backdrop_blur(BackdropBlur::new(12.0).sources(BackdropSources::SURFACE)),
            )
            .into_any()
    };

    let view = container()
        .width(fill())
        .height(fill())
        .layout(ZStack::new())
        .children([
            stripes(450.0, 11, 14.0).into_any(),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .children([
                    case(Corners::bevel(34.0)),
                    case(Corners::superellipse(34.0, 0.5)),
                    case(Corners::scoop(34.0)),
                ])
                .into_any(),
        ]);

    golden(
        "backdrop_blur_follows_every_curvature",
        (450.0, 150.0),
        1.0,
        BACKDROP,
        view,
    );
}

/// A frosted text over the same stripes, at rest and under three transforms.
///
/// The glyphs are the window: what they cover shows the backdrop blurred, and
/// their own colour is the tint over it. The coverage mask is rasterized in the
/// text's own space, so whether it follows the letters through a transform is
/// exactly what these four cells put side by side.
///
/// The turned one carries a stroke as well. Over frost a stroke is a true
/// contour, dilated from the same mask — so it is the one part of the effect
/// measured in mask texels rather than read from them, and it would sit at the
/// wrong width or in the wrong place if that space were got wrong.
///
/// The sixth wraps, which is the only way to catch the mask and the glyphs
/// being shaped in different buffers — see the comment on the cell.
///
/// The fifth is a text too wide for the box it is in, so the frost's viewport
/// is cut on every side. That case used to be carried by a `mask_rect` uniform
/// re-mapping the clipped viewport onto the mask's uv, and the uniform is gone
/// — a fragment's place in the text's own space already accounts for the cut.
/// Nothing watched it before, which is how four deleted floats could have been
/// four deleted floats and a defect.
fn frosted_labels() -> Container {
    let stripes = stripes(576.0, 12, 15.0);

    let cell = |label: Text, place: fn(Container) -> Container| {
        place(
            container()
                .width(96.0)
                .height(180.0)
                .layout(
                    Flex::row()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .child(label.nowrap()),
        )
        .into_any()
    };

    // The one cell that wraps, and the only kind that can tell two shapings
    // apart: a single line breaks nowhere, so it agrees with anything. The mask
    // is shaped separately from the glyphs over it, and a transformed text is
    // drawn by a different shaper than an untransformed one — with the mask
    // following the wrong one, the frost here wrapped a glyph later than the
    // letters did and left a frosted `g` floating where no letter was.
    let wrapping = |label: Text, place: fn(Container) -> Container| {
        place(
            container()
                .width(96.0)
                .height(180.0)
                .layout(
                    Flex::row()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .child(container().width(72.0).child(label.wrap(true))),
        )
        .into_any()
    };

    let frosted = || {
        label("Ag", 34.0)
            .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
            .backdrop_blur(14.0)
    };

    // The frost's viewport runs past this box on every side, and the clip is
    // what stops it. Nothing else here cuts a viewport.
    let scrolled = container()
        .width(96.0)
        .height(180.0)
        .layout(
            Flex::row()
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(
            container()
                .width(58.0)
                .height(30.0)
                .overflow(Overflow::Hidden)
                .layout(
                    Flex::row()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .child(frosted().nowrap()),
        );

    container()
        .width(fill())
        .height(fill())
        .layout(ZStack::new())
        .children([
            stripes.into_any(),
            container()
                .width(fill())
                .height(fill())
                .layout(Flex::row())
                .children([
                    cell(frosted(), |c| c),
                    cell(frosted(), |c| c.scale(1.6)),
                    cell(frosted(), |c| c.scale((0.7, 1.8))),
                    cell(
                        frosted().text_stroke(TextStroke::new(2.0, Color::BLACK)),
                        |c| c.rotate(30.0),
                    ),
                    scrolled.into_any(),
                    wrapping(
                        label("Ag gj", 30.0)
                            .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                            .backdrop_blur(14.0),
                        |c| c.scale(1.3),
                    ),
                ])
                .into_any(),
        ])
}

/// A frosted text keeps its frost under a scale and a rotation.
///
/// Before this the renderer refused the job outright for anything but a
/// translation, so three of these four cells showed sharp letters over a sharp
/// backdrop — the effect absent rather than wrong, and absent in exactly the
/// transforms a hover or an `animate_transform` reaches for (#199).
#[test]
fn frosted_text_follows_its_letters() {
    golden(
        "frosted_text_follows_its_letters",
        (576.0, 180.0),
        1.0,
        BACKDROP,
        frosted_labels(),
    );
}

/// The same four at scale 2.
///
/// The mask's frame is logical and the map that reaches it folds the surface
/// scale in, the way a container's shape does; the glyph origin inside the mask
/// is in texels, which is a third unit again. At scale 1 two of the three agree
/// by accident.
#[test]
fn frosted_text_follows_its_letters_at_scale_2x() {
    golden(
        "frosted_text_follows_its_letters_at_scale_2x",
        (576.0, 180.0),
        2.0,
        BACKDROP,
        frosted_labels(),
    );
}

/// A text that wraps, at scale 2, over a box narrow enough to force the break.
///
/// Nothing in this file wrapped before — every label is one line, and one line
/// agrees with any shaping buffer, which is how the buffer rule came to be
/// unwatched. It is the rule `TextRenderState` shapes every ordinary text with,
/// its floors are in logical pixels and its scale is applied after them, and a
/// mutation of either arithmetic passed the whole suite.
///
/// At scale 2 rather than 1 because that is where the scale in it is legible:
/// at scale 1 the multiply and the floor are indistinguishable from half a
/// dozen wrong rules.
///
/// Both boxes are wider than 200 logical pixels because that is the floor, and
/// under it a text does not wrap at its own box at all — it wraps at 200. That
/// is the rule's own doing and worth seeing here rather than discovering it.
#[test]
fn text_wraps_where_the_box_ends_at_scale_2x() {
    let view = container()
        .background(BACKDROP)
        .padding(12.0)
        .layout(Flex::column().spacing(10.0))
        .child(
            container()
                .width(260.0)
                .child(label("wrap this line where the box ends", 20.0).wrap(true)),
        )
        .child(
            container()
                .width(210.0)
                .child(label("and this one sooner, being narrower", 16.0).wrap(true)),
        );

    golden(
        "text_wraps_where_the_box_ends_at_scale_2x",
        (290.0, 220.0),
        2.0,
        BACKDROP,
        view,
    );
}

/// The same composition at scale 2. Every radius, border width and shadow
/// extent is scaled in the shader rather than in layout, so a HiDPI bug is
/// invisible to every test that does not render at a scale factor.
#[test]
fn hidpi_at_scale_2x() {
    let view = container()
        .background(BACKDROP)
        .padding(12.0)
        .layout(Flex::row().spacing(12.0))
        .child(
            swatch(80.0, 60.0, Color::rgb(0.30, 0.55, 0.95))
                .corners(16.0)
                .border(2.0, Color::WHITE),
        )
        .child(
            swatch(80.0, 60.0, Color::WHITE)
                .corners(Corners::squircle(16.0))
                .shadow(common::LADDER[3]),
        );

    golden("hidpi_at_scale_2x", (200.0, 84.0), 2.0, BACKDROP, view);
}

/// A sibling of the scenario above at scale 1.5, where the scale is not an
/// integer.
///
/// Not the same composition, and deliberately so: every number in it is odd
/// and there is a label the other has not got. Radii, border widths and shadow
/// extents are scaled in the shader and glyphs are rasterised at `font_size *
/// scale`; at 2 every one of those lands on a whole physical pixel, so a
/// half-pixel error there is invisible. Nothing lands on one here. The surface
/// is 201x85 logical — 301.5 x 127.5 physical, rounded halfway away from zero
/// — and the padding, spacing and sizes are odd, so each edge falls between
/// two pixels rather than on one.
///
/// This is the scale the compositor used to round up to 2 and downscale from,
/// which no golden could see: the pixels were right for a scale of 2 and the
/// screen was not showing them at 2.
#[test]
fn hidpi_at_scale_1_5x() {
    let view = container()
        .background(BACKDROP)
        .padding(11.0)
        .layout(Flex::row().spacing(13.0))
        .child(
            swatch(81.0, 63.0, Color::rgb(0.30, 0.55, 0.95))
                .corners(15.0)
                .border(3.0, Color::WHITE),
        )
        .child(
            container()
                .layout(Flex::column().spacing(7.0))
                .child(
                    swatch(83.0, 35.0, Color::WHITE)
                        .corners(Corners::squircle(15.0))
                        .shadow(common::LADDER[3]),
                )
                .child(label("1.5", 21.0)),
        );

    golden("hidpi_at_scale_1_5x", (201.0, 85.0), 1.5, BACKDROP, view);
}

/// Text as glyphon draws it: axis-aligned, several sizes on one surface. This
/// is the path every label takes until something transforms it.
#[test]
fn text_at_rest() {
    let view = container()
        .background(BACKDROP)
        .padding(16.0)
        .layout(Flex::column().spacing(10.0))
        .child(label("Guido", 28.0))
        .child(label("layer shell", 18.0))
        .child(label("0123456789", 13.0))
        .child(
            box_of(200.0, 44.0)
                .background(Color::rgb(0.20, 0.30, 0.45))
                .corners(8.0)
                .padding(10.0)
                .child(label("on a card", 16.0)),
        );

    golden("text_at_rest", (240.0, 190.0), 1.0, BACKDROP, view);
}

/// Text under a transform, which is a pipeline of its own: rasterised to a
/// texture and drawn as a quad rather than handed to glyphon. Until this
/// scenario existed, no golden had put a glyph through it.
///
/// The angles stop short of 90° and 270°, which `text_at_right_angles` covers
/// on their own. At exactly those two the
/// glyphs vanish — #218, measured across eighty angles and reproduced nowhere
/// else, not at 89.9° and not at 90.1°.
#[test]
fn text_transformed() {
    let card = |degrees: f32| {
        box_of(150.0, 46.0)
            .background(Color::rgb(0.18, 0.20, 0.28))
            .corners(6.0)
            .padding(8.0)
            .rotate(degrees)
            .child(label("rotated", 16.0))
    };

    let view = container()
        .background(BACKDROP)
        .padding(30.0)
        .layout(Flex::column().spacing(30.0))
        .child(
            container()
                .layout(Flex::row().spacing(40.0))
                .child(card(0.0))
                .child(card(30.0)),
        )
        .child(
            container()
                .layout(Flex::row().spacing(40.0))
                .child(card(135.0))
                .child(card(180.0)),
        )
        .child(
            box_of(150.0, 46.0)
                .background(Color::rgb(0.28, 0.18, 0.24))
                .corners(6.0)
                .padding(8.0)
                .scale(1.4)
                .child(label("scaled", 16.0)),
        );

    golden("text_transformed", (440.0, 320.0), 1.0, BACKDROP, view);
}

/// The quarter turns, which drew nothing at all until #218 was fixed.
///
/// `a` and `d` are both `cos θ` for a rotation, so the shortcut that skips text
/// scaled to nothing read a quarter turn as a collapse and threw the glyphs
/// away — at exactly 90° and 270° and nowhere else, not at 89.9° and not at
/// 90.1°, which is how it survived every angle anybody happened to try.
#[test]
fn text_at_right_angles() {
    let card = |degrees: f32| {
        box_of(150.0, 46.0)
            .background(Color::rgb(0.18, 0.20, 0.28))
            .corners(6.0)
            .padding(8.0)
            .rotate(degrees)
            .child(label("quarter", 16.0))
    };

    // A card 150 long stands 150 tall once it is turned, so the surface is
    // sized for the turn and the cards are centred in it. A golden that crops
    // its own subject watches less than it appears to.
    let view = container()
        .background(BACKDROP)
        .width(360.0)
        .height(220.0)
        .layout(
            Flex::row()
                .spacing(40.0)
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(card(90.0))
        .child(card(270.0));

    golden("text_at_right_angles", (360.0, 220.0), 1.0, BACKDROP, view);
}
