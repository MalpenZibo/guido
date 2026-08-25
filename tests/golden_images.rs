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
//! The rasterizer's own version is part of it too: a Mesa bump in the runner
//! image can move an antialiased edge by a unit or two, which `TOLERANCE`
//! absorbs. If it ever moves further than that, the runner image is the thing
//! to pin — not the tolerance to widen.
//!
//! Without any Vulkan adapter the tests skip rather than fail, so a
//! contributor without a GPU is not blocked. CI sets `GUIDO_GOLDEN_REQUIRED=1`,
//! which turns that skip back into a failure — a skipped test that reports
//! green is worth less than no test at all.
//!
//! **Re-blessing is not a way to make a test pass.** A changed golden is a
//! changed pixel on someone's screen, and the diff is the review:
//!
//! ```sh
//! UPDATE_GOLDEN=1 cargo test --test golden_images
//! ```
//!
//! CI refuses a pull request that edits `tests/golden/` unless it carries the
//! `golden-update` label, so the re-blessing is always somebody's decision.
//! On failure the three images (expected, actual, and a map of what moved)
//! are written to `target/golden-failures/` and uploaded as a CI artifact.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{GpuContext, PaintContext, RenderNode, Renderer, flatten_root_into};
use guido::tree::Tree;
use guido::widgets::Widget;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Non-sRGB 8-bit RGBA: the same family of format the Wayland surface picks,
/// so what is read back here is what the compositor would have been handed.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Per-channel difference below which two pixels are the same pixel. Blessed
/// and compared on one rasterizer, the expected difference is zero; this
/// absorbs the last bit of floating-point noise without hiding anything a
/// person could see.
const TOLERANCE: u8 = 2;

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
    let width = (logical_width * scale).round() as u32;
    let height = (logical_height * scale).round() as u32;

    // Layout and paint: the same two passes `render_snapshots` runs, and
    // neither needs a compositor or a GPU.
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(
            t,
            id,
            Constraints::new(0.0, 0.0, logical_width, logical_height),
        )
    });

    let mut node = RenderNode::new(root.as_u64());
    tree.with_widget_mut(root, |w, id, t| {
        let mut ctx = PaintContext::new(&mut node);
        w.paint(t, id, &mut ctx);
    });

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

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        assert!(
            is_software_rasterizer(adapter)
                || std::env::var_os("GUIDO_GOLDEN_ANY_ADAPTER").is_some(),
            "refusing to bless `{name}` on `{adapter}`.\n\
             Goldens are compared against lavapipe in CI, and no two rasterizers \
             antialias an edge the same way, so a golden blessed here would fail \
             there for reasons that have nothing to do with the change.\n\
             Install lavapipe and re-run with:\n  \
             VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
             UPDATE_GOLDEN=1 cargo test --test golden_images"
        );
        write_png(&path, &actual);
        eprintln!("blessed {name} ({adapter})");
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
             UPDATE_GOLDEN=1 and put the diff in the pull request — that diff \
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
    let Some(ctx) = ctx() else {
        assert!(
            std::env::var_os("GUIDO_GOLDEN_REQUIRED").is_none(),
            "`{name}` needs a Vulkan adapter and found none, and \
             GUIDO_GOLDEN_REQUIRED is set. In CI this means lavapipe is not \
             installed or VK_ICD_FILENAMES does not point at it."
        );
        eprintln!("skipping golden `{name}`: no Vulkan adapter on this machine");
        return;
    };

    // A renderer per test, and dropped before the test returns. It caches
    // with `Rc` — the library it belongs to draws from one thread — so it
    // cannot be shared between test threads, and keeping one per thread in a
    // `thread_local` drops it during TLS teardown, where the state its own
    // drop reaches has already been torn down. The device it draws with is
    // still the shared one, which is the part that costs.
    let adapter = adapter_name(ctx);
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

/// The elevation ladder. Shadows are the one thing in the pipeline that draws
/// outside the widget's own box, so a golden catches both the falloff and the
/// quad expansion that has to make room for it.
#[test]
fn elevation_ladder() {
    let view = container()
        .background(Color::rgb(0.92, 0.92, 0.94))
        .padding(24.0)
        .layout(Flex::row().spacing(24.0))
        .children((0..5).map(|level| {
            swatch(70.0, 70.0, Color::WHITE)
                .corners(10.0)
                .elevation(level as f32)
        }));

    golden(
        "elevation_ladder",
        (494.0, 118.0),
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
                .elevation(3.0),
        );

    golden("hidpi_at_scale_2x", (200.0, 84.0), 2.0, BACKDROP, view);
}
