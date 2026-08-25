//! Text has to survive every angle, and at two of them it does not.
//!
//! This is #218. A label inside a container rotated to exactly 90° disappears,
//! and comes back on the way round. The issue's own probe stops at the render
//! tree, where the draw command is present at every angle — so what loses it is
//! downstream, in flatten or in the path that draws *transformed* text, which
//! is not glyphon but a texture drawn as a quad.
//!
//! A golden cannot say this. A golden holds one frame against one reference,
//! and the question here is about eighty of them: which angles keep their
//! glyphs and which do not. So this counts instead — the label is white and
//! nothing else in the scene is, so the glyphs are countable.
//!
//! It needs the same rasterizer the goldens need, for the same reason, and
//! skips itself without one:
//!
//! ```sh
//! export VK_ICD_FILENAMES=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -1)
//! cargo test --test text_at_angles
//! ```

use guido::layout::Constraints;
use guido::prelude::*;
use guido::renderer::{GpuContext, PaintContext, RenderNode, Renderer, flatten_root_into};
use guido::tree::Tree;
use guido::widgets::Widget;

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");
const FONT_FAMILY: &str = "DejaVu Sans Mono";
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const WIDTH: u32 = 260;
const HEIGHT: u32 = 190;

/// The scene, with the card's transform supplied whole: a turn, a scale, or
/// both.
fn scene_with(build: impl Fn(Container) -> Container + 'static) -> impl Widget + 'static {
    container()
        .background(Color::rgb(0.08, 0.08, 0.10))
        .padding(40.0)
        .child(build(
            container()
                .width(160.0)
                .height(90.0)
                .background(Color::rgb(0.20, 0.22, 0.30))
                .corners(8.0)
                .layout(Flex::row())
                .child(
                    text("HELLO")
                        .font_family(FontFamily::Name(FONT_FAMILY.into()))
                        .font_size(22.0)
                        .color(Color::WHITE),
                ),
        ))
}

/// Render one scene and count the glyph pixels: the label is the only white
/// thing on the surface.
fn glyph_pixels_of(
    ctx: &GpuContext,
    renderer: &mut Renderer,
    widget: impl Widget + 'static,
) -> usize {
    let mut tree = Tree::new();
    let root = tree.register(Box::new(widget));
    tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
    tree.with_widget_mut(root, |w, id, t| {
        w.layout(
            t,
            id,
            Constraints::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
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

    let extent = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("angle probe"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    renderer.set_screen_size(WIDTH as f32, HEIGHT as f32);
    renderer.set_scale_factor(1.0);
    renderer.render_to_view(
        &view,
        WIDTH,
        HEIGHT,
        &commands,
        &layers,
        Color::rgb(0.08, 0.08, 0.10),
    );

    let unpadded = WIDTH * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("angle probe readback"),
        size: u64::from(padded) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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
                rows_per_image: Some(HEIGHT),
            },
        },
        extent,
    );
    ctx.queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU never finished the readback");
    rx.recv().expect("map callback").expect("map");

    let mapped = slice.get_mapped_range();
    let mut white = 0usize;
    for row in 0..HEIGHT {
        let start = (row * padded) as usize;
        for px in mapped[start..start + unpadded as usize].as_chunks::<4>().0 {
            if px[0] > 200 && px[1] > 200 && px[2] > 200 {
                white += 1;
            }
        }
    }
    drop(mapped);
    buffer.unmap();

    white
}

fn is_software_rasterizer(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("llvmpipe") || name.contains("lavapipe") || name.contains("swiftshader")
}

#[test]
fn no_angle_loses_its_glyphs() {
    let _ = env_logger::builder().is_test(true).try_init();

    let Some(ctx) = GpuContext::try_new() else {
        assert!(
            std::env::var_os("GUIDO_GOLDEN_REQUIRED").is_none(),
            "this needs a Vulkan adapter and found none, and \
             GUIDO_GOLDEN_REQUIRED is set."
        );
        eprintln!("skipping: no Vulkan adapter on this machine");
        return;
    };

    let adapter = pollster::block_on(ctx.instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map(|a| a.get_info().name)
    .unwrap_or_else(|_| "<unknown>".to_string());

    // Counting glyphs does not depend on the rasterizer the way a golden does,
    // but the amount does: a different one antialiases a different number of
    // pixels past the threshold. The bar is "some glyphs", not "this many", so
    // it holds anywhere — and it still runs where the goldens run.
    if !is_software_rasterizer(&adapter) && std::env::var_os("GUIDO_GOLDEN_ANY_ADAPTER").is_none() {
        eprintln!("skipping: `{adapter}` is not the reference rasterizer");
        return;
    }

    guido::load_font(FONT.to_vec());
    let mut renderer = Renderer::new(ctx.device.clone(), ctx.queue.clone(), FORMAT);

    // Every fifth degree of the circle, and either side of the two angles the
    // issue names, so that a fix which merely moves the failure is visible.
    let mut angles: Vec<f32> = (0..72).map(|i| i as f32 * 5.0).collect();
    angles.extend([89.0, 89.9, 90.1, 91.0, 269.0, 269.9, 270.1, 271.0]);
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mut blank = Vec::new();
    for degrees in angles {
        let glyphs = glyph_pixels_of(&ctx, &mut renderer, scene_with(move |c| c.rotate(degrees)));
        if glyphs < 20 {
            blank.push((degrees, glyphs));
        }
    }
    drop(renderer);

    assert!(
        blank.is_empty(),
        "the label vanishes at {} angle(s): {blank:?}.\n\
         Rotated text does not go through glyphon — it is rasterised to a \
         texture and drawn as a quad — so this is that path, on `{adapter}`.",
        blank.len()
    );
}

/// The shortcut the fix touched still has to work: text scaled to nothing is
/// nothing to draw, and reading the scale correctly must not turn that into
/// work — or worse, into something visible.
#[test]
fn text_scaled_to_nothing_draws_nothing() {
    let _ = env_logger::builder().is_test(true).try_init();

    let Some(ctx) = GpuContext::try_new() else {
        eprintln!("skipping: no Vulkan adapter on this machine");
        return;
    };

    guido::load_font(FONT.to_vec());
    let mut renderer = Renderer::new(ctx.device.clone(), ctx.queue.clone(), FORMAT);

    for (sx, sy) in [(0.0, 1.0), (1.0, 0.0), (0.0, 0.0)] {
        let glyphs = glyph_pixels_of(
            &ctx,
            &mut renderer,
            scene_with(move |c| c.scale(Scale::new(sx, sy)).rotate(30.0)),
        );
        assert_eq!(
            glyphs, 0,
            "text scaled to ({sx}, {sy}) put {glyphs} glyph pixels on the surface"
        );
    }
    drop(renderer);
}
