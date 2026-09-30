//! Small images packed into shared pages, so a screen of icons samples a
//! handful of textures rather than one each (#546).
//!
//! A quad's draw call ends wherever the next quad samples through another bind
//! group, so a texture per icon was a draw call per icon. Icons that share a
//! page share its bind group, and a run of them is one call.
//!
//! **Pages, not one texture that grows.** A page is [`PAGE`] square and never
//! changes size; a full one opens another, and one whose last entry has gone
//! is dropped at the next trim. Nothing is ever copied from one texture to
//! another, and the textured-quad shader samples a plain 2D texture as it
//! always has. GTK's GPU renderer and Skia's atlas are built the same way.
//!
//! **A gutter of the entry's own edge.** The sampler is bilinear and clamps to
//! the edge, so a texel on an image's border is blended with itself outside
//! it. Packed next to another image it would be blended with that image
//! instead. Each entry is surrounded by one texel repeating its own border,
//! which is exactly what clamping showed, and the goldens drawn from textures
//! of their own do not move.

use std::cell::RefCell;
use std::rc::Rc;

use etagere::{AllocId, AtlasAllocator, size2};
use wgpu::{BindGroup, Device, Extent3d, Queue, Texture, TextureDimension, TextureUsages};

use super::textured_quad::TexturedQuadPipeline;

/// A page's side, in texels. 4 MB of RGBA each.
pub(super) const PAGE: u32 = 1024;

/// The largest image a page takes, per side. A 64 px icon, rasterized at the
/// SVG quality multiplier on a 2× output, is 256; anything larger — a
/// wallpaper, a photo — keeps a texture of its own.
pub(super) const MAX_ENTRY: u32 = 256;

/// The texels around each entry that repeat its edge.
const GUTTER: u32 = 1;

/// One page: its texture, the bind group every entry on it draws through, and
/// what is allocated on it.
struct Page {
    #[allow(dead_code)] // Kept alive for the bind group
    texture: Texture,
    bind_group: Rc<BindGroup>,
    allocator: RefCell<AtlasAllocator>,
}

/// An image's place on a page. Dropping it gives the space back.
pub(super) struct AtlasEntry {
    page: Rc<Page>,
    id: AllocId,
    /// The texels it holds, gutter included, counted against the cache budget.
    bytes: usize,
}

impl AtlasEntry {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn bind_group(&self) -> &Rc<BindGroup> {
        &self.page.bind_group
    }
}

impl Drop for AtlasEntry {
    fn drop(&mut self) {
        self.page.allocator.borrow_mut().deallocate(self.id);
    }
}

/// Every page there is.
#[derive(Default)]
pub(super) struct ImageAtlas {
    pages: Vec<Rc<Page>>,
}

impl ImageAtlas {
    /// Whether an image of this size goes on a page at all.
    pub(super) fn takes(width: u32, height: u32) -> bool {
        width <= MAX_ENTRY && height <= MAX_ENTRY
    }

    /// Place `rgba` on a page, opening one if none has room, and return the
    /// entry with the part of the page it occupies as `[x, y, width, height]`
    /// in texture coordinates — the image's own texels, not its gutter.
    pub(super) fn insert(
        &mut self,
        device: &Device,
        queue: &Queue,
        quad: &TexturedQuadPipeline,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Option<(AtlasEntry, [f32; 4])> {
        if !Self::takes(width, height) || width == 0 || height == 0 {
            return None;
        }
        let padded = size2((width + 2 * GUTTER) as i32, (height + 2 * GUTTER) as i32);

        let placed = self.pages.iter().find_map(|page| {
            let allocation = page.allocator.borrow_mut().allocate(padded)?;
            Some((page.clone(), allocation))
        });
        let (page, allocation) = match placed {
            Some(placed) => placed,
            None => {
                let page = Rc::new(new_page(device, quad));
                let allocation = page.allocator.borrow_mut().allocate(padded)?;
                self.pages.push(page.clone());
                (page, allocation)
            }
        };

        let origin = allocation.rectangle.min;
        let (x, y) = (origin.x as u32, origin.y as u32);
        let (padded_width, padded_height) = (width + 2 * GUTTER, height + 2 * GUTTER);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &page.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &extruded(width, height, rgba),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * padded_width),
                rows_per_image: Some(padded_height),
            },
            Extent3d {
                width: padded_width,
                height: padded_height,
                depth_or_array_layers: 1,
            },
        );

        let page_side = PAGE as f32;
        let rect = [
            (x + GUTTER) as f32 / page_side,
            (y + GUTTER) as f32 / page_side,
            width as f32 / page_side,
            height as f32 / page_side,
        ];
        let entry = AtlasEntry {
            page,
            id: allocation.id,
            bytes: (padded_width * padded_height * 4) as usize,
        };
        Some((entry, rect))
    }

    /// Drop every page no entry is on any more.
    pub(super) fn drop_empty_pages(&mut self) {
        self.pages
            .retain(|page| !page.allocator.borrow().is_empty());
    }

    /// How many pages there are.
    #[cfg(feature = "testing")]
    pub(super) fn pages(&self) -> usize {
        self.pages.len()
    }
}

fn new_page(device: &Device, quad: &TexturedQuadPipeline) -> Page {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Image Atlas Page"),
        size: Extent3d {
            width: PAGE,
            height: PAGE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Page {
        bind_group: quad.bind_texture(device, &view, "Image Atlas Page Bind Group"),
        texture,
        allocator: RefCell::new(AtlasAllocator::new(size2(PAGE as i32, PAGE as i32))),
    }
}

/// `rgba`, `width` by `height`, surrounded by one texel repeating its edge.
fn extruded(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let padded_width = width + 2;
    let mut out = Vec::with_capacity(padded_width * (height + 2) * 4);
    for row in 0..height + 2 {
        let source_row = row.saturating_sub(1).min(height - 1);
        let line = &rgba[source_row * width * 4..(source_row + 1) * width * 4];
        out.extend_from_slice(&line[..4]);
        out.extend_from_slice(line);
        out.extend_from_slice(&line[line.len() - 4..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::extruded;

    /// The gutter is the edge again, corners included: what `ClampToEdge`
    /// reads outside a texture of its own.
    #[test]
    fn the_gutter_repeats_the_edge() {
        let (a, b, c, d) = ([1, 1, 1, 1], [2, 2, 2, 2], [3, 3, 3, 3], [4, 4, 4, 4]);
        let rgba = [a, b, c, d].concat();

        let padded: Vec<[u8; 4]> = extruded(2, 2, &rgba)
            .chunks(4)
            .map(|texel| texel.try_into().unwrap())
            .collect();

        #[rustfmt::skip]
        let expected = vec![
            a, a, b, b,
            a, a, b, b,
            c, c, d, d,
            c, c, d, d,
        ];
        assert_eq!(padded, expected);
    }

    /// The same on an image that is neither square nor two wide, where adding
    /// two and doubling differ: one column of three rows becomes three columns
    /// of five.
    #[test]
    fn the_gutter_is_one_texel_whatever_the_shape() {
        let (a, b, c) = ([1, 1, 1, 1], [2, 2, 2, 2], [3, 3, 3, 3]);
        let rgba = [a, b, c].concat();

        let padded: Vec<[u8; 4]> = extruded(1, 3, &rgba)
            .chunks(4)
            .map(|texel| texel.try_into().unwrap())
            .collect();

        #[rustfmt::skip]
        let expected = vec![
            a, a, a,
            a, a, a,
            b, b, b,
            c, c, c,
            c, c, c,
        ];
        assert_eq!(padded, expected);
    }
}
